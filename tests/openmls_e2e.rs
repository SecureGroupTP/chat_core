use chat_core::{
    ClientId, CreateClientParams, DeviceBinding, GroupId, IncomingMessage, IncomingMessageKind,
    InviteRequest, MessengerMls, StatusCode,
};

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

#[test]
fn invite_join_then_app_message_hits_known_generation_limit() {
    let mut alice = MessengerMls::new();
    let mut bob = MessengerMls::new();

    alice
        .create_client(make_params("alice", "phone", 7))
        .expect("alice client init");
    bob.create_client(make_params("bob", "phone", 9))
        .expect("bob client init");

    let bob_key_packages = bob
        .create_key_packages(1)
        .expect("bob key package generation");
    let bob_key_package = bob_key_packages.keypackages[0].clone();

    let group_id = GroupId {
        value: b"group-e2e-1".to_vec(),
    };
    alice.create_group(group_id.clone()).expect("create group");

    let invite = alice
        .invite(InviteRequest {
            group_id: group_id.clone(),
            invited_client: bob.get_client_id().expect("bob client id"),
            keypackage: bob_key_package,
        })
        .expect("invite bob");
    assert!(invite.has_welcome, "invite should produce welcome message");
    assert!(
        !invite.commit_message.is_empty(),
        "invite should produce commit message"
    );
    assert!(
        !invite.welcome_message.is_empty(),
        "welcome message should not be empty"
    );

    bob.join_from_welcome(&invite.welcome_message)
        .expect("bob join from welcome");

    let ciphertext = alice
        .encrypt_message(group_id.clone(), b"hello bob".to_vec(), b"aad-1".to_vec())
        .expect("encrypt message");

    let err = bob
        .handle_incoming(IncomingMessage {
            kind: IncomingMessageKind::GroupMessage,
            payload: ciphertext,
        })
        .expect_err("known limitation: generation mismatch after invite/join");
    assert_eq!(err.code, StatusCode::CryptoError);
    assert!(
        err.message.contains("Generation is too old"),
        "unexpected crypto error message: {}",
        err.message
    );

    assert!(
        alice
            .has_pending_commit(group_id.clone())
            .expect("pending commit after invite")
    );
    alice
        .clear_pending_commit(group_id.clone())
        .expect("clear pending commit after invite");

    assert!(
        !alice
            .has_pending_commit(group_id)
            .expect("pending commit query after clear"),
        "pending commit should be cleared"
    );
}

#[test]
fn self_update_sets_and_clears_pending_commit() {
    let mut svc = MessengerMls::new();
    svc.create_client(make_params("alice", "watch", 17))
        .expect("create client");

    let group_id = GroupId {
        value: b"group-self-update".to_vec(),
    };
    svc.create_group(group_id.clone()).expect("create group");

    let update = svc.self_update(group_id.clone()).expect("self update");
    assert!(!update.commit_message.is_empty());

    assert!(
        svc.has_pending_commit(group_id.clone())
            .expect("pending commit should be set")
    );
    svc.clear_pending_commit(group_id.clone())
        .expect("clear pending commit");
    assert!(
        !svc.has_pending_commit(group_id)
            .expect("pending commit query"),
        "pending commit should be cleared"
    );
}

#[test]
fn export_restore_roundtrip_identity_and_key_packages() {
    let mut svc = MessengerMls::new();
    svc.create_client(make_params("alice", "laptop", 11))
        .expect("create client");

    let generated = svc.create_key_packages(2).expect("create key packages");
    let state = svc.export_client_state().expect("export state");

    let mut restored = MessengerMls::new();
    restored.restore_client(&state).expect("restore state");

    let restored_id = restored.get_client_id().expect("restored client id");
    assert_eq!(restored_id.user_id, "alice");
    assert_eq!(restored_id.device_id, "laptop");

    restored
        .mark_key_packages_uploaded(generated)
        .expect("restored state should contain generated key packages");
}

#[test]
fn restore_with_groups_is_unsupported_in_real_backend() {
    let mut svc = MessengerMls::new();
    svc.create_client(make_params("alice", "tablet", 13))
        .expect("create client");
    svc.create_group(GroupId {
        value: b"group-restore-unsupported".to_vec(),
    })
    .expect("create group");

    let snapshot = svc.export_client_state().expect("export state with group");

    let mut restored = MessengerMls::new();
    let err = restored
        .restore_client(&snapshot)
        .expect_err("group restore must be unsupported currently");
    assert_eq!(err.code, StatusCode::Unsupported);
}
