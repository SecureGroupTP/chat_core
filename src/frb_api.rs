use crate::service::MessengerMls;
use crate::types::{
    Bytes, CreateClientParams, Error, GroupId, IncomingMessage, InviteRequest, KeyPackageBundle,
    RemoveRequest, StatusCode,
};
use flutter_rust_bridge::frb;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::panic::{self, AssertUnwindSafe};
use std::sync::Mutex;

#[derive(Debug, Clone)]
pub struct ApiStatus {
    pub code: u32,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct ApiJsonResponse {
    pub code: u32,
    pub message: String,
    pub json: String,
}

#[derive(Debug, Clone)]
pub struct ApiBytesResponse {
    pub code: u32,
    pub message: String,
    pub data: Bytes,
}

#[derive(Debug, Clone, Deserialize)]
struct EncryptMessageRequest {
    group_id: GroupId,
    plaintext: Bytes,
    #[serde(default)]
    aad: Bytes,
}

#[frb(opaque)]
pub struct MessengerMlsBridge {
    inner: Mutex<MessengerMls>,
}

impl MessengerMlsBridge {
    #[frb(sync)]
    pub fn create() -> Self {
        Self {
            inner: Mutex::new(MessengerMls::new()),
        }
    }

    #[frb(sync)]
    pub fn create_client(&self, input_json: String) -> ApiStatus {
        into_status(catch_api_panic(|| {
            let params: CreateClientParams = parse_json(&input_json)?;
            with_service(self, |svc| svc.create_client(params))
        }))
    }

    #[frb(sync)]
    pub fn restore_client(&self, data: Bytes) -> ApiStatus {
        into_status(catch_api_panic(|| with_service(self, |svc| svc.restore_client(&data))))
    }

    #[frb(sync)]
    pub fn export_client_state(&self) -> ApiBytesResponse {
        into_bytes(catch_api_panic(|| with_service(self, |svc| svc.export_client_state())))
    }

    #[frb(sync)]
    pub fn get_client_id(&self) -> ApiJsonResponse {
        into_json(catch_api_panic(|| with_service(self, |svc| svc.get_client_id())))
    }

    #[frb(sync)]
    pub fn create_key_packages(&self, count: u32) -> ApiJsonResponse {
        into_json(catch_api_panic(|| with_service(self, |svc| svc.create_key_packages(count))))
    }

    #[frb(sync)]
    pub fn mark_key_packages_uploaded(&self, input_json: String) -> ApiStatus {
        into_status(catch_api_panic(|| {
            let bundle: KeyPackageBundle = parse_json(&input_json)?;
            with_service(self, |svc| svc.mark_key_packages_uploaded(bundle))
        }))
    }

    #[frb(sync)]
    pub fn create_group(&self, input_json: String) -> ApiJsonResponse {
        into_json(catch_api_panic(|| {
            let group_id: GroupId = parse_json(&input_json)?;
            with_service(self, |svc| svc.create_group(group_id))
        }))
    }

    #[frb(sync)]
    pub fn list_groups(&self) -> ApiJsonResponse {
        into_json(catch_api_panic(|| with_service(self, |svc| svc.list_groups())))
    }

    #[frb(sync)]
    pub fn get_group_state(&self, input_json: String) -> ApiJsonResponse {
        into_json(catch_api_panic(|| {
            let group_id: GroupId = parse_json(&input_json)?;
            with_service(self, |svc| svc.get_group_state(group_id))
        }))
    }

    #[frb(sync)]
    pub fn list_members(&self, input_json: String) -> ApiJsonResponse {
        into_json(catch_api_panic(|| {
            let group_id: GroupId = parse_json(&input_json)?;
            with_service(self, |svc| svc.list_members(group_id))
        }))
    }

    #[frb(sync)]
    pub fn invite(&self, input_json: String) -> ApiJsonResponse {
        into_json(catch_api_panic(|| {
            let request: InviteRequest = parse_json(&input_json)?;
            with_service(self, |svc| svc.invite(request))
        }))
    }

    #[frb(sync)]
    pub fn join_from_welcome(&self, welcome_message: Bytes) -> ApiJsonResponse {
        into_json(catch_api_panic(|| {
            with_service(self, |svc| svc.join_from_welcome(&welcome_message))
        }))
    }

    #[frb(sync)]
    pub fn remove(&self, input_json: String) -> ApiJsonResponse {
        into_json(catch_api_panic(|| {
            let request: RemoveRequest = parse_json(&input_json)?;
            with_service(self, |svc| svc.remove(request))
        }))
    }

