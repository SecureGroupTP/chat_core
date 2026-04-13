use chat_core::backend::{CommitArtifacts, GroupSnapshot, MlsBackend};
use chat_core::{
    Bytes, ClientId, CreateClientParams, DeviceBinding, EventKind, GroupId, IncomingMessage,
    IncomingMessageKind, InviteRequest, MessengerMls, MlsResult, RemoveRequest, StatusCode,
};

#[derive(Default)]
struct ScriptBackend {
    pending: bool,
}

impl ScriptBackend {
    fn snapshot(_group_id: &GroupId) -> GroupSnapshot {
        GroupSnapshot {
            epoch: 2,
            active: true,
            serialized_state: vec![1, 2, 3],
            secret_material: vec![4, 5, 6],
            self_leaf_index: Some(0),
        }
    }
}

impl MlsBackend for ScriptBackend {
    fn reset(&mut self) {}

    fn configure_client(&mut self, _params: &CreateClientParams) -> MlsResult<()> {
        Ok(())
    }

    fn create_key_packages(&mut self, count: u32) -> MlsResult<Vec<Bytes>> {
        Ok((0..count).map(|i| vec![i as u8]).collect())
    }

    fn create_group(&mut self, group_id: &GroupId) -> MlsResult<GroupSnapshot> {
        Ok(Self::snapshot(group_id))
    }

    fn invite(&mut self, group_id: &GroupId, _keypackage: &[u8]) -> MlsResult<CommitArtifacts> {
        self.pending = true;
        Ok(CommitArtifacts {
            commit_message: vec![9],
            welcome_message: Some(vec![8]),
            snapshot: Self::snapshot(group_id),
        })
    }

    fn join_from_welcome(
        &mut self,
        _welcome_message: &[u8],
    ) -> MlsResult<(GroupId, GroupSnapshot)> {
        let gid = GroupId {
            value: b"script-welcome-group".to_vec(),
        };
        Ok((gid.clone(), Self::snapshot(&gid)))
    }

    fn remove(
        &mut self,
        group_id: &GroupId,
        _removed_leaf_index: Option<u32>,
    ) -> MlsResult<CommitArtifacts> {
        self.pending = true;
        Ok(CommitArtifacts {
            commit_message: vec![7],
            welcome_message: None,
            snapshot: Self::snapshot(group_id),
        })
    }

    fn self_update(&mut self, group_id: &GroupId) -> MlsResult<CommitArtifacts> {
        self.pending = true;
        Ok(CommitArtifacts {
            commit_message: vec![6],
            welcome_message: None,
            snapshot: Self::snapshot(group_id),
        })
    }

    fn encrypt(&mut self, _group_id: &GroupId, plaintext: &[u8], _aad: &[u8]) -> MlsResult<Bytes> {
        Ok(plaintext.to_vec())
    }

    fn handle_incoming(&mut self, _message: &IncomingMessage) -> MlsResult<Option<Bytes>> {
        Ok(Some(b"from-backend".to_vec()))
    }

    fn has_pending_commit(&self, _group_id: &GroupId) -> MlsResult<bool> {
        Ok(self.pending)
    }

    fn merge_pending_commit(&mut self, group_id: &GroupId) -> MlsResult<GroupSnapshot> {
        self.pending = false;
        Ok(Self::snapshot(group_id))
    }

    fn clear_pending_commit(&mut self, _group_id: &GroupId) -> MlsResult<()> {
        self.pending = false;
        Ok(())
    }

    fn drop_group(&mut self, _group_id: &GroupId) -> MlsResult<()> {
        Ok(())
    }
}

fn make_params() -> CreateClientParams {
    CreateClientParams {
        client_id: ClientId {
            user_id: "svc-user".to_string(),
            device_id: "svc-device".to_string(),
        },
        device_signature_private_key: vec![3; 32],
        binding: DeviceBinding {
            client_id: ClientId::default(),
            serialized_binding: vec![],
            account_signature: vec![],
        },
        identity_data: vec![],
    }
}

