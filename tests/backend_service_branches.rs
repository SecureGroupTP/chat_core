use chat_core::backend::{MlsBackend, OpenMlsBackend};
use chat_core::{
    ClientId, CreateClientParams, DeviceBinding, GroupId, IncomingMessage, IncomingMessageKind,
    InviteRequest, MessengerMls, RemoveRequest, StatusCode,
};
use ed25519_dalek::SigningKey;

fn params_with_key(user: &str, device: &str, key: Vec<u8>) -> CreateClientParams {
    CreateClientParams {
        client_id: ClientId {
            user_id: user.to_string(),
            device_id: device.to_string(),
        },
        device_signature_private_key: key,
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

fn params32(user: &str, device: &str, seed: u8) -> CreateClientParams {
    params_with_key(user, device, vec![seed; 32])
}

#[test]
fn service_error_branches_are_reported() {
    let mut svc = MessengerMls::new();
    let _ = MessengerMls::default();

    assert_eq!(
        svc.get_client_id().expect_err("id before init").code,
        StatusCode::InvalidState
    );
    assert_eq!(
        svc.create_key_packages(1)
            .expect_err("key packages before init")
            .code,
        StatusCode::InvalidState
    );

    assert_eq!(
        svc.create_client(params_with_key("", "d", vec![1; 32]))
            .expect_err("empty user")
            .code,
        StatusCode::InvalidArgument
    );
    assert_eq!(
        svc.create_client(params_with_key("u", "", vec![1; 32]))
            .expect_err("empty device")
            .code,
        StatusCode::InvalidArgument
    );
    assert_eq!(
        svc.create_client(params_with_key("u", "d", vec![]))
            .expect_err("empty key")
            .code,
        StatusCode::InvalidArgument
    );

    let mut mismatched = params32("alice", "phone", 1);
    mismatched.binding.client_id = ClientId {
        user_id: "other".to_string(),
        device_id: "phone".to_string(),
    };
    assert_eq!(
        svc.create_client(mismatched)
            .expect_err("binding mismatch")
            .code,
        StatusCode::InvalidArgument
    );

    // binding.client_id empty should be accepted.
    let mut empty_binding_id = params32("alice", "phone", 2);
    empty_binding_id.binding.client_id = ClientId::default();
    svc.create_client(empty_binding_id)
        .expect("empty binding id accepted");

    // restore with empty state (without identity) should succeed.
    let empty_persisted =
        serde_json::to_vec(&chat_core::state::PersistedClientState::empty()).expect("encode");
    svc.restore_client(&empty_persisted)
        .expect("restore empty persisted state");

    svc.create_client(params32("alice", "phone", 3))
        .expect("create client");

    assert_eq!(
        svc.create_group(GroupId { value: vec![] })
            .expect_err("empty group")
            .code,
        StatusCode::InvalidArgument
    );

    let unknown_group = GroupId {
        value: b"missing-group".to_vec(),
    };
    assert_eq!(
        svc.get_group_state(unknown_group.clone())
            .expect_err("missing state")
            .code,
        StatusCode::NotFound
    );
    assert_eq!(
        svc.list_members(unknown_group.clone())
            .expect_err("missing members")
            .code,
        StatusCode::NotFound
    );
    assert_eq!(
        svc.has_pending_commit(unknown_group.clone())
            .expect_err("missing pending")
            .code,
        StatusCode::NotFound
    );
    assert_eq!(
        svc.clear_pending_commit(unknown_group.clone())
            .expect_err("missing clear")
            .code,
        StatusCode::NotFound
    );
    assert_eq!(
        svc.drop_group(unknown_group.clone())
            .expect_err("missing drop")
            .code,
        StatusCode::NotFound
    );

    assert_eq!(
        svc.join_from_welcome(&[]).expect_err("empty welcome").code,
        StatusCode::InvalidArgument
    );

    assert_eq!(
        svc.handle_incoming(IncomingMessage {
            kind: IncomingMessageKind::GroupMessage,
            payload: vec![1, 2, 3],
        })
        .expect_err("invalid incoming payload")
        .code,
        StatusCode::InvalidArgument
    );

    let group_id = GroupId {
        value: b"service-remove-group".to_vec(),
    };
    svc.create_group(group_id.clone()).expect("create group");
    assert_eq!(
        svc.remove(RemoveRequest {
            group_id,
            removed_client: ClientId {
                user_id: "nobody".to_string(),
                device_id: "x".to_string(),
            },
        })
        .expect_err("remove unknown member")
        .code,
        StatusCode::NotFound
    );

    assert_eq!(
        svc.invite(InviteRequest {
            group_id: GroupId {
                value: b"missing-group-2".to_vec(),
            },
            invited_client: ClientId {
                user_id: "bob".to_string(),
                device_id: "phone".to_string(),
            },
            keypackage: vec![1, 2],
        })
        .expect_err("invite missing group")
        .code,
        StatusCode::NotFound
    );
}

#[test]
fn backend_direct_error_and_edge_branches() {
    let mut backend = OpenMlsBackend::default();

    assert_eq!(
        backend
            .create_key_packages(1)
            .expect_err("no signer configured")
            .code,
        StatusCode::InvalidState
    );

    assert_eq!(
        backend
            .configure_client(&params_with_key("a", "d", vec![1; 10]))
            .expect_err("invalid key len")
            .code,
        StatusCode::InvalidArgument
    );

    let mut keypair = [0u8; 64];
    keypair[..32].copy_from_slice(&[42u8; 32]);
    let signing = SigningKey::from_bytes((&keypair[..32]).try_into().expect("seed slice"));
    keypair.copy_from_slice(&signing.to_keypair_bytes());

    backend
        .configure_client(&params_with_key("a", "d", keypair.to_vec()))
        .expect("configure with 64-byte keypair");

    assert_eq!(
        backend.create_key_packages(0).expect_err("zero count").code,
        StatusCode::InvalidArgument
    );
    assert_eq!(
        backend
            .create_key_packages(4097)
            .expect_err("too large count")
            .code,
        StatusCode::InvalidArgument
    );

    assert_eq!(
        backend
            .create_group(&GroupId { value: vec![] })
            .expect_err("empty group")
            .code,
        StatusCode::InvalidArgument
    );

    let group_id = GroupId {
        value: b"backend-group".to_vec(),
    };
    backend.create_group(&group_id).expect("create group");
    assert_eq!(
        backend
            .create_group(&group_id)
            .expect_err("duplicate group")
            .code,
        StatusCode::AlreadyExists
    );

    assert_eq!(
        backend
            .invite(
                &GroupId {
                    value: b"missing".to_vec(),
                },
                &[1, 2, 3],
            )
            .expect_err("invite missing group")
            .code,
        StatusCode::CryptoError
    );

    assert_eq!(
        backend
            .join_from_welcome(&[1, 2, 3])
            .expect_err("invalid welcome")
            .code,
        StatusCode::CryptoError
    );
    assert_eq!(
        backend
            .join_from_welcome(&[])
            .expect_err("empty welcome")
            .code,
        StatusCode::InvalidArgument
    );

    assert_eq!(
        backend
            .remove(&group_id, None)
            .expect_err("remove none leaf")
            .code,
        StatusCode::NotFound
    );

    assert_eq!(
        backend
            .self_update(&GroupId {
                value: b"missing".to_vec(),
            })
            .expect_err("self update missing")
            .code,
        StatusCode::NotFound
    );

    assert_eq!(
        backend
            .encrypt(
                &GroupId {
                    value: b"missing".to_vec(),
                },
                b"abc",
                b"aad",
            )
            .expect_err("encrypt missing group")
            .code,
        StatusCode::NotFound
    );

    assert_eq!(
        backend
            .handle_incoming(&IncomingMessage {
                kind: IncomingMessageKind::Welcome,
                payload: vec![9, 9, 9],
            })
            .expect_err("invalid welcome incoming")
            .code,
        StatusCode::CryptoError
    );
    assert_eq!(
        backend
            .handle_incoming(&IncomingMessage {
                kind: IncomingMessageKind::GroupMessage,
                payload: vec![],
            })
            .expect_err("empty incoming")
            .code,
        StatusCode::InvalidArgument
    );
    assert_eq!(
        backend
            .encrypt(&GroupId { value: vec![] }, b"abc", b"aad")
            .expect_err("empty group id")
            .code,
        StatusCode::InvalidArgument
    );

    assert_eq!(
        backend
            .has_pending_commit(&GroupId {
                value: b"missing".to_vec(),
            })
            .expect_err("pending missing")
            .code,
        StatusCode::NotFound
    );

    assert_eq!(
        backend
            .clear_pending_commit(&GroupId {
                value: b"missing".to_vec(),
            })
            .expect_err("clear missing")
            .code,
        StatusCode::NotFound
    );

    // Reconfiguration should clear runtime groups to prevent stale state reuse.
    backend
        .create_group(&GroupId {
            value: b"old-group".to_vec(),
        })
        .expect("create old group");
    backend
        .configure_client(&params32("b", "d", 77))
        .expect("reconfigure");
    assert_eq!(
        backend
            .has_pending_commit(&GroupId {
                value: b"old-group".to_vec(),
            })
            .expect_err("group must be cleared")
            .code,
        StatusCode::NotFound
    );

    assert_eq!(
        backend
            .drop_group(&GroupId {
                value: b"missing".to_vec(),
            })
            .expect_err("drop missing")
            .code,
        StatusCode::NotFound
    );

    backend
        .create_group(&group_id)
        .expect("recreate group after reconfigure");
    backend.drop_group(&group_id).expect("drop existing group");
}

#[test]
fn service_self_message_roundtrip() {
    let mut svc = MessengerMls::new();
    svc.create_client(params32("self", "phone", 41))
        .expect("create client");
    let group_id = GroupId {
        value: b"self-msg-group".to_vec(),
    };
    svc.create_group(group_id.clone()).expect("create group");

    let ciphertext = svc
        .encrypt_message(group_id.clone(), b"ping".to_vec(), b"aad".to_vec())
        .expect("encrypt");
    let err = svc
        .handle_incoming(IncomingMessage {
            kind: IncomingMessageKind::GroupMessage,
            payload: ciphertext,
        })
        .expect_err("own message decrypt should fail in current OpenMLS setup");
    assert_eq!(err.code, StatusCode::CryptoError);
}
