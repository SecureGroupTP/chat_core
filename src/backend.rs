//! Backend-абстракция над MLS-примитивами.
//!
//! Сервисный слой использует [`crate::backend::MlsBackend`], чтобы бизнес-логика
//! была независима от конкретной MLS-реализации.

use crate::state::to_group_key;
use crate::types::{
    Bytes, CreateClientParams, Error, GroupId, IncomingMessage, IncomingMessageKind, MlsResult,
    StatusCode,
};
use ed25519_dalek::{SigningKey, VerifyingKey};
use openmls::prelude::{
    BasicCredential, Ciphersuite, CredentialWithKey, GroupId as OpenMlsGroupId, KeyPackage,
    KeyPackageIn, LeafNodeIndex, MlsGroup, MlsGroupCreateConfig, MlsGroupJoinConfig,
    MlsMessageBodyIn, MlsMessageIn, OpenMlsProvider, ProtocolVersion, StagedWelcome,
    tls_codec::{Deserialize as TlsDeserializeTrait, Serialize as TlsSerializeTrait},
};
use openmls_basic_credential::SignatureKeyPair;
use openmls_rust_crypto::OpenMlsRustCrypto;
use std::collections::HashMap;

#[derive(Debug, Clone)]
/// Снимок состояния группы из backend, используемый сервисным рантаймом.
pub struct GroupSnapshot {
    /// Эпоха группы после последней успешной мутации в backend.
    pub epoch: u64,
    /// Считает ли backend локального участника активным.
    pub active: bool,
    /// Сериализованная часть состояния группы (экспорт ratchet tree).
    pub serialized_state: Bytes,
    /// Экспортированный секретный материал для локальных функций рантайма.
    pub secret_material: Bytes,
    /// Leaf-индекс локального клиента в группе (если известен).
    pub self_leaf_index: Option<u32>,
}

#[derive(Debug, Clone)]
/// Бинарные артефакты, создаваемые commit-подобными операциями.
pub struct CommitArtifacts {
    /// Wire-сообщение commit для рассылки.
    pub commit_message: Bytes,
    /// Опциональное Welcome-сообщение для новых участников.
    pub welcome_message: Option<Bytes>,
    /// Обновлённый снимок группы после операции.
    pub snapshot: GroupSnapshot,
}

/// Трейт MLS-движка, используемый [`crate::service::MessengerMls`].
pub trait MlsBackend: Send + Sync {
    /// Сбрасывает runtime и криптографический контекст backend.
    fn reset(&mut self);
    /// Настраивает credential и подпись клиента.
    fn configure_client(&mut self, params: &CreateClientParams) -> MlsResult<()>;
    /// Создаёт `count` сериализованных key package.
    fn create_key_packages(&mut self, count: u32) -> MlsResult<Vec<Bytes>>;
    /// Создаёт новую MLS-группу с переданным внешним идентификатором.
    fn create_group(&mut self, group_id: &GroupId) -> MlsResult<GroupSnapshot>;
    /// Добавляет участника, представленного `keypackage`, и возвращает артефакты commit.
    fn invite(&mut self, group_id: &GroupId, keypackage: &[u8]) -> MlsResult<CommitArtifacts>;
    /// Вступает в группу из Welcome payload и возвращает итоговый снимок группы.
    fn join_from_welcome(&mut self, welcome_message: &[u8]) -> MlsResult<(GroupId, GroupSnapshot)>;
    /// Удаляет участника по leaf-индексу из указанной группы.
    fn remove(
        &mut self,
        group_id: &GroupId,
        removed_leaf_index: Option<u32>,
    ) -> MlsResult<CommitArtifacts>;
    /// Выполняет self-update commit для локального leaf.
    fn self_update(&mut self, group_id: &GroupId) -> MlsResult<CommitArtifacts>;
    /// Шифрует application message в указанной группе.
    fn encrypt(&mut self, group_id: &GroupId, plaintext: &[u8], aad: &[u8]) -> MlsResult<Bytes>;
    /// Обрабатывает входящее MLS-сообщение и возвращает plaintext при наличии.
    fn handle_incoming(&mut self, message: &IncomingMessage) -> MlsResult<Option<Bytes>>;
    /// Возвращает, есть ли у backend несмерженный pending commit для группы.
    fn has_pending_commit(&self, group_id: &GroupId) -> MlsResult<bool>;
    /// Очищает состояние pending commit в backend для группы.
    fn clear_pending_commit(&mut self, group_id: &GroupId) -> MlsResult<()>;
    /// Удаляет группу из runtime backend.
    fn drop_group(&mut self, group_id: &GroupId) -> MlsResult<()>;
}

