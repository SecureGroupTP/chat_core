use chat_core::state::{
    GroupRuntimeState, PersistedClientState, RuntimeState, bundle_from_records, to_group_key,
};
use chat_core::{ClientId, GroupId, KeyPackageBundle, Member};

#[test]
fn state_helpers_and_restore_paths() {
    let mut runtime = RuntimeState::new();
    let runtime_default = RuntimeState::default();
    assert!(runtime_default.groups.is_empty());
    assert!(runtime.groups.is_empty());

    let group = GroupRuntimeState::new(
        GroupId {
            value: vec![0x01, 0xAB],
        },
        Member {
            client_id: ClientId {
                user_id: "alice".to_string(),
                device_id: "phone".to_string(),
            },
            is_self: true,
        },
    );

    let key = to_group_key(&group.group_state.group_id.value);
    assert_eq!(key, "01ab");

    runtime.groups.insert(key.clone(), group.clone());
    runtime
        .key_packages
        .push(chat_core::state::KeyPackageRecord {
            data: vec![1, 2, 3],
            uploaded: false,
            consumed: false,
            revoked: false,
            expired: false,
        });

    let persisted = PersistedClientState::from_runtime(&runtime);
    assert_eq!(persisted.groups.len(), 1);

    let restored = RuntimeState::restore(persisted.clone());
    assert_eq!(restored.groups.len(), 1);
    assert!(restored.groups.contains_key(&key));

    let bundle: KeyPackageBundle = bundle_from_records(&persisted.key_packages);
    assert_eq!(bundle.keypackages, vec![vec![1, 2, 3]]);

    let empty = PersistedClientState::empty();
    assert!(empty.identity.is_none());
    assert!(empty.groups.is_empty());
    assert!(empty.key_packages.is_empty());

    // Duplicate group ids in persisted input should not overwrite first entry.
    let mut g1 = group.clone();
    g1.group_state.epoch = 11;
    let mut g2 = group.clone();
    g2.group_state.epoch = 99;
    let restored_dup = RuntimeState::restore(PersistedClientState {
        identity: None,
        backend_snapshot: None,
        groups: vec![g1, g2],
        key_packages: Vec::new(),
        key_package_counter: 0,
    });
    let only = restored_dup.groups.get(&key).expect("group exists");
    assert_eq!(only.group_state.epoch, 11);
}
