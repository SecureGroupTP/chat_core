use chat_core::ffi::{
    MlsBuffer, messenger_mls_buffer_free, messenger_mls_clear_pending_commit,
    messenger_mls_create_client, messenger_mls_create_group, messenger_mls_create_key_packages,
    messenger_mls_drop_group, messenger_mls_encrypt_message, messenger_mls_export_client_state,
    messenger_mls_free, messenger_mls_get_client_id, messenger_mls_get_group_state,
    messenger_mls_handle_incoming, messenger_mls_has_pending_commit, messenger_mls_invite,
    messenger_mls_join_from_welcome, messenger_mls_last_error, messenger_mls_list_groups,
    messenger_mls_list_members, messenger_mls_mark_key_packages_uploaded, messenger_mls_new,
    messenger_mls_remove, messenger_mls_restore_client, messenger_mls_self_update,
};
use chat_core::{
    ClientId, CreateClientParams, DeviceBinding, GroupId, GroupState, IncomingMessage,
    IncomingMessageKind, InviteRequest, InviteResult, KeyPackageBundle, RemoveRequest,
    SelfUpdateResult, StatusCode,
};
use serde::de::DeserializeOwned;

fn make_params(user: &str, device: &str, seed: u8) -> CreateClientParams {
    CreateClientParams {
        client_id: ClientId {
            user_id: user.to_string(),
            device_id: device.to_string(),
        },
        device_signature_private_key: vec![seed; 32],
        binding: DeviceBinding {
            client_id: ClientId {
                user_id: user.to_string(),
                device_id: device.to_string(),
            },
            serialized_binding: Vec::new(),
            account_signature: Vec::new(),
        },
        identity_data: Vec::new(),
    }
}

fn read_buf_as_vec(buf: MlsBuffer) -> Vec<u8> {
    if buf.ptr.is_null() || buf.len == 0 {
        return Vec::new();
    }

    // SAFETY: buffer is allocated and owned by the FFI layer for this call.
    let bytes = unsafe { std::slice::from_raw_parts(buf.ptr, buf.len) }.to_vec();

    // SAFETY: buffer is returned by chat_core FFI and must be freed exactly once.
    unsafe { messenger_mls_buffer_free(buf) };
    bytes
}

fn read_json<T: DeserializeOwned>(buf: MlsBuffer) -> T {
    serde_json::from_slice(&read_buf_as_vec(buf)).expect("decode json from MlsBuffer")
}

fn call_out_json<T: DeserializeOwned>(
    handle: *mut chat_core::ffi::MessengerMlsHandle,
    f: impl FnOnce(*mut chat_core::ffi::MessengerMlsHandle, *mut MlsBuffer) -> u32,
) -> (u32, Option<T>) {
    let mut out = MlsBuffer::default();
    let code = f(handle, &mut out as *mut MlsBuffer);
    if code == StatusCode::Ok as u32 {
        (code, Some(read_json(out)))
    } else {
        (code, None)
    }
}

