//! Высокоуровневый сервисный API MLS-клиента.
//!
//! [`MessengerMls`] координирует валидацию, учёт runtime-состояния и
//! делегирование операций MLS-backend.

use crate::backend::{GroupSnapshot, MlsBackend, OpenMlsBackend};
use crate::state::{
    ClientIdentityState, GroupRuntimeState, KeyPackageRecord, PersistedClientState, RuntimeState,
    to_group_key,
};
use crate::types::{
    Bytes, ClientId, CreateClientParams, DeviceBinding, Error, Event, EventKind, GroupId,
    GroupState, IncomingMessage, IncomingMessageKind, InviteRequest, InviteResult,
    KeyPackageBundle, Member, MlsResult, RemoveRequest, RemoveResult, SelfUpdateResult, StatusCode,
};
use openmls::prelude::{MlsMessageIn, tls_codec::Deserialize as TlsDeserializeTrait};

/// Основной MLS-сервис для слоя приложения.
///
/// Хранит локальное runtime-состояние в памяти и делегирует криптографические
/// MLS-операции внутренней реализации [`MlsBackend`].
pub struct MessengerMls {
    state: RuntimeState,
    backend: Box<dyn MlsBackend>,
}

impl Default for MessengerMls {
    fn default() -> Self {
        Self::new()
    }
}

impl MessengerMls {
    const MAX_INCOMING_QUEUE: usize = 1024;
    const MAX_KEY_PACKAGES_PER_CALL: u32 = 4096;

    /// Сравнивает две логические клиентские идентичности.
    fn same_client(a: &ClientId, b: &ClientId) -> bool {
        a.user_id == b.user_id && a.device_id == b.device_id
    }

    /// Проверяет, что идентификатор группы не пустой.
    fn validate_group_id(group_id: &GroupId) -> MlsResult<()> {
        if group_id.value.is_empty() {
            return Err(Error::new(StatusCode::InvalidArgument, "group_id is empty"));
        }
        Ok(())
    }

    /// Создаёт сервис с backend по умолчанию [`OpenMlsBackend`].
    pub fn new() -> Self {
        Self {
            state: RuntimeState::new(),
            backend: Box::new(OpenMlsBackend::default()),
        }
    }

    /// Создаёт сервис с пользовательским backend.
    ///
    /// Полезно для тестов и альтернативных backend-реализаций.
    pub fn with_backend(backend: Box<dyn MlsBackend>) -> Self {
        Self {
            state: RuntimeState::new(),
            backend,
        }
    }

    /// Инициализирует локальную идентичность клиента и credential-контекст backend.
    ///
    /// Возвращает [`StatusCode::InvalidArgument`], если обязательные поля
    /// идентичности отсутствуют или `binding.client_id` не совпадает с `client_id`.
    /// Возвращает [`StatusCode::CryptoError`] или [`StatusCode::InvalidState`]
    /// при сбоях инициализации backend.
    pub fn create_client(&mut self, params: CreateClientParams) -> MlsResult<()> {
        if params.client_id.user_id.is_empty() || params.client_id.device_id.is_empty() {
            return Err(Error::new(
                StatusCode::InvalidArgument,
                "client_id.user_id and client_id.device_id are required",
            ));
        }
        if params.device_signature_private_key.is_empty() {
            return Err(Error::new(
                StatusCode::InvalidArgument,
                "device_signature_private_key is required",
            ));
        }
        if (!params.binding.client_id.user_id.is_empty()
            || !params.binding.client_id.device_id.is_empty())
            && (params.binding.client_id.user_id != params.client_id.user_id
                || params.binding.client_id.device_id != params.client_id.device_id)
        {
            return Err(Error::new(
                StatusCode::InvalidArgument,
                "binding.client_id must match client_id when provided",
            ));
        }

        // Re-init must not keep groups or key packages from previous identity.
        self.backend.reset();
        self.state = RuntimeState::new();
        self.backend.configure_client(&params)?;
        self.state.identity = Some(ClientIdentityState::from(&params));
        Ok(())
    }

    /// Восстанавливает runtime-состояние из байтов, полученных через [`Self::export_client_state`].
    ///
    /// Сейчас восстановление persisted-групп намеренно запрещено с
    /// [`StatusCode::Unsupported`], чтобы избежать небезопасного частичного
    /// восстановления OpenMLS.
    /// Возвращает [`StatusCode::InvalidArgument`] для невалидного JSON payload.
    pub fn restore_client(&mut self, serialized_client_state: &[u8]) -> MlsResult<()> {
        let persisted: PersistedClientState = serde_json::from_slice(serialized_client_state)
            .map_err(|e| Error::new(StatusCode::InvalidArgument, format!("invalid state: {e}")))?;
        if !persisted.groups.is_empty() {
            return Err(Error::new(
                StatusCode::Unsupported,
                "restoring persisted groups is not supported by the current OpenMLS backend",
            ));
        }

        self.backend.reset();
        if let Some(identity) = persisted.identity.as_ref() {
            let params = CreateClientParams {
                client_id: identity.client_id.clone(),
                device_signature_private_key: identity.signature_private_key.clone(),
                binding: DeviceBinding {
                    client_id: identity.client_id.clone(),
                    serialized_binding: identity.credential.clone(),
                    account_signature: identity.device_binding.clone(),
                },
                identity_data: identity.identity_data.clone(),
            };
            self.backend.configure_client(&params)?;
        }
        self.state = RuntimeState::restore(persisted);
        Ok(())
    }