    #[frb(sync)]
    pub fn self_update(&self, input_json: String) -> ApiJsonResponse {
        into_json(catch_api_panic(|| {
            let group_id: GroupId = parse_json(&input_json)?;
            with_service(self, |svc| svc.self_update(group_id))
        }))
    }

    #[frb(sync)]
    pub fn encrypt_message(&self, input_json: String) -> ApiJsonResponse {
        into_json(catch_api_panic(|| {
            let request: EncryptMessageRequest = parse_json(&input_json)?;
            with_service(self, |svc| {
                svc.encrypt_message(request.group_id, request.plaintext, request.aad)
            })
        }))
    }

    #[frb(sync)]
    pub fn handle_incoming(&self, input_json: String) -> ApiJsonResponse {
        into_json(catch_api_panic(|| {
            let message: IncomingMessage = parse_json(&input_json)?;
            with_service(self, |svc| svc.handle_incoming(message))
        }))
    }

    #[frb(sync)]
    pub fn has_pending_commit(&self, input_json: String) -> ApiJsonResponse {
        into_json(catch_api_panic(|| {
            let group_id: GroupId = parse_json(&input_json)?;
            with_service(self, |svc| svc.has_pending_commit(group_id))
        }))
    }

    #[frb(sync)]
    pub fn merge_pending_commit(&self, input_json: String) -> ApiJsonResponse {
        into_json(catch_api_panic(|| {
            let group_id: GroupId = parse_json(&input_json)?;
            with_service(self, |svc| svc.merge_pending_commit(group_id))
        }))
    }

    #[frb(sync)]
    pub fn clear_pending_commit(&self, input_json: String) -> ApiStatus {
        into_status(catch_api_panic(|| {
            let group_id: GroupId = parse_json(&input_json)?;
            with_service(self, |svc| svc.clear_pending_commit(group_id))
        }))
    }

    #[frb(sync)]
    pub fn drop_group(&self, input_json: String) -> ApiStatus {
        into_status(catch_api_panic(|| {
            let group_id: GroupId = parse_json(&input_json)?;
            with_service(self, |svc| svc.drop_group(group_id))
        }))
    }
}

fn with_service<T>(
    bridge: &MessengerMlsBridge,
    f: impl FnOnce(&mut MessengerMls) -> Result<T, Error>,
) -> Result<T, Error> {
    let mut guard = bridge.inner.lock().map_err(|_| {
        Error::new(
            StatusCode::InternalError,
            "messenger bridge mutex is poisoned",
        )
    })?;
    f(&mut guard)
}

fn parse_json<T: DeserializeOwned>(input_json: &str) -> Result<T, Error> {
    if input_json.is_empty() {
        return Err(Error::new(
            StatusCode::InvalidArgument,
            "input json is empty",
        ));
    }
    serde_json::from_str(input_json)
        .map_err(|e| Error::new(StatusCode::InvalidArgument, format!("invalid json: {e}")))
}

fn encode_json<T: Serialize>(value: &T) -> Result<String, Error> {
    serde_json::to_string(value)
        .map_err(|e| Error::new(StatusCode::InternalError, format!("encode json: {e}")))
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<String>() {
        return message.clone();
    }
    if let Some(message) = payload.downcast_ref::<&'static str>() {
        return (*message).to_string();
    }
    "panic without string payload".to_string()
}

fn catch_api_panic<T>(f: impl FnOnce() -> Result<T, Error>) -> Result<T, Error> {
    panic::catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|payload| {
        Err(Error::new(
            StatusCode::InternalError,
            format!("panic across flutter_rust_bridge boundary: {}", panic_message(payload)),
        ))
    })
}

fn into_status(result: Result<(), Error>) -> ApiStatus {
    match result {
        Ok(()) => ApiStatus {
            code: StatusCode::Ok as u32,
            message: String::new(),
        },
        Err(error) => ApiStatus {
            code: error.code as u32,
            message: error.message,
        },
    }
}

fn into_json<T: Serialize>(result: Result<T, Error>) -> ApiJsonResponse {
    match result.and_then(|value| encode_json(&value)) {
        Ok(json) => ApiJsonResponse {
            code: StatusCode::Ok as u32,
            message: String::new(),
            json,
        },
        Err(error) => ApiJsonResponse {
            code: error.code as u32,
            message: error.message,
            json: String::new(),
        },
    }
}

fn into_bytes(result: Result<Bytes, Error>) -> ApiBytesResponse {
    match result {
        Ok(data) => ApiBytesResponse {
            code: StatusCode::Ok as u32,
            message: String::new(),
            data,
        },
        Err(error) => ApiBytesResponse {
            code: error.code as u32,
            message: error.message,
            data: Vec::new(),
        },
    }
}