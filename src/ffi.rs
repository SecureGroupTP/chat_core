//! C ABI фасад для [`crate::service::MessengerMls`].
//!
//! JSON payload передаётся через байтовые буферы `(ptr, len)`. Методы, которые
//! возвращают данные, записывают сериализованный JSON в [`crate::ffi::MlsBuffer`].

use crate::service::MessengerMls;
use crate::types::{
    Bytes, CreateClientParams, Error, GroupId, IncomingMessage, InviteRequest, KeyPackageBundle,
    RemoveRequest, StatusCode,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::ptr;
use std::slice;
use std::sync::Mutex;

#[repr(C)]
#[derive(Clone, Copy)]
/// Дескриптор исходящего байтового буфера, выделенного библиотекой.
pub struct MlsBuffer {
    /// Указатель на выделенные байты (`null`, если буфер пуст).
    pub ptr: *mut u8,
    /// Количество байтов, доступных по [`Self::ptr`].
    pub len: usize,
}

impl Default for MlsBuffer {
    fn default() -> Self {
        Self {
            ptr: ptr::null_mut(),
            len: 0,
        }
    }
}

/// Непрозрачный handle для C-клиентов.
///
/// Создаётся через [`messenger_mls_new`], освобождается через [`messenger_mls_free`].
pub struct MessengerMlsHandle {
    inner: Mutex<MessengerMls>,
    last_error: Mutex<String>,
}

#[derive(Debug, Clone, Deserialize)]
/// JSON-тело запроса для `messenger_mls_encrypt_message`.
struct EncryptMessageRequest {
    group_id: GroupId,
    plaintext: Bytes,
    #[serde(default)]
    aad: Bytes,
}

/// Сохраняет форматированную ошибку в локальный `last_error` внутри handle.
fn set_last_error(handle: &MessengerMlsHandle, error: &Error) {
    if let Ok(mut slot) = handle.last_error.lock() {
        *slot = format!("{}: {}", error.code as u32, error.message);
    }
}

/// Записывает владение байтами в дескриптор выходного буфера.
fn write_out(out: *mut MlsBuffer, data: Vec<u8>) -> StatusCode {
    if out.is_null() {
        return StatusCode::InvalidArgument;
    }

    // SAFETY: caller provides a valid pointer to output buffer descriptor.
    unsafe {
        (*out).ptr = ptr::null_mut();
        (*out).len = 0;
    }

    if data.is_empty() {
        return StatusCode::Ok;
    }

    let mut boxed = data.into_boxed_slice();
    let ptr = boxed.as_mut_ptr();
    let len = boxed.len();
    std::mem::forget(boxed);

    // SAFETY: caller provides a valid pointer to output buffer descriptor.
    unsafe {
        (*out).ptr = ptr;
        (*out).len = len;
    }

    StatusCode::Ok
}

/// Читает входные байты вызывающей стороны из пары сырого указателя и длины.
fn read_input(input_ptr: *const u8, input_len: usize) -> Result<Vec<u8>, Error> {
    if input_len == 0 {
        return Ok(Vec::new());
    }
    if input_ptr.is_null() {
        return Err(Error::new(
            StatusCode::InvalidArgument,
            "input_ptr is null while input_len > 0",
        ));
    }

    // SAFETY: input pointer and length are validated by the checks above.
    let bytes = unsafe { slice::from_raw_parts(input_ptr, input_len) };
    Ok(bytes.to_vec())
}

/// Парсит JSON payload из входа в формате указатель/длина.
fn parse_json<T: DeserializeOwned>(input_ptr: *const u8, input_len: usize) -> Result<T, Error> {
    let bytes = read_input(input_ptr, input_len)?;
    if bytes.is_empty() {
        return Err(Error::new(
            StatusCode::InvalidArgument,
            "input json is empty",
        ));
    }
    serde_json::from_slice(&bytes)
        .map_err(|e| Error::new(StatusCode::InvalidArgument, format!("invalid json: {e}")))
}

/// Кодирует значение в JSON-вектор байтов.
fn encode_json<T: Serialize>(value: &T) -> Result<Vec<u8>, Error> {
    serde_json::to_vec(value)
        .map_err(|e| Error::new(StatusCode::InternalError, format!("encode json: {e}")))
}

/// Вспомогательная обёртка для FFI-методов, возвращающих только код статуса.
fn run_void<F>(handle: *mut MessengerMlsHandle, f: F) -> u32
where
    F: FnOnce(&mut MessengerMls) -> Result<(), Error>,
{
    if handle.is_null() {
        return StatusCode::InvalidArgument as u32;
    }

    // SAFETY: handle is checked for null and created by messenger_mls_new.
    let handle_ref = unsafe { &*handle };
    let mut guard = match handle_ref.inner.lock() {
        Ok(g) => g,
        Err(_) => return StatusCode::InternalError as u32,
    };

    match f(&mut guard) {
        Ok(()) => {
            if let Ok(mut slot) = handle_ref.last_error.lock() {
                slot.clear();
            }
            StatusCode::Ok as u32
        }
        Err(e) => {
            set_last_error(handle_ref, &e);
            e.code as u32
        }
    }
}

/// Вспомогательная обёртка для FFI-методов, возвращающих JSON через [`MlsBuffer`].
fn run_out<F, T>(handle: *mut MessengerMlsHandle, out: *mut MlsBuffer, f: F) -> u32
where
    F: FnOnce(&mut MessengerMls) -> Result<T, Error>,
    T: Serialize,
{
    if handle.is_null() {
        return StatusCode::InvalidArgument as u32;
    }
    if out.is_null() {
        return StatusCode::InvalidArgument as u32;
    }

    // SAFETY: handle is checked for null and created by messenger_mls_new.
    let handle_ref = unsafe { &*handle };
    let mut guard = match handle_ref.inner.lock() {
        Ok(g) => g,
        Err(_) => return StatusCode::InternalError as u32,
    };

    match f(&mut guard)
        .and_then(|value| encode_json(&value))
        .map(|json| write_out(out, json))
    {
        Ok(code) => {
            if let Ok(mut slot) = handle_ref.last_error.lock() {
                slot.clear();
            }
            code as u32
        }
        Err(e) => {
            set_last_error(handle_ref, &e);
            e.code as u32
        }
    }
}

/// Вспомогательная обёртка для FFI-методов, возвращающих сырые байты через [`MlsBuffer`].
fn run_out_bytes<F>(handle: *mut MessengerMlsHandle, out: *mut MlsBuffer, f: F) -> u32
where
    F: FnOnce(&mut MessengerMls) -> Result<Bytes, Error>,
{
    if handle.is_null() {
        return StatusCode::InvalidArgument as u32;
    }
    if out.is_null() {
        return StatusCode::InvalidArgument as u32;
    }

    // SAFETY: handle is checked for null and created by messenger_mls_new.
    let handle_ref = unsafe { &*handle };
    let mut guard = match handle_ref.inner.lock() {
        Ok(g) => g,
        Err(_) => return StatusCode::InternalError as u32,
    };

    match f(&mut guard).map(|bytes| write_out(out, bytes)) {
        Ok(code) => {
            if let Ok(mut slot) = handle_ref.last_error.lock() {
                slot.clear();
            }
            code as u32
        }
        Err(e) => {
            set_last_error(handle_ref, &e);
            e.code as u32
        }
    }
}

#[unsafe(no_mangle)]
/// Выделяет новый handle мессенджера.
///
/// Возвращённый указатель нужно освободить через [`messenger_mls_free`].
pub extern "C" fn messenger_mls_new() -> *mut MessengerMlsHandle {
    let handle = MessengerMlsHandle {
        inner: Mutex::new(MessengerMls::new()),
        last_error: Mutex::new(String::new()),
    };
    Box::into_raw(Box::new(handle))
}

#[unsafe(no_mangle)]
/// Освобождает handle, ранее возвращённый [`messenger_mls_new`].
///
/// # Safety
/// `handle` должен быть указателем, ранее возвращённым [`messenger_mls_new`],
/// и не должен использоваться после этого вызова.
pub unsafe extern "C" fn messenger_mls_free(handle: *mut MessengerMlsHandle) {
    if handle.is_null() {
        return;
    }
    // SAFETY: pointer originates from Box::into_raw in messenger_mls_new.
    drop(unsafe { Box::from_raw(handle) });
}

#[unsafe(no_mangle)]
/// Освобождает буфер, ранее возвращённый через `out: *mut MlsBuffer`.
///
/// # Safety
/// `buf` должен быть буфером, возвращённым этой библиотекой через out-параметр,
/// и не должен освобождаться более одного раза.
pub unsafe extern "C" fn messenger_mls_buffer_free(buf: MlsBuffer) {
    if buf.ptr.is_null() {
        return;
    }

    // SAFETY: ptr/len come from write_out allocation.
    let _ = unsafe { Vec::from_raw_parts(buf.ptr, buf.len, buf.len) };
}

#[unsafe(no_mangle)]
/// Возвращает строку последней ошибки (`"<code>: <message>"`) для handle.
///
/// # Safety
/// `handle` должен быть валидным указателем, полученным из [`messenger_mls_new`],
/// а `out` должен быть валидным записываемым указателем на `MlsBuffer`.
pub unsafe extern "C" fn messenger_mls_last_error(
    handle: *mut MessengerMlsHandle,
    out: *mut MlsBuffer,
) -> u32 {
    if handle.is_null() {
        return StatusCode::InvalidArgument as u32;
    }
    if out.is_null() {
        return StatusCode::InvalidArgument as u32;
    }

    // SAFETY: handle is checked for null and created by messenger_mls_new.
    let handle_ref = unsafe { &*handle };
    let msg = match handle_ref.last_error.lock() {
        Ok(m) => m.clone(),
        Err(_) => return StatusCode::InternalError as u32,
    };

    write_out(out, msg.into_bytes()) as u32
}

#[unsafe(no_mangle)]
/// Инициализирует идентичность клиента из JSON-кодированного [`CreateClientParams`].
///
/// Возвращает числовой [`StatusCode`].
pub extern "C" fn messenger_mls_create_client(
    handle: *mut MessengerMlsHandle,
    input_ptr: *const u8,
    input_len: usize,
) -> u32 {
    run_void(handle, |svc| {
        let params: CreateClientParams = parse_json(input_ptr, input_len)?;
        svc.create_client(params)
    })
}

#[unsafe(no_mangle)]
/// Восстанавливает runtime клиента из байтов, полученных в `export_client_state`.
///
/// Возвращает числовой [`StatusCode`].
pub extern "C" fn messenger_mls_restore_client(
    handle: *mut MessengerMlsHandle,
    input_ptr: *const u8,
    input_len: usize,
) -> u32 {
    run_void(handle, |svc| {
        let bytes = read_input(input_ptr, input_len)?;
        svc.restore_client(&bytes)
    })
}

#[unsafe(no_mangle)]
/// Экспортирует сериализованное состояние клиента как сырые JSON-байты.
///
/// # Safety
/// `handle` должен быть валидным указателем, полученным из [`messenger_mls_new`],
/// а `out` должен быть валидным записываемым указателем на `MlsBuffer`.
pub unsafe extern "C" fn messenger_mls_export_client_state(
    handle: *mut MessengerMlsHandle,
    out: *mut MlsBuffer,
) -> u32 {
    run_out_bytes(handle, out, |svc| svc.export_client_state())
}

#[unsafe(no_mangle)]
/// Возвращает JSON-кодированный [`crate::types::ClientId`] текущего клиента.
pub extern "C" fn messenger_mls_get_client_id(
    handle: *mut MessengerMlsHandle,
    out: *mut MlsBuffer,
) -> u32 {
    run_out(handle, out, |svc| svc.get_client_id())
}

#[unsafe(no_mangle)]
/// Генерирует `count` key package и возвращает JSON-кодированный набор.
pub extern "C" fn messenger_mls_create_key_packages(
    handle: *mut MessengerMlsHandle,
    count: u32,
    out: *mut MlsBuffer,
) -> u32 {
    run_out(handle, out, |svc| svc.create_key_packages(count))
}

#[unsafe(no_mangle)]
/// Помечает JSON-кодированный [`KeyPackageBundle`] как загруженный.
pub extern "C" fn messenger_mls_mark_key_packages_uploaded(
    handle: *mut MessengerMlsHandle,
    input_ptr: *const u8,
    input_len: usize,
) -> u32 {
    run_void(handle, |svc| {
        let bundle: KeyPackageBundle = parse_json(input_ptr, input_len)?;
        svc.mark_key_packages_uploaded(bundle)
    })
}

#[unsafe(no_mangle)]
/// Создаёт группу из JSON-кодированного [`GroupId`] и возвращает JSON-состояние.
pub extern "C" fn messenger_mls_create_group(
    handle: *mut MessengerMlsHandle,
    input_ptr: *const u8,
    input_len: usize,
    out: *mut MlsBuffer,
) -> u32 {
    run_out(handle, out, |svc| {
        let group_id: GroupId = parse_json(input_ptr, input_len)?;
        svc.create_group(group_id)
    })
}

#[unsafe(no_mangle)]
/// Возвращает JSON-массив известных состояний групп.
pub extern "C" fn messenger_mls_list_groups(
    handle: *mut MessengerMlsHandle,
    out: *mut MlsBuffer,
) -> u32 {
    run_out(handle, out, |svc| svc.list_groups())
}

#[unsafe(no_mangle)]
/// Возвращает JSON-кодированное состояние группы для входного JSON [`GroupId`].
pub extern "C" fn messenger_mls_get_group_state(
    handle: *mut MessengerMlsHandle,
    input_ptr: *const u8,
    input_len: usize,
    out: *mut MlsBuffer,
) -> u32 {
    run_out(handle, out, |svc| {
        let group_id: GroupId = parse_json(input_ptr, input_len)?;
        svc.get_group_state(group_id)
    })
}

#[unsafe(no_mangle)]
/// Возвращает JSON-список участников для входного JSON [`GroupId`].
pub extern "C" fn messenger_mls_list_members(
    handle: *mut MessengerMlsHandle,
    input_ptr: *const u8,
    input_len: usize,
    out: *mut MlsBuffer,
) -> u32 {
    run_out(handle, out, |svc| {
        let group_id: GroupId = parse_json(input_ptr, input_len)?;
        svc.list_members(group_id)
    })
}

#[unsafe(no_mangle)]
/// Приглашает участника из JSON-кодированного [`InviteRequest`].
pub extern "C" fn messenger_mls_invite(
    handle: *mut MessengerMlsHandle,
    input_ptr: *const u8,
    input_len: usize,
    out: *mut MlsBuffer,
) -> u32 {
    run_out(handle, out, |svc| {
        let req: InviteRequest = parse_json(input_ptr, input_len)?;
        svc.invite(req)
    })
}

#[unsafe(no_mangle)]
/// Вступает в группу по сырым байтам Welcome-сообщения и возвращает JSON состояния группы.
pub extern "C" fn messenger_mls_join_from_welcome(
    handle: *mut MessengerMlsHandle,
    welcome_ptr: *const u8,
    welcome_len: usize,
    out: *mut MlsBuffer,
) -> u32 {
    run_out(handle, out, |svc| {
        let welcome = read_input(welcome_ptr, welcome_len)?;
        svc.join_from_welcome(&welcome)
    })
}

#[unsafe(no_mangle)]
/// Удаляет участника из JSON-кодированного [`RemoveRequest`].
pub extern "C" fn messenger_mls_remove(
    handle: *mut MessengerMlsHandle,
    input_ptr: *const u8,
    input_len: usize,
    out: *mut MlsBuffer,
) -> u32 {
    run_out(handle, out, |svc| {
        let req: RemoveRequest = parse_json(input_ptr, input_len)?;
        svc.remove(req)
    })
}

#[unsafe(no_mangle)]
/// Выполняет self-update для JSON-кодированного [`GroupId`].
pub extern "C" fn messenger_mls_self_update(
    handle: *mut MessengerMlsHandle,
    input_ptr: *const u8,
    input_len: usize,
    out: *mut MlsBuffer,
) -> u32 {
    run_out(handle, out, |svc| {
        let group_id: GroupId = parse_json(input_ptr, input_len)?;
        svc.self_update(group_id)
    })
}

#[unsafe(no_mangle)]
/// Шифрует сообщение из JSON-запроса `{ group_id, plaintext, aad }`.
pub extern "C" fn messenger_mls_encrypt_message(
    handle: *mut MessengerMlsHandle,
    input_ptr: *const u8,
    input_len: usize,
    out: *mut MlsBuffer,
) -> u32 {
    run_out(handle, out, |svc| {
        let req: EncryptMessageRequest = parse_json(input_ptr, input_len)?;
        svc.encrypt_message(req.group_id, req.plaintext, req.aad)
    })
}

#[unsafe(no_mangle)]
/// Обрабатывает JSON-кодированное [`IncomingMessage`] и возвращает JSON-события.
pub extern "C" fn messenger_mls_handle_incoming(
    handle: *mut MessengerMlsHandle,
    input_ptr: *const u8,
    input_len: usize,
    out: *mut MlsBuffer,
) -> u32 {
    run_out(handle, out, |svc| {
        let req: IncomingMessage = parse_json(input_ptr, input_len)?;
        svc.handle_incoming(req)
    })
}

#[unsafe(no_mangle)]
/// Возвращает JSON-boolean о состоянии pending commit для JSON [`GroupId`].
pub extern "C" fn messenger_mls_has_pending_commit(
    handle: *mut MessengerMlsHandle,
    input_ptr: *const u8,
    input_len: usize,
    out: *mut MlsBuffer,
) -> u32 {
    run_out(handle, out, |svc| {
        let group_id: GroupId = parse_json(input_ptr, input_len)?;
        svc.has_pending_commit(group_id)
    })
}

#[unsafe(no_mangle)]
/// Очищает pending commit для JSON-кодированного [`GroupId`].
pub extern "C" fn messenger_mls_clear_pending_commit(
    handle: *mut MessengerMlsHandle,
    input_ptr: *const u8,
    input_len: usize,
) -> u32 {
    run_void(handle, |svc| {
        let group_id: GroupId = parse_json(input_ptr, input_len)?;
        svc.clear_pending_commit(group_id)
    })
}

#[unsafe(no_mangle)]
/// Удаляет группу для JSON-кодированного [`GroupId`].
pub extern "C" fn messenger_mls_drop_group(
    handle: *mut MessengerMlsHandle,
    input_ptr: *const u8,
    input_len: usize,
) -> u32 {
    run_void(handle, |svc| {
        let group_id: GroupId = parse_json(input_ptr, input_len)?;
        svc.drop_group(group_id)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic;

    fn make_params() -> CreateClientParams {
        CreateClientParams {
            client_id: crate::types::ClientId {
                user_id: "ffi-ut".to_string(),
                device_id: "phone".to_string(),
            },
            device_signature_private_key: vec![7; 32],
            binding: crate::types::DeviceBinding {
                client_id: crate::types::ClientId {
                    user_id: "ffi-ut".to_string(),
                    device_id: "phone".to_string(),
                },
                serialized_binding: vec![],
                account_signature: vec![],
            },
            identity_data: vec![],
        }
    }

    #[test]
    fn helper_paths_and_poisoned_mutex_branches() {
        // write_out null branch.
        assert_eq!(
            write_out(std::ptr::null_mut(), vec![1, 2]),
            StatusCode::InvalidArgument
        );

        // read_input len=0 branch.
        assert_eq!(read_input(std::ptr::null(), 0).expect("empty input"), b"");
        // read_input null pointer with non-zero len.
        assert_eq!(
            read_input(std::ptr::null(), 1)
                .expect_err("null pointer")
                .code,
            StatusCode::InvalidArgument
        );

        // run_void/run_out null handle branches.
        assert_eq!(
            run_void(std::ptr::null_mut(), |_svc| Ok(())),
            StatusCode::InvalidArgument as u32
        );
        let dummy = serde_json::json!({});
        assert_eq!(
            run_out::<_, serde_json::Value>(std::ptr::null_mut(), std::ptr::null_mut(), |_svc| {
                Ok(dummy.clone())
            }),
            StatusCode::InvalidArgument as u32
        );
        assert_eq!(
            run_out_bytes(std::ptr::null_mut(), std::ptr::null_mut(), |_svc| Ok(
                vec![]
            )),
            StatusCode::InvalidArgument as u32
        );
        let handle_ok = messenger_mls_new();
        assert!(!handle_ok.is_null());
        let mut out_ok = MlsBuffer::default();
        let ok_code =
            run_out::<_, serde_json::Value>(handle_ok, &mut out_ok as *mut MlsBuffer, |_svc| {
                Ok(dummy.clone())
            });
        assert_eq!(ok_code, StatusCode::Ok as u32);
        // SAFETY: buffer was allocated by write_out inside run_out.
        unsafe { messenger_mls_buffer_free(out_ok) };
        // SAFETY: free exactly once.
        unsafe { messenger_mls_free(handle_ok) };

        // Poison inner mutex to trigger InternalError branch.
        let handle = messenger_mls_new();
        assert!(!handle.is_null());
        // SAFETY: handle is valid and points to allocated MessengerMlsHandle.
        let href = unsafe { &*handle };
        let _ = panic::catch_unwind(|| {
            let _guard = href.inner.lock().expect("lock inner");
            panic!("poison inner mutex");
        });
        let code_poison = messenger_mls_create_client(handle, std::ptr::null(), 0);
        assert_eq!(code_poison, StatusCode::InternalError as u32);
        let mut out_buf = MlsBuffer::default();
        let code_poison_out = messenger_mls_get_client_id(handle, &mut out_buf as *mut MlsBuffer);
        assert_eq!(code_poison_out, StatusCode::InternalError as u32);
        let mut out_poison_export = MlsBuffer::default();
        let code_poison_export =
            unsafe { messenger_mls_export_client_state(handle, &mut out_poison_export) };
        assert_eq!(code_poison_export, StatusCode::InternalError as u32);

        // Poison last_error mutex to trigger messenger_mls_last_error lock error branch.
        let _ = panic::catch_unwind(|| {
            let _guard = href.last_error.lock().expect("lock error slot");
            panic!("poison last_error mutex");
        });
        let mut out = MlsBuffer::default();
        let code_last_err = unsafe { messenger_mls_last_error(handle, &mut out as *mut MlsBuffer) };
        assert_eq!(code_last_err, StatusCode::InternalError as u32);

        // SAFETY: free exactly once.
        unsafe { messenger_mls_free(handle) };

        // Null-pointer edge branches for free helpers.
        // SAFETY: null free is a no-op by contract.
        unsafe { messenger_mls_free(std::ptr::null_mut()) };
        // SAFETY: default/empty buffer free is a no-op by contract.
        unsafe { messenger_mls_buffer_free(MlsBuffer::default()) };
    }

    #[test]
    fn exported_functions_edge_paths() {
        let handle = messenger_mls_new();
        assert!(!handle.is_null());

        // Null out pointer for exporter should fail with InvalidArgument.
        let code = unsafe { messenger_mls_export_client_state(handle, std::ptr::null_mut()) };
        assert_eq!(code, StatusCode::InvalidArgument as u32);
        // SAFETY: null handle branch.
        assert_eq!(
            unsafe {
                messenger_mls_export_client_state(std::ptr::null_mut(), std::ptr::null_mut())
            },
            StatusCode::InvalidArgument as u32
        );
        // SAFETY: null handle branch for last_error.
        assert_eq!(
            unsafe { messenger_mls_last_error(std::ptr::null_mut(), std::ptr::null_mut()) },
            StatusCode::InvalidArgument as u32
        );
        // SAFETY: valid handle but null out branch.
        assert_eq!(
            unsafe { messenger_mls_last_error(handle, std::ptr::null_mut()) },
            StatusCode::InvalidArgument as u32
        );

        let req = serde_json::to_vec(&make_params()).expect("serialize");
        assert_eq!(
            messenger_mls_create_client(handle, req.as_ptr(), req.len()),
            StatusCode::Ok as u32
        );

        // Null out for run_out-based API should fail fast without mutating state.
        let code_kp_null_out = messenger_mls_create_key_packages(handle, 1, std::ptr::null_mut());
        assert_eq!(code_kp_null_out, StatusCode::InvalidArgument as u32);
        let mut exported_after_null_out = MlsBuffer::default();
        let export_code =
            unsafe { messenger_mls_export_client_state(handle, &mut exported_after_null_out) };
        assert_eq!(export_code, StatusCode::Ok as u32);
        // SAFETY: buffer was allocated by messenger_mls_export_client_state.
        let exported_state = unsafe {
            std::slice::from_raw_parts(exported_after_null_out.ptr, exported_after_null_out.len)
        };
        let persisted: crate::state::PersistedClientState =
            serde_json::from_slice(exported_state).expect("decode persisted");
        assert_eq!(persisted.key_package_counter, 0);
        // SAFETY: free buffer once.
        unsafe { messenger_mls_buffer_free(exported_after_null_out) };

        // Ensure remove wrapper path is exercised.
        let remove_req = serde_json::to_vec(&crate::types::RemoveRequest {
            group_id: GroupId {
                value: b"missing-remove".to_vec(),
            },
            removed_client: crate::types::ClientId {
                user_id: "nobody".to_string(),
                device_id: "x".to_string(),
            },
        })
        .expect("remove req");
        let mut out = MlsBuffer::default();
        let code_remove = messenger_mls_remove(
            handle,
            remove_req.as_ptr(),
            remove_req.len(),
            &mut out as *mut MlsBuffer,
        );
        assert_eq!(code_remove, StatusCode::NotFound as u32);

        // Empty JSON is now rejected explicitly.
        let code_empty_json = messenger_mls_get_group_state(handle, std::ptr::null(), 0, &mut out);
        assert_eq!(code_empty_json, StatusCode::InvalidArgument as u32);

        // SAFETY: free exactly once.
        unsafe { messenger_mls_free(handle) };
    }
}