    /// Сериализует текущее состояние клиента в JSON-blob.
    pub fn export_client_state(&self) -> MlsResult<Bytes> {
        let persisted = PersistedClientState::from_runtime(&self.state);
        serde_json::to_vec(&persisted)
            .map_err(|e| Error::new(StatusCode::StorageError, format!("serialize state: {e}")))
    }

    /// Возвращает инициализированную идентичность локального клиента.
    pub fn get_client_id(&self) -> MlsResult<ClientId> {
        self.require_identity().map(|x| x.client_id.clone())
    }

    /// Генерирует новые key package и добавляет их в локальный инвентарь.
    ///
    /// Возвращает [`StatusCode::InvalidState`], если клиент не инициализирован.
    /// Возвращает [`StatusCode::InvalidArgument`], если `count == 0`.
    pub fn create_key_packages(&mut self, count: u32) -> MlsResult<KeyPackageBundle> {
        self.require_identity()?;
        if count == 0 {
            return Err(Error::new(
                StatusCode::InvalidArgument,
                "count must be greater than 0",
            ));
        }
        if count > Self::MAX_KEY_PACKAGES_PER_CALL {
            return Err(Error::new(
                StatusCode::InvalidArgument,
                format!("count must be <= {}", Self::MAX_KEY_PACKAGES_PER_CALL),
            ));
        }
        let keypackages = self.backend.create_key_packages(count)?;

        for kp in &keypackages {
            self.state.key_package_counter = self.state.key_package_counter.saturating_add(1);
            self.state.key_packages.push(KeyPackageRecord {
                data: kp.clone(),
                uploaded: false,
                consumed: false,
                revoked: false,
                expired: false,
            });
        }

        Ok(KeyPackageBundle { keypackages })
    }

    /// Помечает ранее сгенерированный набор key package как загруженный.
    ///
    /// Возвращает [`StatusCode::NotFound`], если хотя бы один key package неизвестен.
    pub fn mark_key_packages_uploaded(&mut self, bundle: KeyPackageBundle) -> MlsResult<()> {
        let mut missing = 0usize;
        for kp in &bundle.keypackages {
            let mut matched = false;
            for existing in self.state.key_packages.iter_mut().filter(|x| x.data == *kp) {
                existing.uploaded = true;
                matched = true;
            }
            if !matched {
                missing += 1;
            }
        }
        if missing > 0 {
            return Err(Error::new(
                StatusCode::NotFound,
                format!("{} key package(s) were not found in local state", missing),
            ));
        }
        Ok(())
    }

    /// Создаёт группу и добавляет её в локальный runtime.
    ///
    /// Возвращает [`StatusCode::AlreadyExists`], если группа уже существует.
    pub fn create_group(&mut self, group_id: GroupId) -> MlsResult<GroupState> {
        let self_client = self.require_identity()?.client_id.clone();
        if group_id.value.is_empty() {
            return Err(Error::new(StatusCode::InvalidArgument, "group_id is empty"));
        }

        let key = to_group_key(&group_id.value);
        if self.state.groups.contains_key(&key) {
            return Err(Error::new(
                StatusCode::AlreadyExists,
                "group already exists",
            ));
        }

        let snapshot = self.backend.create_group(&group_id)?;

        let mut runtime = GroupRuntimeState::new(
            group_id,
            Member {
                client_id: self_client,
                is_self: true,
            },
        );
        Self::apply_snapshot(&mut runtime, &snapshot);

        let result = runtime.group_state.clone();
        self.state.groups.insert(key, runtime);
        Ok(result)
    }

    /// Возвращает снимки всех локально известных групп.
    pub fn list_groups(&self) -> MlsResult<Vec<GroupState>> {
        let mut out: Vec<_> = self
            .state
            .groups
            .values()
            .map(|x| x.group_state.clone())
            .collect();
        out.sort_by(|a, b| a.group_id.value.cmp(&b.group_id.value));
        Ok(out)
    }

    /// Возвращает состояние конкретной группы.
    pub fn get_group_state(&self, group_id: GroupId) -> MlsResult<GroupState> {
        Self::validate_group_id(&group_id)?;
        self.get_group(&group_id).map(|g| g.group_state.clone())
    }