#[derive(Debug, Default)]
/// Реализация [`MlsBackend`] по умолчанию на базе OpenMLS.
pub struct OpenMlsBackend {
    provider: OpenMlsRustCrypto,
    signer_private: Option<Vec<u8>>,
    signer_public: Option<Vec<u8>>,
    credential_with_key: Option<CredentialWithKey>,
    groups: HashMap<String, MlsGroup>,
}

impl OpenMlsBackend {
    const CIPHERSUITE: Ciphersuite = Ciphersuite::MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519;
    const MAX_KEY_PACKAGES_PER_CALL: u32 = 4096;

    /// Возвращает key pair подписи из настроенного сырого ключевого материала.
    fn signer(&self) -> MlsResult<SignatureKeyPair> {
        let private = self.signer_private.clone().ok_or_else(|| {
            Error::new(StatusCode::InvalidState, "client signer is not configured")
        })?;
        let public = self.signer_public.clone().ok_or_else(|| {
            Error::new(StatusCode::InvalidState, "client signer is not configured")
        })?;

        Ok(SignatureKeyPair::from_raw(
            Self::CIPHERSUITE.signature_algorithm(),
            private,
            public,
        ))
    }

    /// Возвращает настроенный credential и связанный публичный ключ подписи.
    fn credential(&self) -> MlsResult<CredentialWithKey> {
        self.credential_with_key
            .clone()
            .ok_or_else(|| Error::new(StatusCode::InvalidState, "credential is not configured"))
    }

    /// Ищет группу по идентификатору.
    fn group_ref(&self, group_id: &GroupId) -> MlsResult<&MlsGroup> {
        let key = to_group_key(&group_id.value);
        self.groups
            .get(&key)
            .ok_or_else(|| Error::new(StatusCode::NotFound, "group not found"))
    }

    /// Проверяет, что `group_id` не пустой.
    fn validate_group_id(group_id: &GroupId) -> MlsResult<()> {
        if group_id.value.is_empty() {
            return Err(Error::new(StatusCode::InvalidArgument, "group_id is empty"));
        }
        Ok(())
    }

    /// Строит снимок для сервисного слоя из экземпляра OpenMLS-группы.
    fn snapshot_for(group: &MlsGroup, provider: &OpenMlsRustCrypto) -> MlsResult<GroupSnapshot> {
        let serialized_state = group
            .export_ratchet_tree()
            .tls_serialize_detached()
            .map_err(|e| Self::map_err("serialize ratchet tree", e))?;

        let secret_material = group
            .export_secret(provider.crypto(), "chat_core_state", &[], 32)
            .map_err(|e| Self::map_err("export group secret", e))?;

        Ok(GroupSnapshot {
            epoch: group.epoch().as_u64(),
            active: group.is_active(),
            serialized_state,
            secret_material,
            self_leaf_index: Some(group.own_leaf_index().u32()),
        })
    }

    /// Разбирает Ed25519 приватный ключ в формате, который принимает публичный API.
    ///
    /// Допустимые форматы: 32-байтный seed или 64-байтный keypair.
    fn parse_device_key(raw: &[u8]) -> MlsResult<(Vec<u8>, Vec<u8>)> {
        if raw.len() == 32 {
            let mut sk = [0u8; 32];
            sk.copy_from_slice(raw);
            let signing = SigningKey::from_bytes(&sk);
            let verifying: VerifyingKey = signing.verifying_key();
            return Ok((sk.to_vec(), verifying.to_bytes().to_vec()));
        }

        if raw.len() == 64 {
            let mut keypair = [0u8; 64];
            keypair.copy_from_slice(raw);
            let signing = SigningKey::from_keypair_bytes(&keypair).map_err(|_| {
                Error::new(
                    StatusCode::InvalidArgument,
                    "invalid 64-byte ed25519 keypair in device_signature_private_key",
                )
            })?;
            let verifying: VerifyingKey = signing.verifying_key();
            return Ok((signing.to_bytes().to_vec(), verifying.to_bytes().to_vec()));
        }

        Err(Error::new(
            StatusCode::InvalidArgument,
            "device_signature_private_key must be 32-byte seed or 64-byte keypair",
        ))
    }

