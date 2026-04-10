use serde::{Deserialize, Serialize};

/// Контейнер сырых байтов, используемый во всём публичном API.
pub type Bytes = Vec<u8>;

/// Коды результата/статуса, общие для Rust и FFI API.
///
/// Значения стабильны (`repr(u32)`), так как используются и в C ABI.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StatusCode {
    /// Операция успешно завершена.
    Ok = 0,
    /// Входной аргумент некорректен или нарушает предусловия метода.
    InvalidArgument = 1,
    /// Запрошенная сущность отсутствует в локальном runtime/backend-состоянии.
    NotFound = 2,
    /// Сущность уже существует и не может быть создана/добавлена повторно.
    AlreadyExists = 3,
    /// Внутреннее состояние не удовлетворяет требованиям операции.
    InvalidState = 4,
    /// Криптографическая проверка не пройдена.
    VerificationFailed = 5,
    /// Ошибка криптографического backend (OpenMLS/подпись/сериализация).
    CryptoError = 6,
    /// Ошибка слоя хранения/сериализации/персистентности.
    StorageError = 7,
    /// Ошибка транспортного/интеграционного слоя.
    TransportError = 8,
    /// Валидная операция, но в текущей реализации не поддерживается.
    Unsupported = 9,
    /// Общая непредвиденная внутренняя ошибка.
    InternalError = 10,
}

/// Структурированная ошибка, возвращаемая методами service/backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Error {
    /// Машиночитаемый код статуса.
    pub code: StatusCode,
    /// Человекочитаемое описание для логов/диагностики.
    pub message: String,
}

impl Error {
    /// Создаёт новый [`Error`].
    pub fn new(code: StatusCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// Каноничный алиас результата, используемый в библиотеке.
pub type MlsResult<T> = Result<T, Error>;

/// Логическая идентичность клиента (пользователь + устройство).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClientId {
    /// Стабильный идентификатор аккаунта/пользователя.
    pub user_id: String,
    /// Идентификатор устройства в рамках пользователя.
    pub device_id: String,
}

/// Обёртка над бинарным идентификатором MLS-группы.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GroupId {
    /// Непрозрачные байты идентификатора группы.
    pub value: Bytes,
}

/// Метаданные привязки устройства, предоставляемые приложением.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeviceBinding {
    /// Идентичность, к которой относится эта привязка.
    pub client_id: ClientId,
    /// Сериализованный binding/credential-контейнер из верхних слоёв.
    pub serialized_binding: Bytes,
    /// Подпись уровня аккаунта, связывающая устройство с пользователем.
    pub account_signature: Bytes,
}

/// Пакет сериализованных MLS key package.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KeyPackageBundle {
    /// Список сериализованных key package.
    pub keypackages: Vec<Bytes>,
}

/// Публичный снимок состояния группы, возвращаемый сервисом.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GroupState {
    /// Идентификатор группы.
    pub group_id: GroupId,
    /// Текущая MLS-эпоха.
    pub epoch: u64,
    /// Считает ли локальный клиент членство активным.
    pub active: bool,
    /// Payload снимка из backend (сейчас это экспорт ratchet tree).
    pub serialized_state: Bytes,
}

/// Описание участника группы для операций листинга.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Member {
    /// Идентичность участника.
    pub client_id: ClientId,
    /// `true` для записи, соответствующей локальному клиенту.
    pub is_self: bool,
}

/// Тип входящего payload для [`crate::service::MessengerMls::handle_incoming`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum IncomingMessageKind {
    /// Welcome-сообщение для инициализации/вступления в группу.
    Welcome,
    /// Стандартное MLS group message (application/commit/proposal).
    #[default]
    GroupMessage,
}

/// Непрозрачный входящий транспортный контейнер.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IncomingMessage {
    /// Класс сообщения, определяющий разбор payload.
    pub kind: IncomingMessageKind,
    /// Байтовая форма сериализованного MLS wire-сообщения.
    pub payload: Bytes,
}

/// Категории событий, возвращаемых [`crate::service::MessengerMls::handle_incoming`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum EventKind {
    /// Событие отсутствует.
    #[default]
    None,
    /// Группа создана локально.
    GroupCreated,
    /// Выполнено вступление в группу из Welcome.
    GroupJoined,
    /// Участник добавлен.
    MemberAdded,
    /// Участник удалён.
    MemberRemoved,
    /// Получен plaintext application-сообщения.
    MessageReceived,
    /// Изменились метаданные/состояние группы.
    GroupStateChanged,
    /// Локальный клиент удалён из группы.
    SelfRemoved,
    /// Запас key package подходит к концу и требует пополнения.
    KeyPackagesNeeded,
}

/// Универсальный контейнер события, возвращаемый сервисом.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Event {
    /// Дискриминатор события.
    pub kind: EventKind,
    /// Идентификатор целевой группы.
    pub group_id: GroupId,
    /// Идентичность инициатора, если доступна.
    pub actor: ClientId,
    /// Идентичность объекта действия, если доступна.
    pub subject: ClientId,
    /// Расшифрованный application payload для [`EventKind::MessageReceived`].
    pub message_plaintext: Bytes,
}

/// Входной контракт для инициализации клиента.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CreateClientParams {
    /// Идентичность локального клиента.
    pub client_id: ClientId,
    /// Материал приватного ключа Ed25519 (32-байтный seed или 64-байтный keypair).
    pub device_signature_private_key: Bytes,
    /// Метаданные привязки устройства/аккаунта.
    pub binding: DeviceBinding,
    /// Пользовательский identity payload для credential; если пустой, backend берёт `user:device`.
    pub identity_data: Bytes,
}

/// Входной контракт для приглашения участника.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InviteRequest {
    /// Группа, которую нужно изменить.
    pub group_id: GroupId,
    /// Приглашаемый логический клиент.
    pub invited_client: ClientId,
    /// Сериализованный key package приглашаемого клиента.
    pub keypackage: Bytes,
}

/// Входной контракт для удаления участника.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RemoveRequest {
    /// Группа, которую нужно изменить.
    pub group_id: GroupId,
    /// Удаляемый участник.
    pub removed_client: ClientId,
}

/// Артефакты операции приглашения.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InviteResult {
    /// Байты commit-сообщения, которые нужно распространить в группе.
    pub commit_message: Bytes,
    /// Welcome-сообщение для приглашённого участника (если сформировано).
    pub welcome_message: Bytes,
    /// `true`, когда [`Self::welcome_message`] содержит валидные байты.
    pub has_welcome: bool,
    /// Обновлённый снимок группы после создания commit.
    pub group_state: GroupState,
}

/// Артефакты операции удаления.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RemoveResult {
    /// Байты commit-сообщения для распространения.
    pub commit_message: Bytes,
    /// Обновлённый снимок группы после удаления.
    pub group_state: GroupState,
}

/// Артефакты операции self-update.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SelfUpdateResult {
    /// Байты commit-сообщения для распространения.
    pub commit_message: Bytes,
    /// Обновлённый снимок группы после self-update.
    pub group_state: GroupState,
}