    /// Возвращает список участников, отслеживаемых локальным runtime.
    pub fn list_members(&self, group_id: GroupId) -> MlsResult<Vec<Member>> {
        Self::validate_group_id(&group_id)?;
        self.get_group(&group_id).map(|g| {
            let mut members = g.members.clone();
            members.sort_by(|a, b| {
                (a.is_self, &a.client_id.user_id, &a.client_id.device_id).cmp(&(
                    b.is_self,
                    &b.client_id.user_id,
                    &b.client_id.device_id,
                ))
            });
            members
        })
    }

    /// Приглашает участника по key package и возвращает артефакты commit.
    ///
    /// Обновляет локальный runtime: выставляет `pending_commit` и оптимистично
    /// дополняет карту участников.
    /// Возвращает [`StatusCode::AlreadyExists`], если клиент уже в группе.
    pub fn invite(&mut self, request: InviteRequest) -> MlsResult<InviteResult> {
        let (group_id, invited_client, keypackage) =
            (request.group_id, request.invited_client, request.keypackage);
        Self::validate_group_id(&group_id)?;
        if keypackage.is_empty() {
            return Err(Error::new(
                StatusCode::InvalidArgument,
                "keypackage is empty",
            ));
        }
        {
            let group = self.get_group(&group_id)?;
            if group
                .members
                .iter()
                .any(|member| Self::same_client(&member.client_id, &invited_client))
            {
                return Err(Error::new(
                    StatusCode::AlreadyExists,
                    "invited client is already in group",
                ));
            }
        }

        let artifacts = self.backend.invite(&group_id, &keypackage)?;
        let group = self.get_group_mut(&group_id)?;

        group.pending_commit = true;
        Self::apply_snapshot(group, &artifacts.snapshot);

        let next_index = group
            .member_map
            .keys()
            .copied()
            .max()
            .map(|x| x.saturating_add(1))
            .unwrap_or(0);
        group.member_map.insert(next_index, invited_client.clone());
        group.members.push(Member {
            client_id: invited_client,
            is_self: false,
        });

        Ok(InviteResult {
            commit_message: artifacts.commit_message,
            welcome_message: artifacts.welcome_message.clone().unwrap_or_default(),
            has_welcome: artifacts.welcome_message.is_some(),
            group_state: group.group_state.clone(),
        })
    }

    /// Вступает в группу или обновляет её из байтов Welcome-сообщения.
    ///
    /// Возвращает [`StatusCode::InvalidArgument`] для пустого/невалидного welcome.
    pub fn join_from_welcome(&mut self, welcome_message: &[u8]) -> MlsResult<GroupState> {
        self.require_identity()?;
        if welcome_message.is_empty() {
            return Err(Error::new(
                StatusCode::InvalidArgument,
                "welcome_message is empty",
            ));
        }

        let (group_id, snapshot) = self.backend.join_from_welcome(welcome_message)?;
        let self_client = self.require_identity()?.client_id.clone();
        let key = to_group_key(&group_id.value);

        let mut runtime = self.state.groups.remove(&key).unwrap_or_else(|| {
            GroupRuntimeState::new(
                group_id.clone(),
                Member {
                    client_id: self_client,
                    is_self: true,
                },
            )
        });

        Self::apply_snapshot(&mut runtime, &snapshot);
        let group_state = runtime.group_state.clone();
        self.state.groups.insert(key, runtime);
        Ok(group_state)
    }

    /// Удаляет участника из группы и возвращает артефакты commit.
    ///
    /// Если удалённый участник совпадает с локальным клиентом, группа помечается
    /// как неактивная в локальном состоянии.
    /// Возвращает [`StatusCode::NotFound`], когда целевой участник не найден.
    pub fn remove(&mut self, request: RemoveRequest) -> MlsResult<RemoveResult> {
        let (group_id, removed_client) = (request.group_id, request.removed_client);
        Self::validate_group_id(&group_id)?;
        let self_id = self.require_identity()?.client_id.clone();

        let removed_leaf_index = {
            let group = self.get_group(&group_id)?;
            group
                .member_map
                .iter()
                .find(|(_, c)| Self::same_client(c, &removed_client))
                .map(|(leaf, _)| *leaf)
        };
        if removed_leaf_index.is_none() {
            return Err(Error::new(
                StatusCode::NotFound,
                "removed client is not present in group member map",
            ));
        }

        let artifacts = self.backend.remove(&group_id, removed_leaf_index)?;
        let group = self.get_group_mut(&group_id)?;

        group.pending_commit = true;
        Self::apply_snapshot(group, &artifacts.snapshot);
        group
            .members
            .retain(|m| !Self::same_client(&m.client_id, &removed_client));
        group
            .member_map
            .retain(|_, c| !Self::same_client(c, &removed_client));

        if Self::same_client(&removed_client, &self_id) {
            group.group_state.active = false;
            group.self_leaf_index = None;
        }

        Ok(RemoveResult {
            commit_message: artifacts.commit_message,
            group_state: group.group_state.clone(),
        })
    }