    /// Приводит низкоуровневые crypto-ошибки к единому формату сервисных ошибок.
    fn map_err<E: core::fmt::Display>(context: &str, err: E) -> Error {
        Error::new(StatusCode::CryptoError, format!("{context}: {err}"))
    }
}

impl MlsBackend for OpenMlsBackend {
    fn reset(&mut self) {
        self.provider = OpenMlsRustCrypto::default();
        self.signer_private = None;
        self.signer_public = None;
        self.credential_with_key = None;
        self.groups.clear();
    }

    fn configure_client(&mut self, params: &CreateClientParams) -> MlsResult<()> {
        self.reset();
        let (private, public) = Self::parse_device_key(&params.device_signature_private_key)?;

        let signer = SignatureKeyPair::from_raw(
            Self::CIPHERSUITE.signature_algorithm(),
            private.clone(),
            public.clone(),
        );
        signer
            .store(self.provider.storage())
            .map_err(|e| Self::map_err("store signature key", e))?;

        let identity = if params.identity_data.is_empty() {
            format!(
                "{}:{}",
                params.client_id.user_id, params.client_id.device_id
            )
            .into_bytes()
        } else {
            params.identity_data.clone()
        };

        let credential = BasicCredential::new(identity);
        self.credential_with_key = Some(CredentialWithKey {
            credential: credential.into(),
            signature_key: public.clone().into(),
        });
        self.signer_private = Some(private);
        self.signer_public = Some(public);
        Ok(())
    }

    fn create_key_packages(&mut self, count: u32) -> MlsResult<Vec<Bytes>> {
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

        let signer = self.signer()?;
        let credential = self.credential()?;

        let mut out = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let kp_bundle = KeyPackage::builder()
                .build(
                    Self::CIPHERSUITE,
                    &self.provider,
                    &signer,
                    credential.clone(),
                )
                .map_err(|e| Self::map_err("create key package", e))?;

            let bytes = kp_bundle
                .key_package()
                .tls_serialize_detached()
                .map_err(|e| Self::map_err("serialize key package", e))?;
            out.push(bytes);
        }