#[test]
fn ffi_end_to_end_core_operations() {
    // SAFETY: defensive no-op branches for null/empty inputs.
    unsafe { messenger_mls_free(std::ptr::null_mut()) };
    // SAFETY: defensive no-op branch for empty buffer.
    unsafe { messenger_mls_buffer_free(MlsBuffer::default()) };
    // SAFETY: null-handle error path.
    assert_eq!(
        unsafe { messenger_mls_last_error(std::ptr::null_mut(), std::ptr::null_mut()) },
        StatusCode::InvalidArgument as u32
    );
    // SAFETY: null-handle error path.
    assert_eq!(
        unsafe { messenger_mls_export_client_state(std::ptr::null_mut(), std::ptr::null_mut()) },
        StatusCode::InvalidArgument as u32
    );

    let alice = messenger_mls_new();
    let bob = messenger_mls_new();
    assert!(!alice.is_null());
    assert!(!bob.is_null());

    // Invalid JSON path to verify parse errors and last_error behavior.
    let bad = b"{";
    let code_bad = messenger_mls_create_client(alice, bad.as_ptr(), bad.len());
    assert_eq!(code_bad, StatusCode::InvalidArgument as u32);
    assert_eq!(
        messenger_mls_create_client(alice, std::ptr::null(), 1),
        StatusCode::InvalidArgument as u32
    );

    let mut err_buf = MlsBuffer::default();
    let err_code = unsafe { messenger_mls_last_error(alice, &mut err_buf as *mut MlsBuffer) };
    assert_eq!(err_code, StatusCode::Ok as u32);
    let err_text = String::from_utf8(read_buf_as_vec(err_buf)).expect("utf8 last_error");
    assert!(
        err_text.contains("invalid json") || err_text.contains("input_ptr is null"),
        "unexpected last_error: {err_text}"
    );

    // Initialize both clients.
    let alice_input = serde_json::to_vec(&make_params("ffi-alice", "phone", 31)).unwrap();
    let bob_input = serde_json::to_vec(&make_params("ffi-bob", "phone", 33)).unwrap();
    assert_eq!(
        messenger_mls_create_client(alice, alice_input.as_ptr(), alice_input.len()),
        StatusCode::Ok as u32
    );
    assert_eq!(
        messenger_mls_create_client(bob, bob_input.as_ptr(), bob_input.len()),
        StatusCode::Ok as u32
    );

    let (code_id, id_opt): (u32, Option<ClientId>) =
        call_out_json(alice, |h, out| messenger_mls_get_client_id(h, out));
    assert_eq!(code_id, StatusCode::Ok as u32);
    let id = id_opt.expect("client id");
    assert_eq!(id.user_id, "ffi-alice");
    assert_eq!(
        messenger_mls_get_client_id(alice, std::ptr::null_mut()),
        StatusCode::InvalidArgument as u32
    );

    // Key packages for Bob.
    let (code_kp, bob_bundle_opt): (u32, Option<KeyPackageBundle>) =
        call_out_json(bob, |h, out| messenger_mls_create_key_packages(h, 1, out));
    assert_eq!(code_kp, StatusCode::Ok as u32);
    let bob_bundle = bob_bundle_opt.expect("bob key package bundle");
    assert_eq!(bob_bundle.keypackages.len(), 1);

    // Unknown package should fail on mark-uploaded.
    let unknown_bundle = KeyPackageBundle {
        keypackages: vec![vec![9, 9, 9]],
    };
    let unknown_json = serde_json::to_vec(&unknown_bundle).unwrap();
    assert_eq!(
        messenger_mls_mark_key_packages_uploaded(alice, unknown_json.as_ptr(), unknown_json.len()),
        StatusCode::NotFound as u32
    );

    // Mark known Bob package as uploaded in Bob state.
    let bob_bundle_json = serde_json::to_vec(&bob_bundle).unwrap();
    assert_eq!(
        messenger_mls_mark_key_packages_uploaded(
            bob,
            bob_bundle_json.as_ptr(),
            bob_bundle_json.len()
        ),
        StatusCode::Ok as u32
    );

    // Create and query group.
    let group_id = GroupId {
        value: b"ffi-group-1".to_vec(),
    };
    let group_json = serde_json::to_vec(&group_id).unwrap();

    let (code_create_group, group_state_opt): (u32, Option<GroupState>) =
        call_out_json(alice, |h, out| {
            messenger_mls_create_group(h, group_json.as_ptr(), group_json.len(), out)
        });
    assert_eq!(code_create_group, StatusCode::Ok as u32);
    let group_state = group_state_opt.expect("group state");
    assert_eq!(group_state.group_id.value, group_id.value);

    let (code_groups, groups_opt): (u32, Option<Vec<GroupState>>) =
        call_out_json(alice, |h, out| messenger_mls_list_groups(h, out));
    assert_eq!(code_groups, StatusCode::Ok as u32);
    assert_eq!(groups_opt.expect("groups").len(), 1);

    let (code_get_group, get_group_opt): (u32, Option<GroupState>) =
        call_out_json(alice, |h, out| {
            messenger_mls_get_group_state(h, group_json.as_ptr(), group_json.len(), out)
        });
    assert_eq!(code_get_group, StatusCode::Ok as u32);
    assert_eq!(
        get_group_opt.expect("get group").group_id.value,
        group_id.value
    );

    let (code_members, members_opt): (u32, Option<Vec<chat_core::Member>>) =
        call_out_json(alice, |h, out| {
            messenger_mls_list_members(h, group_json.as_ptr(), group_json.len(), out)
        });
    assert_eq!(code_members, StatusCode::Ok as u32);
    assert_eq!(members_opt.expect("members").len(), 1);

    // Invite Bob and join from welcome.
    let invite_req = InviteRequest {
        group_id: group_id.clone(),
        invited_client: ClientId {
            user_id: "ffi-bob".to_string(),
            device_id: "phone".to_string(),
        },
        keypackage: bob_bundle.keypackages[0].clone(),
    };
    let invite_json = serde_json::to_vec(&invite_req).unwrap();

    let (code_invite, invite_opt): (u32, Option<InviteResult>) = call_out_json(alice, |h, out| {
        messenger_mls_invite(h, invite_json.as_ptr(), invite_json.len(), out)
    });
    assert_eq!(code_invite, StatusCode::Ok as u32);
    let invite = invite_opt.expect("invite result");
    assert!(invite.has_welcome);

    let (code_join, bob_joined_opt): (u32, Option<GroupState>) = call_out_json(bob, |h, out| {
        messenger_mls_join_from_welcome(
            h,
            invite.welcome_message.as_ptr(),
            invite.welcome_message.len(),
            out,
        )
    });
    assert_eq!(code_join, StatusCode::Ok as u32);
    assert_eq!(
        bob_joined_opt.expect("bob joined").group_id.value,
        group_id.value
    );

    // Pending commit lifecycle via self-update.
    let (code_pending_before, pending_before_opt): (u32, Option<bool>) =
        call_out_json(alice, |h, out| {
            messenger_mls_has_pending_commit(h, group_json.as_ptr(), group_json.len(), out)
        });
    assert_eq!(code_pending_before, StatusCode::Ok as u32);
    assert!(pending_before_opt.expect("pending before clear from invite"));

    assert_eq!(
        messenger_mls_clear_pending_commit(alice, group_json.as_ptr(), group_json.len()),
        StatusCode::Ok as u32
    );

    let (code_pending_cleared, pending_cleared_opt): (u32, Option<bool>) =
        call_out_json(alice, |h, out| {
            messenger_mls_has_pending_commit(h, group_json.as_ptr(), group_json.len(), out)
        });
    assert_eq!(code_pending_cleared, StatusCode::Ok as u32);
    assert!(!pending_cleared_opt.expect("pending cleared"));

    let (code_update, update_opt): (u32, Option<SelfUpdateResult>) =
        call_out_json(alice, |h, out| {
            messenger_mls_self_update(h, group_json.as_ptr(), group_json.len(), out)
        });
    assert_eq!(code_update, StatusCode::Ok as u32);
    assert!(!update_opt.expect("self update").commit_message.is_empty());

    // Encrypt path coverage.
    let encrypt_req = serde_json::json!({
        "group_id": { "value": group_id.value },
        "plaintext": [1,2,3],
        "aad": [7,8],
    });
    let encrypt_json = serde_json::to_vec(&encrypt_req).unwrap();
    let mut encrypted_out = MlsBuffer::default();
    let code_encrypt = messenger_mls_encrypt_message(
        alice,
        encrypt_json.as_ptr(),
        encrypt_json.len(),
        &mut encrypted_out as *mut MlsBuffer,
    );
    assert_eq!(code_encrypt, StatusCode::Ok as u32);
    let encrypted: Vec<u8> = serde_json::from_slice(&read_buf_as_vec(encrypted_out)).unwrap();
    assert!(!encrypted.is_empty());

    assert_eq!(
        messenger_mls_encrypt_message(
            alice,
            encrypt_json.as_ptr(),
            encrypt_json.len(),
            std::ptr::null_mut(),
        ),
        StatusCode::InvalidArgument as u32
    );

    // Handle incoming invalid welcome payload to cover error branch.
    let incoming = serde_json::to_vec(&IncomingMessage {
        kind: IncomingMessageKind::Welcome,
        payload: vec![],
    })
    .unwrap();
    let mut events_out = MlsBuffer::default();
    let code_incoming = messenger_mls_handle_incoming(
        alice,
        incoming.as_ptr(),
        incoming.len(),
        &mut events_out as *mut MlsBuffer,
    );
    assert_eq!(code_incoming, StatusCode::InvalidArgument as u32);

    // Remove wrapper path.
    let remove_json = serde_json::to_vec(&RemoveRequest {
        group_id: group_id.clone(),
        removed_client: ClientId {
            user_id: "ffi-bob".to_string(),
            device_id: "phone".to_string(),
        },
    })
    .unwrap();
    let mut remove_out = MlsBuffer::default();
    let _ = messenger_mls_remove(
        alice,
        remove_json.as_ptr(),
        remove_json.len(),
        &mut remove_out as *mut MlsBuffer,
    );

    // Export/restore client state.
    let mut state_out = MlsBuffer::default();
    let code_export =
        unsafe { messenger_mls_export_client_state(alice, &mut state_out as *mut MlsBuffer) };
    assert_eq!(code_export, StatusCode::Ok as u32);
    let exported_state = read_buf_as_vec(state_out);

    let restored = messenger_mls_new();
    assert!(!restored.is_null());
    let code_restore =
        messenger_mls_restore_client(restored, exported_state.as_ptr(), exported_state.len());
    assert!(
        code_restore == StatusCode::Ok as u32 || code_restore == StatusCode::Unsupported as u32,
        "unexpected restore code: {code_restore}"
    );

    // Drop group + expected not found on subsequent state query.
    assert_eq!(
        messenger_mls_drop_group(alice, group_json.as_ptr(), group_json.len()),
        StatusCode::Ok as u32
    );
    let mut dropped_state_out = MlsBuffer::default();
    let code_after_drop = messenger_mls_get_group_state(
        alice,
        group_json.as_ptr(),
        group_json.len(),
        &mut dropped_state_out as *mut MlsBuffer,
    );
    assert_eq!(code_after_drop, StatusCode::NotFound as u32);

    // Null handle should be invalid argument.
    assert_eq!(
        messenger_mls_create_client(
            std::ptr::null_mut(),
            alice_input.as_ptr(),
            alice_input.len()
        ),
        StatusCode::InvalidArgument as u32
    );

    // SAFETY: handles were allocated by messenger_mls_new and are freed once each.
    unsafe {
        messenger_mls_free(alice);
        messenger_mls_free(bob);
        messenger_mls_free(restored);
    }
}
