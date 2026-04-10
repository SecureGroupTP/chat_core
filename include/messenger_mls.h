#pragma once

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct MessengerMlsHandle MessengerMlsHandle;

typedef struct MlsBuffer {
    uint8_t* ptr;
    size_t len;
} MlsBuffer;

typedef enum MlsStatusCode {
    MLS_OK = 0,
    MLS_INVALID_ARGUMENT = 1,
    MLS_NOT_FOUND = 2,
    MLS_ALREADY_EXISTS = 3,
    MLS_INVALID_STATE = 4,
    MLS_VERIFICATION_FAILED = 5,
    MLS_CRYPTO_ERROR = 6,
    MLS_STORAGE_ERROR = 7,
    MLS_TRANSPORT_ERROR = 8,
    MLS_UNSUPPORTED = 9,
    MLS_INTERNAL_ERROR = 10,
} MlsStatusCode;

MessengerMlsHandle* messenger_mls_new(void);
void messenger_mls_free(MessengerMlsHandle* handle);
void messenger_mls_buffer_free(MlsBuffer buf);

uint32_t messenger_mls_last_error(MessengerMlsHandle* handle, MlsBuffer* out);

uint32_t messenger_mls_create_client(MessengerMlsHandle* handle, const uint8_t* input_ptr, size_t input_len);
uint32_t messenger_mls_restore_client(MessengerMlsHandle* handle, const uint8_t* input_ptr, size_t input_len);
uint32_t messenger_mls_export_client_state(MessengerMlsHandle* handle, MlsBuffer* out);
uint32_t messenger_mls_get_client_id(MessengerMlsHandle* handle, MlsBuffer* out);

uint32_t messenger_mls_create_key_packages(MessengerMlsHandle* handle, uint32_t count, MlsBuffer* out);
uint32_t messenger_mls_mark_key_packages_uploaded(MessengerMlsHandle* handle, const uint8_t* input_ptr, size_t input_len);

uint32_t messenger_mls_create_group(MessengerMlsHandle* handle, const uint8_t* input_ptr, size_t input_len, MlsBuffer* out);
uint32_t messenger_mls_list_groups(MessengerMlsHandle* handle, MlsBuffer* out);
uint32_t messenger_mls_get_group_state(MessengerMlsHandle* handle, const uint8_t* input_ptr, size_t input_len, MlsBuffer* out);
uint32_t messenger_mls_list_members(MessengerMlsHandle* handle, const uint8_t* input_ptr, size_t input_len, MlsBuffer* out);

uint32_t messenger_mls_invite(MessengerMlsHandle* handle, const uint8_t* input_ptr, size_t input_len, MlsBuffer* out);
uint32_t messenger_mls_join_from_welcome(MessengerMlsHandle* handle, const uint8_t* welcome_ptr, size_t welcome_len, MlsBuffer* out);
uint32_t messenger_mls_remove(MessengerMlsHandle* handle, const uint8_t* input_ptr, size_t input_len, MlsBuffer* out);
uint32_t messenger_mls_self_update(MessengerMlsHandle* handle, const uint8_t* input_ptr, size_t input_len, MlsBuffer* out);

uint32_t messenger_mls_encrypt_message(MessengerMlsHandle* handle, const uint8_t* input_ptr, size_t input_len, MlsBuffer* out);
uint32_t messenger_mls_handle_incoming(MessengerMlsHandle* handle, const uint8_t* input_ptr, size_t input_len, MlsBuffer* out);

uint32_t messenger_mls_has_pending_commit(MessengerMlsHandle* handle, const uint8_t* input_ptr, size_t input_len, MlsBuffer* out);
uint32_t messenger_mls_clear_pending_commit(MessengerMlsHandle* handle, const uint8_t* input_ptr, size_t input_len);
uint32_t messenger_mls_drop_group(MessengerMlsHandle* handle, const uint8_t* input_ptr, size_t input_len);

#ifdef __cplusplus
}
#endif