    /// Выполняет self-update для локального leaf в группе.
    pub fn self_update(&mut self, group_id: GroupId) -> MlsResult<SelfUpdateResult> {
        Self::validate_group_id(&group_id)?;
        let artifacts = self.backend.self_update(&group_id)?;
        let group = self.get_group_mut(&group_id)?;

        group.pending_commit = true;
        Self::apply_snapshot(group, &artifacts.snapshot);

        Ok(SelfUpdateResult {
            commit_message: artifacts.commit_message,
            group_state: group.group_state.clone(),
        })
    }

    /// Шифрует plaintext как MLS application message.
    ///
    /// `aad` прикрепляется как аутентифицированные дополнительные данные.
    pub fn encrypt_message(
        &mut self,
        group_id: GroupId,
        plaintext: Bytes,
        aad: Bytes,
    ) -> MlsResult<Bytes> {
        Self::validate_group_id(&group_id)?;
        self.get_group(&group_id)?;
        self.backend.encrypt(&group_id, &plaintext, &aad)
    }

    /// Обрабатывает входящее сообщение и возвращает высокоуровневые события.
    ///
    /// Для [`IncomingMessageKind::GroupMessage`] метод также обновляет локальную
    /// очередь входящих сообщений и транспортный offset для найденной группы.
    /// Возвращает [`StatusCode::InvalidArgument`], если MLS payload не парсится.
    pub fn handle_incoming(&mut self, message: IncomingMessage) -> MlsResult<Vec<Event>> {
        let mut events = Vec::new();

        match message.kind {
            IncomingMessageKind::Welcome => {
                let group_state = self.join_from_welcome(&message.payload)?;
                events.push(Event {
                    kind: EventKind::GroupJoined,
                    group_id: group_state.group_id,
                    actor: ClientId::default(),
                    subject: self.get_client_id().unwrap_or_default(),
                    message_plaintext: Vec::new(),
                });
            }
            IncomingMessageKind::GroupMessage => {
                let target_group = Self::incoming_group_id(&message)?;
                let plaintext = self.backend.handle_incoming(&message)?;

                if let Some(group_id) = target_group.clone() {
                    let group = self.get_group_mut(&group_id)?;
                    group.transport_last_offset = group.transport_last_offset.saturating_add(1);
                    group.incoming_queue.push_back(message.clone());
                    while group.incoming_queue.len() > Self::MAX_INCOMING_QUEUE {
                        let _ = group.incoming_queue.pop_front();
                    }
                }

                if let Some(plaintext) = plaintext {
                    events.push(Event {
                        kind: EventKind::MessageReceived,
                        group_id: target_group.unwrap_or_default(),
                        actor: ClientId::default(),
                        subject: self.get_client_id().unwrap_or_default(),
                        message_plaintext: plaintext,
                    });
                }
            }
        }

        Ok(events)
    }

    /// Возвращает, есть ли у группы pending commit (локальный или backend).
    pub fn has_pending_commit(&self, group_id: GroupId) -> MlsResult<bool> {
        Self::validate_group_id(&group_id)?;
        let local = self.get_group(&group_id).map(|g| g.pending_commit)?;
        if local {
            return Ok(true);
        }
        self.backend.has_pending_commit(&group_id)
    }

    /// Очищает состояние pending commit в backend и локальном runtime.
    pub fn clear_pending_commit(&mut self, group_id: GroupId) -> MlsResult<()> {
        Self::validate_group_id(&group_id)?;
        self.get_group(&group_id)?;
        self.backend.clear_pending_commit(&group_id)?;
        let group = self.get_group_mut(&group_id)?;
        group.pending_commit = false;
        group.staged_state = None;
        Ok(())
    }

    /// Удаляет группу из backend и локальных runtime-карт.
    pub fn drop_group(&mut self, group_id: GroupId) -> MlsResult<()> {
        Self::validate_group_id(&group_id)?;
        let key = to_group_key(&group_id.value);
        if !self.state.groups.contains_key(&key) {
            return Err(Error::new(StatusCode::NotFound, "group not found"));
        }
        self.backend.drop_group(&group_id)?;
        let _ = self.state.groups.remove(&key);
        Ok(())
    }

    /// Возвращает неизменяемую ссылку на runtime группы или [`StatusCode::NotFound`].
    fn get_group(&self, group_id: &GroupId) -> MlsResult<&GroupRuntimeState> {
        let key = to_group_key(&group_id.value);
        self.state
            .groups
            .get(&key)
            .ok_or_else(|| Error::new(StatusCode::NotFound, "group not found"))
    }

    /// Возвращает изменяемую ссылку на runtime группы или [`StatusCode::NotFound`].
    fn get_group_mut(&mut self, group_id: &GroupId) -> MlsResult<&mut GroupRuntimeState> {
        let key = to_group_key(&group_id.value);
        self.state
            .groups
            .get_mut(&key)
            .ok_or_else(|| Error::new(StatusCode::NotFound, "group not found"))
    }

