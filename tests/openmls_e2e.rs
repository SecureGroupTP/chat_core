use chat_core::{
    ClientId, CreateClientParams, DeviceBinding, EventKind, GroupId, IncomingMessage,
    IncomingMessageKind, InviteRequest, MessengerMls,
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
fn invite_join_then_app_message_delivers_after_inviter_merges_pending_commit() {
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

    let merged_state = alice
        .merge_pending_commit(group_id.clone())
        .expect("alice merges pending invite commit");
    assert_eq!(merged_state.epoch, 1);
    assert!(
        !alice
            .has_pending_commit(group_id.clone())
            .expect("pending commit after merge"),
        "pending commit should be cleared after merge"
    );

    let ciphertext = alice
        .encrypt_message(group_id.clone(), b"hello bob".to_vec(), b"aad-1".to_vec())
        .expect("encrypt message");

    let events = bob
        .handle_incoming(IncomingMessage {
            kind: IncomingMessageKind::GroupMessage,
            payload: ciphertext,
        })
        .expect("application message should decrypt after merge");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, EventKind::MessageReceived);
    assert_eq!(events[0].group_id.value, group_id.value);
    assert_eq!(events[0].message_plaintext, b"hello bob".to_vec());
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
    let merged = svc
        .merge_pending_commit(group_id.clone())
        .expect("merge pending commit");
    assert_eq!(merged.epoch, 1);
    assert!(
        !svc.has_pending_commit(group_id)
            .expect("pending commit query"),
        "pending commit should be cleared after merge"
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
fn restore_with_groups_roundtrips_real_backend_state_and_flow() {
    let mut alice = MessengerMls::new();
    let mut bob = MessengerMls::new();

    alice
        .create_client(make_params("alice", "tablet", 13))
        .expect("alice create client");
    bob.create_client(make_params("bob", "tablet", 15))
        .expect("bob create client");

    let mut bob_key_packages = bob.create_key_packages(1).expect("bob key packages");
    let bob_key_package = bob_key_packages.keypackages.remove(0);

    let group_id = GroupId {
        value: b"group-restore-real".to_vec(),
    };
    alice
        .create_group(group_id.clone())
        .expect("alice create group");

    let invite = alice
        .invite(InviteRequest {
            group_id: group_id.clone(),
            invited_client: bob.get_client_id().expect("bob client id"),
            keypackage: bob_key_package,
        })
        .expect("invite bob");
    bob.join_from_welcome(&invite.welcome_message)
        .expect("bob join welcome");

    let alice_state = alice
        .merge_pending_commit(group_id.clone())
        .expect("alice merge pending");
    let bob_state_before_export = bob
        .get_group_state(group_id.clone())
        .expect("bob knows group before export");
    assert_eq!(bob_state_before_export.epoch, alice_state.epoch);

    let snapshot = bob.export_client_state().expect("export state with group");

    let mut restored_bob = MessengerMls::new();
    restored_bob
        .restore_client(&snapshot)
        .expect("restore group state");

    let restored_groups = restored_bob.list_groups().expect("restored groups");
    assert_eq!(restored_groups.len(), 1);
    assert_eq!(restored_groups[0].group_id.value, group_id.value.clone());

    let restored_state = restored_bob
        .get_group_state(group_id.clone())
        .expect("restored group state");
    assert_eq!(restored_state.epoch, bob_state_before_export.epoch);

    let ciphertext_from_alice = alice
        .encrypt_message(
            group_id.clone(),
            b"message after restore".to_vec(),
            b"aad-restore-1".to_vec(),
        )
        .expect("alice encrypt after restore");
    let restored_events = restored_bob
        .handle_incoming(IncomingMessage {
            kind: IncomingMessageKind::GroupMessage,
            payload: ciphertext_from_alice,
        })
        .expect("restored bob decrypts alice message");
    assert_eq!(restored_events.len(), 1);
    assert_eq!(restored_events[0].kind, EventKind::MessageReceived);
    assert_eq!(
        restored_events[0].message_plaintext,
        b"message after restore".to_vec()
    );

    let ciphertext_from_restored_bob = restored_bob
        .encrypt_message(
            group_id.clone(),
            b"reply from restored bob".to_vec(),
            b"aad-restore-2".to_vec(),
        )
        .expect("restored bob encrypt");
    let alice_events = alice
        .handle_incoming(IncomingMessage {
            kind: IncomingMessageKind::GroupMessage,
            payload: ciphertext_from_restored_bob,
        })
        .expect("alice decrypts restored bob message");
    assert_eq!(alice_events.len(), 1);
    assert_eq!(alice_events[0].kind, EventKind::MessageReceived);
    assert_eq!(
        alice_events[0].message_plaintext,
        b"reply from restored bob".to_vec()
    );
}
