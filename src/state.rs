//! Модели runtime- и persisted-состояния для сервисного слоя.

use crate::types::{
    Bytes, ClientId, CreateClientParams, GroupId, GroupState, IncomingMessage, KeyPackageBundle,
    Member,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Persisted-запись идентичности, восстанавливаемая из [`CreateClientParams`].
pub struct ClientIdentityState {
    /// Идентичность локального клиента.
    pub client_id: ClientId,
    /// Сериализованный credential/binding payload из слоя приложения.
    pub credential: Bytes,
    /// Сырой приватный ключ подписи устройства для повторной настройки backend.
    pub signature_private_key: Bytes,
    /// Payload подписи аккаунта/привязки устройства.
    pub device_binding: Bytes,
    /// Байты идентичности, переданные в MLS credential.
    pub identity_data: Bytes,
}

impl From<&CreateClientParams> for ClientIdentityState {
    fn from(value: &CreateClientParams) -> Self {
        Self {
            client_id: value.client_id.clone(),
            credential: value.binding.serialized_binding.clone(),
            signature_private_key: value.device_signature_private_key.clone(),
            device_binding: value.binding.account_signature.clone(),
            identity_data: value.identity_data.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Локальная запись инвентаря для сгенерированного key package.
pub struct KeyPackageRecord {
    /// Сериализованный key package.
    pub data: Bytes,
    /// Сообщило ли приложение, что пакет загружен.
    pub uploaded: bool,
    /// Был ли пакет использован в процессе приглашения.
    pub consumed: bool,
    /// Был ли пакет отозван.
    pub revoked: bool,
    /// Истёк ли пакет по политике приложения.
    pub expired: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Runtime-состояние группы, которым владеет сервис.
pub struct GroupRuntimeState {
    /// Публичный снимок группы.
    pub group_state: GroupState,
    /// Локальная проекция списка участников.
    pub members: Vec<Member>,
    /// MLS leaf-индекс локального клиента, если известен.
    pub self_leaf_index: Option<u32>,
    /// Отображение leaf-индекса в логический идентификатор участника.
    pub member_map: HashMap<u32, ClientId>,
    /// Кэш байтов ratchet tree для интеграций.
    pub ratchet_tree_cache: Bytes,
    /// `true`, если локальное состояние ожидает подтверждение/merge commit.
    pub pending_commit: bool,
    /// Опциональное staged-сериализованное состояние для будущих workflow.
    pub staged_state: Option<Bytes>,
    /// Экспортированный общий секретный материал для локальных возможностей.
    pub secret_material: Bytes,
    /// Монотонный счётчик обработанных транспортных элементов.
    pub transport_last_offset: u64,
    /// Локально удерживаемая очередь входящих сообщений.
    pub incoming_queue: VecDeque<IncomingMessage>,
}

impl GroupRuntimeState {
    /// Создаёт новое runtime-состояние, где `self_member` находится в leaf `0`.
    pub fn new(group_id: GroupId, self_member: Member) -> Self {
        let mut member_map = HashMap::new();
        member_map.insert(0, self_member.client_id.clone());

        Self {
            group_state: GroupState {
                group_id,
                epoch: 0,
                active: true,
                serialized_state: Vec::new(),
            },
            members: vec![self_member],
            self_leaf_index: Some(0),
            member_map,
            ratchet_tree_cache: Vec::new(),
            pending_commit: false,
            staged_state: None,
            secret_material: Vec::new(),
            transport_last_offset: 0,
            incoming_queue: VecDeque::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Сериализуемый снимок клиента для API экспорта/восстановления.
pub struct PersistedClientState {
    /// Опциональная конфигурация идентичности клиента.
    pub identity: Option<ClientIdentityState>,
    /// Persisted-группы.
    pub groups: Vec<GroupRuntimeState>,
    /// Persisted-инвентарь key package.
    pub key_packages: Vec<KeyPackageRecord>,
    /// Монотонный локальный счётчик key package.
    pub key_package_counter: u64,
}

impl PersistedClientState {
    /// Создаёт пустое persisted-состояние.
    pub fn empty() -> Self {
        Self {
            identity: None,
            groups: Vec::new(),
            key_packages: Vec::new(),
            key_package_counter: 0,
        }
    }

    /// Преобразует runtime-состояние в памяти в persisted-представление.
    pub fn from_runtime(runtime: &RuntimeState) -> Self {
        Self {
            identity: runtime.identity.clone(),
            groups: runtime.groups.values().cloned().collect(),
            key_packages: runtime.key_packages.clone(),
            key_package_counter: runtime.key_package_counter,
        }
    }
}

#[derive(Debug, Clone)]
/// Runtime-состояние сервиса в памяти.
pub struct RuntimeState {
    /// Инициализированная идентичность, если есть.
    pub identity: Option<ClientIdentityState>,
    /// Карта групп с ключом в виде hex-кодированного group id.
    pub groups: HashMap<String, GroupRuntimeState>,
    /// Локальный инвентарь key package.
    pub key_packages: Vec<KeyPackageRecord>,
    /// Монотонный локальный счётчик key package.
    pub key_package_counter: u64,
}

impl RuntimeState {
    /// Создаёт пустое runtime-состояние.
    pub fn new() -> Self {
        Self {
            identity: None,
            groups: HashMap::new(),
            key_packages: Vec::new(),
            key_package_counter: 0,
        }
    }

    /// Восстанавливает runtime-состояние из persisted-снимка.
    pub fn restore(persisted: PersistedClientState) -> Self {
        let mut groups = HashMap::new();
        for group in persisted.groups {
            let key = to_group_key(&group.group_state.group_id.value);
            groups.insert(key, group);
        }

        Self {
            identity: persisted.identity,
            groups,
            key_packages: persisted.key_packages,
            key_package_counter: persisted.key_package_counter,
        }
    }
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self::new()
    }
}

/// Преобразует бинарный group id в стабильный hex-ключ в нижнем регистре.
pub fn to_group_key(group_id: &[u8]) -> String {
    let mut out = String::with_capacity(group_id.len() * 2);
    for b in group_id {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

/// Преобразует записи key package в публичный DTO `KeyPackageBundle`.
pub fn bundle_from_records(records: &[KeyPackageRecord]) -> KeyPackageBundle {
    KeyPackageBundle {
        keypackages: records.iter().map(|r| r.data.clone()).collect(),
    }
}