    /// Возвращает текущую идентичность или [`StatusCode::InvalidState`], если её нет.
    fn require_identity(&self) -> MlsResult<&ClientIdentityState> {
        self.state
            .identity
            .as_ref()
            .ok_or_else(|| Error::new(StatusCode::InvalidState, "client is not initialized"))
    }

    /// Применяет backend-снимок к локальной runtime-проекции группы.
    fn apply_snapshot(group: &mut GroupRuntimeState, snapshot: &GroupSnapshot) {
        group.group_state.epoch = snapshot.epoch;
        group.group_state.active = snapshot.active;
        group.group_state.serialized_state = snapshot.serialized_state.clone();
        group.ratchet_tree_cache = snapshot.serialized_state.clone();
        group.secret_material = snapshot.secret_material.clone();
        group.self_leaf_index = snapshot.self_leaf_index;
    }

    /// Пытается извлечь `group_id` из входящего MLS wire payload.
    fn incoming_group_id(message: &IncomingMessage) -> MlsResult<Option<GroupId>> {
        match message.kind {
            IncomingMessageKind::Welcome => Ok(None),
            IncomingMessageKind::GroupMessage => {
                let mls_message =
                    MlsMessageIn::tls_deserialize_exact(&message.payload).map_err(|e| {
                        Error::new(
                            StatusCode::InvalidArgument,
                            format!("invalid incoming MLS message payload: {e}"),
                        )
                    })?;
                let protocol = mls_message.try_into_protocol_message().map_err(|e| {
                    Error::new(
                        StatusCode::InvalidArgument,
                        format!("invalid incoming protocol message: {e}"),
                    )
                })?;
                Ok(Some(GroupId {
                    value: protocol.group_id().as_slice().to_vec(),
                }))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[derive(Default)]
    struct MockBackend {
        configure_calls: Arc<AtomicUsize>,
        invite_calls: Arc<AtomicUsize>,
    }

    impl MockBackend {
        fn with_counters(
            configure_calls: Arc<AtomicUsize>,
            invite_calls: Arc<AtomicUsize>,
        ) -> Self {
            Self {
                configure_calls,
                invite_calls,
            }
        }

        fn snapshot(_group_id: &GroupId) -> GroupSnapshot {
            GroupSnapshot {
                epoch: 1,
                active: true,
                serialized_state: Vec::new(),
                secret_material: Vec::new(),
                self_leaf_index: Some(0),
            }
        }
    }

    impl MlsBackend for MockBackend {
        fn reset(&mut self) {}

        fn configure_client(&mut self, _params: &CreateClientParams) -> MlsResult<()> {
            self.configure_calls.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        fn create_key_packages(&mut self, count: u32) -> MlsResult<Vec<Bytes>> {
            Ok((0..count).map(|i| vec![i as u8]).collect())
        }

        fn create_group(&mut self, group_id: &GroupId) -> MlsResult<GroupSnapshot> {
            Ok(Self::snapshot(group_id))
        }

        fn invite(
            &mut self,
            group_id: &GroupId,
            _keypackage: &[u8],
        ) -> MlsResult<crate::backend::CommitArtifacts> {
            self.invite_calls.fetch_add(1, Ordering::Relaxed);
            Ok(crate::backend::CommitArtifacts {
                commit_message: vec![1],
                welcome_message: Some(vec![2]),
                snapshot: Self::snapshot(group_id),
            })
        }

        fn join_from_welcome(
            &mut self,
            _welcome_message: &[u8],
        ) -> MlsResult<(GroupId, GroupSnapshot)> {
            Err(Error::new(StatusCode::Unsupported, "not used in test"))
        }

        fn remove(
            &mut self,
            group_id: &GroupId,
            _removed_leaf_index: Option<u32>,
        ) -> MlsResult<crate::backend::CommitArtifacts> {
            Ok(crate::backend::CommitArtifacts {
                commit_message: vec![1],
                welcome_message: None,
                snapshot: Self::snapshot(group_id),
            })
        }

        fn self_update(
            &mut self,
            group_id: &GroupId,
        ) -> MlsResult<crate::backend::CommitArtifacts> {
            Ok(crate::backend::CommitArtifacts {
                commit_message: vec![1],
                welcome_message: None,
                snapshot: Self::snapshot(group_id),
            })
        }

        fn encrypt(
            &mut self,
            _group_id: &GroupId,
            _plaintext: &[u8],
            _aad: &[u8],
        ) -> MlsResult<Bytes> {
            Ok(Vec::new())
        }

        fn handle_incoming(&mut self, _message: &IncomingMessage) -> MlsResult<Option<Bytes>> {
            Ok(None)
        }

        fn has_pending_commit(&self, _group_id: &GroupId) -> MlsResult<bool> {
            Ok(false)
        }

        fn clear_pending_commit(&mut self, _group_id: &GroupId) -> MlsResult<()> {
            Ok(())
        }

        fn drop_group(&mut self, _group_id: &GroupId) -> MlsResult<()> {
            Ok(())
        }
    }

    fn make_params(user: &str, device: &str) -> CreateClientParams {
        CreateClientParams {
            client_id: ClientId {
                user_id: user.to_string(),
                device_id: device.to_string(),
            },
            device_signature_private_key: vec![7; 32],
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
    fn create_client_rejects_mismatched_binding_client_id() {
        let mut service = MessengerMls::with_backend(Box::new(MockBackend::default()));
        let mut params = make_params("alice", "phone");
        params.binding.client_id = ClientId {
            user_id: "bob".to_string(),
            device_id: "phone".to_string(),
        };

        let err = service
            .create_client(params)
            .expect_err("mismatched binding must fail");
        assert_eq!(err.code, StatusCode::InvalidArgument);
    }

    #[test]
    fn mark_uploaded_rejects_unknown_key_package() {
        let mut service = MessengerMls::with_backend(Box::new(MockBackend::default()));
        service
            .create_client(make_params("alice", "phone"))
            .unwrap();
        let _ = service.create_key_packages(1).unwrap();

        let err = service
            .mark_key_packages_uploaded(KeyPackageBundle {
                keypackages: vec![vec![99]],
            })
            .expect_err("unknown key package should fail");
        assert_eq!(err.code, StatusCode::NotFound);
    }

    #[test]
    fn create_key_packages_rejects_too_large_count() {
        let mut service = MessengerMls::with_backend(Box::new(MockBackend::default()));
        service
            .create_client(make_params("alice", "phone"))
            .expect("create");
        let err = service
            .create_key_packages(MessengerMls::MAX_KEY_PACKAGES_PER_CALL + 1)
            .expect_err("too large count");
        assert_eq!(err.code, StatusCode::InvalidArgument);
    }

    #[test]
    fn invite_rejects_duplicate_member_without_backend_call() {
        let configure_calls = Arc::new(AtomicUsize::new(0));
        let invite_calls = Arc::new(AtomicUsize::new(0));
        let backend = MockBackend::with_counters(configure_calls, invite_calls.clone());
        let mut service = MessengerMls::with_backend(Box::new(backend));

        service
            .create_client(make_params("alice", "phone"))
            .unwrap();
        service
            .create_group(GroupId {
                value: b"group-a".to_vec(),
            })
            .unwrap();

        let err = service
            .invite(InviteRequest {
                group_id: GroupId {
                    value: b"group-a".to_vec(),
                },
                invited_client: ClientId {
                    user_id: "alice".to_string(),
                    device_id: "phone".to_string(),
                },
                keypackage: vec![1],
            })
            .expect_err("duplicate invite should fail");
        assert_eq!(err.code, StatusCode::AlreadyExists);
        assert_eq!(invite_calls.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn invite_rejects_empty_keypackage() {
        let mut service = MessengerMls::with_backend(Box::new(MockBackend::default()));
        service
            .create_client(make_params("alice", "phone"))
            .expect("create");
        service
            .create_group(GroupId {
                value: b"group-a".to_vec(),
            })
            .expect("group");
        let err = service
            .invite(InviteRequest {
                group_id: GroupId {
                    value: b"group-a".to_vec(),
                },
                invited_client: ClientId {
                    user_id: "bob".to_string(),
                    device_id: "phone".to_string(),
                },
                keypackage: Vec::new(),
            })
            .expect_err("empty keypackage");
        assert_eq!(err.code, StatusCode::InvalidArgument);
    }

    #[test]
    fn restore_client_rejects_persisted_groups() {
        let mut service = MessengerMls::with_backend(Box::new(MockBackend::default()));
        let persisted = PersistedClientState {
            identity: None,
            groups: vec![GroupRuntimeState::new(
                GroupId {
                    value: b"group-a".to_vec(),
                },
                Member {
                    client_id: ClientId {
                        user_id: "alice".to_string(),
                        device_id: "phone".to_string(),
                    },
                    is_self: true,
                },
            )],
            key_packages: Vec::new(),
            key_package_counter: 0,
        };
        let bytes = serde_json::to_vec(&persisted).unwrap();
        let err = service
            .restore_client(&bytes)
            .expect_err("groups restore should be unsupported");
        assert_eq!(err.code, StatusCode::Unsupported);
    }

    #[test]
    fn restore_client_reconfigures_backend_identity() {
        let configure_calls = Arc::new(AtomicUsize::new(0));
        let backend =
            MockBackend::with_counters(configure_calls.clone(), Arc::new(AtomicUsize::new(0)));
        let mut service = MessengerMls::with_backend(Box::new(backend));

        let persisted = PersistedClientState {
            identity: Some(ClientIdentityState::from(&make_params("alice", "phone"))),
            groups: Vec::new(),
            key_packages: Vec::new(),
            key_package_counter: 0,
        };
        let bytes = serde_json::to_vec(&persisted).unwrap();
        service.restore_client(&bytes).unwrap();

        assert_eq!(configure_calls.load(Ordering::Relaxed), 1);
        assert_eq!(service.get_client_id().unwrap().user_id, "alice");
    }

    #[test]
    fn mock_backend_direct_methods_are_exercised() {
        let mut backend = MockBackend::default();
        let group_id = GroupId {
            value: b"group-mock".to_vec(),
        };

        backend.reset();
        backend
            .configure_client(&make_params("alice", "phone"))
            .unwrap();
        assert_eq!(backend.create_key_packages(2).unwrap().len(), 2);
        let _ = backend.create_group(&group_id).unwrap();
        let _ = backend.invite(&group_id, &[1, 2, 3]).unwrap();
        assert!(backend.join_from_welcome(&[1]).is_err());
        let _ = backend.remove(&group_id, Some(0)).unwrap();
        let _ = backend.self_update(&group_id).unwrap();
        let _ = backend.encrypt(&group_id, b"pt", b"aad").unwrap();
        let _ = backend
            .handle_incoming(&IncomingMessage::default())
            .unwrap();
        assert!(!backend.has_pending_commit(&group_id).unwrap());
        backend.clear_pending_commit(&group_id).unwrap();
        backend.drop_group(&group_id).unwrap();
    }

    #[test]
    fn incoming_group_id_branches() {
        let none = MessengerMls::incoming_group_id(&IncomingMessage {
            kind: IncomingMessageKind::Welcome,
            payload: vec![1, 2, 3],
        })
        .expect("welcome maps to none");
        assert!(none.is_none());

        let mut producer = MessengerMls::new();
        producer
            .create_client(make_params("alice", "phone"))
            .expect("producer create");
        let gid = GroupId {
            value: b"incoming-group-id".to_vec(),
        };
        producer.create_group(gid.clone()).expect("producer group");
        let welcome = producer
            .join_from_welcome(b"invalid")
            .expect_err("invalid welcome")
            .code;
        assert_eq!(welcome, StatusCode::CryptoError);

        // Build a valid MLS wire message and ensure group id extraction works.
        let ciphertext = producer
            .encrypt_message(gid.clone(), b"x".to_vec(), vec![])
            .expect("encrypt");
        let extracted = MessengerMls::incoming_group_id(&IncomingMessage {
            kind: IncomingMessageKind::GroupMessage,
            payload: ciphertext,
        })
        .expect("extract group id")
        .expect("some group id");
        assert_eq!(extracted.value, gid.value);

        // Feed a Welcome payload as GroupMessage to hit protocol conversion error branch.
        let mut a = MessengerMls::new();
        let mut b = MessengerMls::new();
        a.create_client(make_params("a", "d")).expect("a client");
        let mut b_params = make_params("b", "d");
        b_params.device_signature_private_key = vec![8; 32];
        b.create_client(b_params).expect("b client");
        let kp = b.create_key_packages(1).expect("kp").keypackages.remove(0);
        let g2 = GroupId {
            value: b"incoming-g2".to_vec(),
        };
        a.create_group(g2.clone()).expect("g2 create");
        let inv = a
            .invite(InviteRequest {
                group_id: g2,
                invited_client: b.get_client_id().expect("b id"),
                keypackage: kp,
            })
            .expect("invite");
        let err = MessengerMls::incoming_group_id(&IncomingMessage {
            kind: IncomingMessageKind::GroupMessage,
            payload: inv.welcome_message,
        })
        .expect_err("welcome payload is not protocol message");
        assert_eq!(err.code, StatusCode::InvalidArgument);
    }

    #[test]
    fn handle_incoming_group_message_enqueues_into_runtime_queue() {
        let mut service = MessengerMls::with_backend(Box::new(MockBackend::default()));
        let gid = GroupId {
            value: b"queue-group".to_vec(),
        };
        service
            .create_client(make_params("queue-user", "phone"))
            .expect("create client");
        service.create_group(gid.clone()).expect("create group");

        // Build a valid MLS GroupMessage payload via real backend so incoming_group_id() succeeds.
        let mut producer = MessengerMls::new();
        producer
            .create_client(make_params("producer-user", "phone"))
            .expect("producer client");
        producer.create_group(gid.clone()).expect("producer group");
        let payload = producer
            .encrypt_message(gid.clone(), b"payload".to_vec(), b"aad".to_vec())
            .expect("producer encrypt");

        let events = service
            .handle_incoming(IncomingMessage {
                kind: IncomingMessageKind::GroupMessage,
                payload,
            })
            .expect("incoming handled");
        assert!(events.is_empty(), "mock backend returns no plaintext");

        let exported = service.export_client_state().expect("export state");
        let persisted: PersistedClientState =
            serde_json::from_slice(&exported).expect("decode persisted");
        let group = persisted
            .groups
            .into_iter()
            .find(|g| g.group_state.group_id.value == gid.value)
            .expect("group exists in persisted state");
        assert_eq!(group.transport_last_offset, 1);
        assert_eq!(group.incoming_queue.len(), 1);
    }

    #[test]
    fn create_client_reinitialization_clears_runtime_state() {
        let mut service = MessengerMls::with_backend(Box::new(MockBackend::default()));
        service
            .create_client(make_params("alice", "phone"))
            .expect("first create");
        service
            .create_group(GroupId {
                value: b"g-reset".to_vec(),
            })
            .expect("group created");
        let _ = service.create_key_packages(2).expect("kps");

        service
            .create_client(make_params("bob", "laptop"))
            .expect("recreate resets state");

        assert!(service.list_groups().expect("list").is_empty());
        let exported = service.export_client_state().expect("export");
        let persisted: PersistedClientState = serde_json::from_slice(&exported).expect("decode");
        assert!(persisted.key_packages.is_empty());
        assert_eq!(persisted.key_package_counter, 0);
        assert_eq!(
            persisted.identity.expect("identity").client_id.user_id,
            "bob".to_string()
        );
    }

    #[test]
    fn api_rejects_empty_group_id_consistently() {
        let mut service = MessengerMls::with_backend(Box::new(MockBackend::default()));
        service
            .create_client(make_params("alice", "phone"))
            .expect("create client");
        let empty = GroupId { value: Vec::new() };

        assert_eq!(
            service
                .get_group_state(empty.clone())
                .expect_err("empty group id")
                .code,
            StatusCode::InvalidArgument
        );
        assert_eq!(
            service
                .list_members(empty.clone())
                .expect_err("empty group id")
                .code,
            StatusCode::InvalidArgument
        );
        assert_eq!(
            service
                .self_update(empty.clone())
                .expect_err("empty group id")
                .code,
            StatusCode::InvalidArgument
        );
        assert_eq!(
            service
                .encrypt_message(empty.clone(), vec![1], vec![])
                .expect_err("empty group id")
                .code,
            StatusCode::InvalidArgument
        );
        assert_eq!(
            service
                .has_pending_commit(empty.clone())
                .expect_err("empty group id")
                .code,
            StatusCode::InvalidArgument
        );
        assert_eq!(
            service
                .clear_pending_commit(empty.clone())
                .expect_err("empty group id")
                .code,
            StatusCode::InvalidArgument
        );
        assert_eq!(
            service.drop_group(empty).expect_err("empty group id").code,
            StatusCode::InvalidArgument
        );
    }

    #[test]
    fn incoming_queue_is_bounded() {
        let mut service = MessengerMls::with_backend(Box::new(MockBackend::default()));
        let gid = GroupId {
            value: b"queue-bounded".to_vec(),
        };
        service
            .create_client(make_params("queue-user", "phone"))
            .expect("create client");
        service.create_group(gid.clone()).expect("create group");

        let mut producer = MessengerMls::new();
        producer
            .create_client(make_params("producer-user", "phone"))
            .expect("producer client");
        producer.create_group(gid.clone()).expect("producer group");
        let payload = producer
            .encrypt_message(gid.clone(), b"payload".to_vec(), b"aad".to_vec())
            .expect("producer encrypt");

        for _ in 0..(MessengerMls::MAX_INCOMING_QUEUE + 16) {
            let _ = service
                .handle_incoming(IncomingMessage {
                    kind: IncomingMessageKind::GroupMessage,
                    payload: payload.clone(),
                })
                .expect("incoming handled");
        }

        let exported = service.export_client_state().expect("export state");
        let persisted: PersistedClientState =
            serde_json::from_slice(&exported).expect("decode persisted");
        let group = persisted
            .groups
            .into_iter()
            .find(|g| g.group_state.group_id.value == gid.value)
            .expect("group exists");
        assert_eq!(
            group.transport_last_offset,
            (MessengerMls::MAX_INCOMING_QUEUE + 16) as u64
        );
        assert_eq!(group.incoming_queue.len(), MessengerMls::MAX_INCOMING_QUEUE);
    }

    #[test]
    fn list_members_is_stable_and_sorted() {
        let mut service = MessengerMls::with_backend(Box::new(MockBackend::default()));
        let gid = GroupId {
            value: b"members-sorted".to_vec(),
        };
        service
            .create_client(make_params("self", "phone"))
            .expect("create");
        service.create_group(gid.clone()).expect("group");
        let _ = service
            .invite(InviteRequest {
                group_id: gid.clone(),
                invited_client: ClientId {
                    user_id: "zzz".to_string(),
                    device_id: "1".to_string(),
                },
                keypackage: vec![1],
            })
            .expect("invite z");
        let _ = service
            .invite(InviteRequest {
                group_id: gid.clone(),
                invited_client: ClientId {
                    user_id: "aaa".to_string(),
                    device_id: "1".to_string(),
                },
                keypackage: vec![2],
            })
            .expect("invite a");

        let members = service.list_members(gid).expect("members");
        assert_eq!(members.len(), 3);
        assert_eq!(members[0].client_id.user_id, "aaa");
        assert_eq!(members[1].client_id.user_id, "zzz");
        assert_eq!(members[2].client_id.user_id, "self");
    }
}