#[test]
fn service_paths_with_script_backend() {
    let mut svc = MessengerMls::with_backend(Box::new(ScriptBackend::default()));
    svc.create_client(make_params()).expect("create client");

    let gid = GroupId {
        value: b"script-group".to_vec(),
    };
    let _ = svc.create_group(gid.clone()).expect("create group");
    let persisted_after_create: chat_core::state::PersistedClientState =
        serde_json::from_slice(&svc.export_client_state().expect("export"))
            .expect("decode persisted");
    let created_group = persisted_after_create
        .groups
        .iter()
        .find(|g| g.group_state.group_id.value == gid.value)
        .expect("created group exists");
    assert_eq!(created_group.ratchet_tree_cache, vec![1, 2, 3]);

    // Duplicate group path.
    assert_eq!(
        svc.create_group(gid.clone())
            .expect_err("duplicate group")
            .code,
        StatusCode::AlreadyExists
    );

    let invite = svc
        .invite(InviteRequest {
            group_id: gid.clone(),
            invited_client: ClientId {
                user_id: "other".to_string(),
                device_id: "d1".to_string(),
            },
            keypackage: vec![1],
        })
        .expect("invite");
    assert!(invite.has_welcome);
    assert!(svc.has_pending_commit(gid.clone()).expect("pending true"));

    let remove = svc
        .remove(RemoveRequest {
            group_id: gid.clone(),
            removed_client: ClientId {
                user_id: "other".to_string(),
                device_id: "d1".to_string(),
            },
        })
        .expect("remove existing invited member");
    assert!(!remove.commit_message.is_empty());

    assert_eq!(
        svc.remove(RemoveRequest {
            group_id: gid.clone(),
            removed_client: ClientId {
                user_id: "other".to_string(),
                device_id: "d1".to_string(),
            },
        })
        .expect_err("double remove should fail before backend")
        .code,
        StatusCode::NotFound
    );

    let self_remove = svc
        .remove(RemoveRequest {
            group_id: gid.clone(),
            removed_client: ClientId {
                user_id: "svc-user".to_string(),
                device_id: "svc-device".to_string(),
            },
        })
        .expect("self remove path");
    assert!(!self_remove.group_state.active);
    let persisted_after_self_remove: chat_core::state::PersistedClientState =
        serde_json::from_slice(&svc.export_client_state().expect("export"))
            .expect("decode persisted");
    let self_removed_group = persisted_after_self_remove
        .groups
        .iter()
        .find(|g| g.group_state.group_id.value == gid.value)
        .expect("group exists");
    assert_eq!(self_removed_group.self_leaf_index, None);

    svc.clear_pending_commit(gid.clone())
        .expect("clear pending from backend");

    // handle_incoming Welcome branch.
    let welcome_events = svc
        .handle_incoming(IncomingMessage {
            kind: IncomingMessageKind::Welcome,
            payload: vec![1],
        })
        .expect("welcome handled");
    assert_eq!(welcome_events.len(), 1);
    assert_eq!(welcome_events[0].kind, EventKind::GroupJoined);

    // Build valid MLS payload from real service to drive incoming_group_id + queue path.
    let mut real = MessengerMls::new();
    real.create_client(CreateClientParams {
        client_id: ClientId {
            user_id: "real".to_string(),
            device_id: "d".to_string(),
        },
        device_signature_private_key: vec![7; 32],
        binding: DeviceBinding {
            client_id: ClientId {
                user_id: "real".to_string(),
                device_id: "d".to_string(),
            },
            serialized_binding: vec![],
            account_signature: vec![],
        },
        identity_data: vec![],
    })
    .expect("real create");
    real.create_group(gid.clone()).expect("real group");
    let ciphertext = real
        .encrypt_message(gid.clone(), b"x".to_vec(), b"a".to_vec())
        .expect("real encrypt");

    let msg_events = svc
        .handle_incoming(IncomingMessage {
            kind: IncomingMessageKind::GroupMessage,
            payload: ciphertext,
        })
        .expect("group message handled");
    assert_eq!(msg_events.len(), 1);
    assert_eq!(msg_events[0].kind, EventKind::MessageReceived);
    assert_eq!(msg_events[0].message_plaintext, b"from-backend");

    // Repeat invite/remove to ensure member-map indexes don't collide after holes.
    let invite_again = svc
        .invite(InviteRequest {
            group_id: gid.clone(),
            invited_client: ClientId {
                user_id: "other2".to_string(),
                device_id: "d2".to_string(),
            },
            keypackage: vec![2],
        })
        .expect("invite after remove");
    assert!(!invite_again.commit_message.is_empty());

    // drop_group local-not-found path after backend succeeds.
    let missing_gid = GroupId {
        value: b"not-in-local-map".to_vec(),
    };
    assert_eq!(
        svc.drop_group(missing_gid)
            .expect_err("local missing after backend ok")
            .code,
        StatusCode::NotFound
    );
}