        Ok(out)
    }

    fn create_group(&mut self, group_id: &GroupId) -> MlsResult<GroupSnapshot> {
        Self::validate_group_id(group_id)?;
        let key = to_group_key(&group_id.value);
        if self.groups.contains_key(&key) {
            return Err(Error::new(
                StatusCode::AlreadyExists,
                "group already exists",
            ));
        }

        let signer = self.signer()?;
        let credential = self.credential()?;
        let config = MlsGroupCreateConfig::builder()
            .ciphersuite(Self::CIPHERSUITE)
            .use_ratchet_tree_extension(true)
            .build();

        let group = MlsGroup::new_with_group_id(
            &self.provider,
            &signer,
            &config,
            OpenMlsGroupId::from_slice(&group_id.value),
            credential,
        )
        .map_err(|e| Self::map_err("create group", e))?;

        let snapshot = Self::snapshot_for(&group, &self.provider)?;
        self.groups.insert(key, group);
        Ok(snapshot)
    }

    fn invite(&mut self, group_id: &GroupId, keypackage: &[u8]) -> MlsResult<CommitArtifacts> {
        Self::validate_group_id(group_id)?;
        if keypackage.is_empty() {
            return Err(Error::new(
                StatusCode::InvalidArgument,
                "keypackage is empty",
            ));
        }
        let signer = self.signer()?;

        let key_package_in = KeyPackageIn::tls_deserialize_exact(keypackage)
            .map_err(|e| Self::map_err("deserialize key package", e))?;
        let key_package: KeyPackage = key_package_in
            .validate(self.provider.crypto(), ProtocolVersion::Mls10)
            .map_err(|e| Self::map_err("validate key package", e))?;

        let key = to_group_key(&group_id.value);
        let group = self
            .groups
            .get_mut(&key)
            .ok_or_else(|| Error::new(StatusCode::NotFound, "group not found"))?;

        let (commit, welcome, _group_info) = group
            .add_members(&self.provider, &signer, &[key_package])
            .map_err(|e| Self::map_err("add member", e))?;

        let commit_message = commit
            .to_bytes()
            .map_err(|e| Self::map_err("serialize commit", e))?;

        let welcome_message = Some(
            welcome
                .to_bytes()
                .map_err(|e| Self::map_err("serialize welcome", e))?,
        );

        Ok(CommitArtifacts {
            commit_message,
            welcome_message,
            snapshot: Self::snapshot_for(group, &self.provider)?,
        })
    }

    fn join_from_welcome(&mut self, welcome_message: &[u8]) -> MlsResult<(GroupId, GroupSnapshot)> {
        if welcome_message.is_empty() {
            return Err(Error::new(
                StatusCode::InvalidArgument,
                "welcome_message is empty",
            ));
        }
        let mls_message = MlsMessageIn::tls_deserialize_exact(welcome_message)
            .map_err(|e| Self::map_err("deserialize welcome message", e))?;
        let welcome = match mls_message.extract() {
            MlsMessageBodyIn::Welcome(w) => w,
            _ => {
                return Err(Error::new(
                    StatusCode::InvalidArgument,
                    "expected Welcome message",
                ));
            }
        };

        let config = MlsGroupJoinConfig::builder()
            .use_ratchet_tree_extension(true)
            .build();

        let staged = StagedWelcome::new_from_welcome(&self.provider, &config, welcome, None)
            .map_err(|e| Self::map_err("stage welcome", e))?;
        let group = staged
            .into_group(&self.provider)
            .map_err(|e| Self::map_err("join from welcome", e))?;

        let group_id = GroupId {
            value: group.group_id().as_slice().to_vec(),
        };
        let key = to_group_key(&group_id.value);
        let snapshot = Self::snapshot_for(&group, &self.provider)?;
        self.groups.insert(key, group);
        Ok((group_id, snapshot))
    }

    fn remove(
        &mut self,
        group_id: &GroupId,
        removed_leaf_index: Option<u32>,
    ) -> MlsResult<CommitArtifacts> {
        Self::validate_group_id(group_id)?;
        let signer = self.signer()?;

        let leaf = removed_leaf_index.ok_or_else(|| {
            Error::new(
                StatusCode::NotFound,
                "cannot remove: target member leaf index is unknown",
            )
        })?;

        let key = to_group_key(&group_id.value);
        let group = self
            .groups
            .get_mut(&key)
            .ok_or_else(|| Error::new(StatusCode::NotFound, "group not found"))?;

        let (commit, welcome, _group_info) = group
            .remove_members(&self.provider, &signer, &[LeafNodeIndex::new(leaf)])
            .map_err(|e| Self::map_err("remove member", e))?;

        let commit_message = commit
            .to_bytes()
            .map_err(|e| Self::map_err("serialize commit", e))?;
        let welcome_message = match welcome {
            Some(msg) => Some(
                msg.to_bytes()
                    .map_err(|e| Self::map_err("serialize welcome", e))?,
            ),
            None => None,
        };

        Ok(CommitArtifacts {
            commit_message,
            welcome_message,
            snapshot: Self::snapshot_for(group, &self.provider)?,
        })
    }

    fn self_update(&mut self, group_id: &GroupId) -> MlsResult<CommitArtifacts> {
        Self::validate_group_id(group_id)?;
        let signer = self.signer()?;
        let key = to_group_key(&group_id.value);
        let group = self
            .groups
            .get_mut(&key)
            .ok_or_else(|| Error::new(StatusCode::NotFound, "group not found"))?;

        let (commit, welcome, _group_info) = group
            .self_update(&self.provider, &signer, Default::default())
            .map_err(|e| Self::map_err("self update", e))?
            .into_contents();

        let commit_message = commit
            .to_bytes()
            .map_err(|e| Self::map_err("serialize commit", e))?;
        let welcome_message = match welcome {
            Some(msg) => Some(
                msg.tls_serialize_detached()
                    .map_err(|e| Self::map_err("serialize welcome", e))?,
            ),
            None => None,
        };

        Ok(CommitArtifacts {
            commit_message,
            welcome_message,
            snapshot: Self::snapshot_for(group, &self.provider)?,
        })
    }

    fn encrypt(&mut self, group_id: &GroupId, plaintext: &[u8], aad: &[u8]) -> MlsResult<Bytes> {
        Self::validate_group_id(group_id)?;
        let key = to_group_key(&group_id.value);
        let signer = self.signer()?;
        let provider = &self.provider;
        let group = self
            .groups
            .get_mut(&key)
            .ok_or_else(|| Error::new(StatusCode::NotFound, "group not found"))?;

        group.set_aad(aad.to_vec());
        let out = group
            .create_message(provider, &signer, plaintext)
            .map_err(|e| Self::map_err("encrypt/create application message", e))?;

        out.to_bytes()
            .map_err(|e| Self::map_err("serialize application message", e))
    }

    fn handle_incoming(&mut self, message: &IncomingMessage) -> MlsResult<Option<Bytes>> {
        if message.payload.is_empty() {
            return Err(Error::new(
                StatusCode::InvalidArgument,
                "incoming payload is empty",
            ));
        }
        match message.kind {
            IncomingMessageKind::Welcome => {
                let _ = self.join_from_welcome(&message.payload)?;
                Ok(None)
            }
            IncomingMessageKind::GroupMessage => {
                let mls_message = MlsMessageIn::tls_deserialize_exact(&message.payload)
                    .map_err(|e| Self::map_err("deserialize incoming message", e))?;
                let protocol = mls_message
                    .try_into_protocol_message()
                    .map_err(|e| Self::map_err("invalid incoming protocol message", e))?;

                let target_group = GroupId {
                    value: protocol.group_id().as_slice().to_vec(),
                };
                let key = to_group_key(&target_group.value);
                let provider = &self.provider;
                let group = self
                    .groups
                    .get_mut(&key)
                    .ok_or_else(|| Error::new(StatusCode::NotFound, "group not found"))?;

                let processed = group
                    .process_message(provider, protocol)
                    .map_err(|e| Self::map_err("process incoming", e))?;

                match processed.into_content() {
                    openmls::prelude::ProcessedMessageContent::ApplicationMessage(app) => {
                        Ok(Some(app.into_bytes()))
                    }
                    openmls::prelude::ProcessedMessageContent::StagedCommitMessage(staged) => {
                        group
                            .merge_staged_commit(provider, *staged)
                            .map_err(|e| Self::map_err("merge staged commit", e))?;
                        Ok(None)
                    }
                    _ => Ok(None),
                }
            }
        }
    }

    fn has_pending_commit(&self, group_id: &GroupId) -> MlsResult<bool> {
        Self::validate_group_id(group_id)?;
        Ok(self.group_ref(group_id)?.pending_commit().is_some())
    }

    fn clear_pending_commit(&mut self, group_id: &GroupId) -> MlsResult<()> {
        Self::validate_group_id(group_id)?;
        let storage = self.provider.storage();
        let key = to_group_key(&group_id.value);
        let group = self
            .groups
            .get_mut(&key)
            .ok_or_else(|| Error::new(StatusCode::NotFound, "group not found"))?;
        group
            .clear_pending_commit(storage)
            .map_err(|e| Self::map_err("clear pending commit", e))
    }

    fn drop_group(&mut self, group_id: &GroupId) -> MlsResult<()> {
        Self::validate_group_id(group_id)?;
        let key = to_group_key(&group_id.value);
        if self.groups.remove(&key).is_none() {
            return Err(Error::new(StatusCode::NotFound, "group not found"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params_with_key(key: Vec<u8>, identity_data: Vec<u8>) -> CreateClientParams {
        CreateClientParams {
            client_id: crate::types::ClientId {
                user_id: "u".to_string(),
                device_id: "d".to_string(),
            },
            device_signature_private_key: key,
            binding: crate::types::DeviceBinding {
                client_id: crate::types::ClientId {
                    user_id: "u".to_string(),
                    device_id: "d".to_string(),
                },
                serialized_binding: vec![],
                account_signature: vec![],
            },
            identity_data,
        }
    }

    #[test]
    fn internal_state_error_branches() {
        let mut backend = OpenMlsBackend {
            signer_private: Some(vec![1; 32]),
            ..OpenMlsBackend::default()
        };
        assert_eq!(
            backend.signer().expect_err("missing public").code,
            StatusCode::InvalidState
        );

        backend.signer_private = None;
        backend.signer_public = Some(vec![2; 32]);
        assert_eq!(
            backend.signer().expect_err("missing private").code,
            StatusCode::InvalidState
        );

        backend.credential_with_key = None;
        assert_eq!(
            backend.credential().expect_err("missing credential").code,
            StatusCode::InvalidState
        );

        assert_eq!(
            backend
                .group_ref(&GroupId {
                    value: b"missing".to_vec(),
                })
                .expect_err("missing group")
                .code,
            StatusCode::NotFound
        );
    }

    #[test]
    fn parse_device_key_paths_and_identity_data_branch() {
        let bad = OpenMlsBackend::parse_device_key(&[1; 31]).expect_err("invalid len");
        assert_eq!(bad.code, StatusCode::InvalidArgument);

        let invalid64 = OpenMlsBackend::parse_device_key(&[7; 64]).expect_err("invalid keypair");
        assert_eq!(invalid64.code, StatusCode::InvalidArgument);

        let mut keypair = [0u8; 64];
        keypair[..32].copy_from_slice(&[11u8; 32]);
        let signing = SigningKey::from_bytes((&keypair[..32]).try_into().expect("seed slice"));
        keypair.copy_from_slice(&signing.to_keypair_bytes());
        let parsed = OpenMlsBackend::parse_device_key(&keypair).expect("valid 64 keypair");
        assert_eq!(parsed.0.len(), 32);
        assert_eq!(parsed.1.len(), 32);

        // Cover configure_client identity_data non-empty branch.
        let mut backend = OpenMlsBackend::default();
        backend
            .configure_client(&params_with_key(vec![9; 32], b"explicit-id".to_vec()))
            .expect("configure with explicit identity_data");
    }

    #[test]
    fn join_welcome_type_error_branch() {
        let mut backend = OpenMlsBackend::default();
        backend
            .configure_client(&params_with_key(vec![5; 32], vec![]))
            .expect("configure");
        let gid = GroupId {
            value: b"g-join-check".to_vec(),
        };
        backend.create_group(&gid).expect("create group");
        let app_msg = backend.encrypt(&gid, b"hello", b"aad").expect("encrypt");

        let err = backend
            .join_from_welcome(&app_msg)
            .expect_err("application message is not welcome");
        assert_eq!(err.code, StatusCode::InvalidArgument);
    }

    #[test]
    fn handle_incoming_welcome_success_path() {
        let mut alice = OpenMlsBackend::default();
        let mut bob = OpenMlsBackend::default();
        alice
            .configure_client(&params_with_key(vec![21; 32], vec![]))
            .expect("alice configure");
        bob.configure_client(&params_with_key(vec![22; 32], vec![]))
            .expect("bob configure");

        let bob_kp = bob
            .create_key_packages(1)
            .expect("bob key package")
            .remove(0);
        let gid = GroupId {
            value: b"g-welcome-incoming".to_vec(),
        };
        alice.create_group(&gid).expect("create group");
        let invite = alice.invite(&gid, &bob_kp).expect("invite");
        let welcome = invite.welcome_message.expect("welcome bytes");

        let out = bob
            .handle_incoming(&IncomingMessage {
                kind: IncomingMessageKind::Welcome,
                payload: welcome,
            })
            .expect("handle welcome");
        assert!(out.is_none());
    }

    #[test]
    fn remove_and_self_update_success_paths() {
        let mut alice = OpenMlsBackend::default();
        let mut bob = OpenMlsBackend::default();
        alice
            .configure_client(&params_with_key(vec![31; 32], vec![]))
            .expect("alice configure");
        bob.configure_client(&params_with_key(vec![32; 32], vec![]))
            .expect("bob configure");

        let kp = bob
            .create_key_packages(1)
            .expect("bob key packages")
            .remove(0);
        let gid = GroupId {
            value: b"g-remove-success".to_vec(),
        };
        alice.create_group(&gid).expect("create group");
        let invite = alice.invite(&gid, &kp).expect("invite");
        bob.join_from_welcome(&invite.welcome_message.expect("welcome"))
            .expect("join");

        let remove_res = bob.remove(&gid, Some(0)).expect("bob removes alice");
        assert!(!remove_res.commit_message.is_empty());

        bob.clear_pending_commit(&gid)
            .expect("clear pending before self update");
        let update_res = bob.self_update(&gid).expect("bob self update");
        assert!(!update_res.commit_message.is_empty());
    }

    #[test]
    fn handle_incoming_group_message_application_path() {
        let mut backend = OpenMlsBackend::default();
        backend
            .configure_client(&params_with_key(vec![41; 32], vec![]))
            .expect("configure");
        let gid = GroupId {
            value: b"g-handle-incoming-msg".to_vec(),
        };
        backend.create_group(&gid).expect("create group");

        let app = backend.encrypt(&gid, b"hello", b"aad").expect("encrypt");
        let err = backend
            .handle_incoming(&IncomingMessage {
                kind: IncomingMessageKind::GroupMessage,
                payload: app,
            })
            .expect_err("OpenMLS does not decrypt own app messages");
        assert_eq!(err.code, StatusCode::CryptoError);
        assert!(err.message.contains("Cannot decrypt own messages"));
    }
}
