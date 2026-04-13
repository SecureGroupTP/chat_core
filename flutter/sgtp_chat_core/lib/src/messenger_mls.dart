// ignore_for_file: unused_element

import 'dart:async';
import 'dart:convert';
import 'dart:ffi' as ffi;
import 'dart:io';
import 'dart:typed_data';

import 'package:ffi/ffi.dart';

final class MlsException implements Exception {
  MlsException(this.code, this.message);
  final int code;
  final String message;

  @override
  String toString() => 'MlsException(code: $code, message: $message)';
}

final class MlsBuffer extends ffi.Struct {
  external ffi.Pointer<ffi.Uint8> ptr;

  @ffi.Size()
  external int len;
}

typedef _NewNative = ffi.Pointer<ffi.Void> Function();
typedef _NewDart = ffi.Pointer<ffi.Void> Function();
typedef _FreeNative = ffi.Void Function(ffi.Pointer<ffi.Void>);
typedef _FreeDart = void Function(ffi.Pointer<ffi.Void>);
typedef _LastErrorNative =
    ffi.Uint32 Function(ffi.Pointer<ffi.Void>, ffi.Pointer<MlsBuffer>);
typedef _LastErrorDart =
    int Function(ffi.Pointer<ffi.Void>, ffi.Pointer<MlsBuffer>);
typedef _BufferFreeNative = ffi.Void Function(MlsBuffer);
typedef _BufferFreeDart = void Function(MlsBuffer);
typedef _NoInputVoidNative = ffi.Uint32 Function(ffi.Pointer<ffi.Void>);
typedef _NoInputVoidDart = int Function(ffi.Pointer<ffi.Void>);
typedef _NoInputOutNative =
    ffi.Uint32 Function(ffi.Pointer<ffi.Void>, ffi.Pointer<MlsBuffer>);
typedef _NoInputOutDart =
    int Function(ffi.Pointer<ffi.Void>, ffi.Pointer<MlsBuffer>);
typedef _InputVoidNative =
    ffi.Uint32 Function(
      ffi.Pointer<ffi.Void>,
      ffi.Pointer<ffi.Uint8>,
      ffi.Size,
    );
typedef _InputVoidDart =
    int Function(ffi.Pointer<ffi.Void>, ffi.Pointer<ffi.Uint8>, int);
typedef _InputOutNative =
    ffi.Uint32 Function(
      ffi.Pointer<ffi.Void>,
      ffi.Pointer<ffi.Uint8>,
      ffi.Size,
      ffi.Pointer<MlsBuffer>,
    );
typedef _InputOutDart =
    int Function(
      ffi.Pointer<ffi.Void>,
      ffi.Pointer<ffi.Uint8>,
      int,
      ffi.Pointer<MlsBuffer>,
    );
typedef _U32VoidNative = ffi.Uint32 Function(ffi.Pointer<ffi.Void>, ffi.Uint32);
typedef _U32VoidDart = int Function(ffi.Pointer<ffi.Void>, int);
typedef _U32OutNative =
    ffi.Uint32 Function(
      ffi.Pointer<ffi.Void>,
      ffi.Uint32,
      ffi.Pointer<MlsBuffer>,
    );
typedef _U32OutDart =
    int Function(ffi.Pointer<ffi.Void>, int, ffi.Pointer<MlsBuffer>);

final class MessengerMls implements ffi.Finalizable {
  MessengerMls._(this._lib, this._handle) {
    _finalizer.attach(this, _handle);
  }

  final ffi.DynamicLibrary _lib;
  ffi.Pointer<ffi.Void> _handle;

  static final _finalizer = ffi.NativeFinalizer(
    _resolveLib()
        .lookup<ffi.NativeFunction<_FreeNative>>('messenger_mls_free')
        .cast(),
  );

  static ffi.DynamicLibrary _resolveLib() {
    if (Platform.isIOS) return ffi.DynamicLibrary.process();
    if (Platform.isMacOS) return _openBundledOrProcess('libchat_core.dylib');
    if (Platform.isWindows) return ffi.DynamicLibrary.open('chat_core.dll');
    return _openBundledOrProcess('libchat_core.so');
  }

  static ffi.DynamicLibrary _openBundledOrProcess(String name) {
    try {
      return ffi.DynamicLibrary.open(name);
    } on ArgumentError {
      return ffi.DynamicLibrary.process();
    } on OSError {
      return ffi.DynamicLibrary.process();
    }
  }

  static MessengerMls create({ffi.DynamicLibrary? library}) {
    final lib = library ?? _resolveLib();
    final newFn = lib.lookupFunction<_NewNative, _NewDart>('messenger_mls_new');
    final handle = newFn();
    if (handle == ffi.nullptr) {
      throw StateError('messenger_mls_new returned nullptr');
    }
    return MessengerMls._(lib, handle);
  }

  void close() {
    if (_handle == ffi.nullptr) return;
    final freeFn = _lib.lookupFunction<_FreeNative, _FreeDart>(
      'messenger_mls_free',
    );
    _finalizer.detach(this);
    freeFn(_handle);
    _handle = ffi.nullptr;
  }

  String _lastError() {
    final fn = _lib.lookupFunction<_LastErrorNative, _LastErrorDart>(
      'messenger_mls_last_error',
    );
    final freeBuf = _lib.lookupFunction<_BufferFreeNative, _BufferFreeDart>(
      'messenger_mls_buffer_free',
    );
    final outBuf = calloc<MlsBuffer>();
    try {
      fn(_handle, outBuf);
      final ptr = outBuf.ref.ptr;
      final len = outBuf.ref.len;
      if (ptr == ffi.nullptr || len == 0) return '';
      final bytes = ptr.asTypedList(len);
      return utf8.decode(bytes, allowMalformed: true);
    } finally {
      freeBuf(outBuf.ref);
      calloc.free(outBuf);
    }
  }

  void _throwIfError(int code) {
    if (code == 0) return;
    final msg = _lastError();
    throw MlsException(code, msg.isEmpty ? 'operation failed' : msg);
  }

  Uint8List _takeBytes(MlsBuffer buf) {
    final freeBuf = _lib.lookupFunction<_BufferFreeNative, _BufferFreeDart>(
      'messenger_mls_buffer_free',
    );
    try {
      if (buf.ptr == ffi.nullptr || buf.len == 0) return Uint8List(0);
      return Uint8List.fromList(buf.ptr.asTypedList(buf.len));
    } finally {
      freeBuf(buf);
    }
  }

  String _takeJson(MlsBuffer buf) =>
      utf8.decode(_takeBytes(buf), allowMalformed: true);

  int _callNoInputVoid(String symbol) {
    final fn = _lib.lookupFunction<_NoInputVoidNative, _NoInputVoidDart>(
      symbol,
    );
    return fn(_handle);
  }

  int _callNoInputOut(String symbol, ffi.Pointer<MlsBuffer> outBuf) {
    final fn = _lib.lookupFunction<_NoInputOutNative, _NoInputOutDart>(symbol);
    return fn(_handle, outBuf);
  }

  int _callInputVoid(String symbol, Uint8List data) {
    final fn = _lib.lookupFunction<_InputVoidNative, _InputVoidDart>(symbol);
    if (data.isEmpty) return fn(_handle, ffi.nullptr, 0);
    final ptr = calloc<ffi.Uint8>(data.length);
    try {
      ptr.asTypedList(data.length).setAll(0, data);
      return fn(_handle, ptr, data.length);
    } finally {
      calloc.free(ptr);
    }
  }

  int _callInputOut(
    String symbol,
    Uint8List data,
    ffi.Pointer<MlsBuffer> outBuf,
  ) {
    final fn = _lib.lookupFunction<_InputOutNative, _InputOutDart>(symbol);
    if (data.isEmpty) return fn(_handle, ffi.nullptr, 0, outBuf);
    final ptr = calloc<ffi.Uint8>(data.length);
    try {
      ptr.asTypedList(data.length).setAll(0, data);
      return fn(_handle, ptr, data.length, outBuf);
    } finally {
      calloc.free(ptr);
    }
  }

  int _callU32Void(String symbol, int value) {
    final fn = _lib.lookupFunction<_U32VoidNative, _U32VoidDart>(symbol);
    return fn(_handle, value);
  }

  int _callU32Out(String symbol, int value, ffi.Pointer<MlsBuffer> outBuf) {
    final fn = _lib.lookupFunction<_U32OutNative, _U32OutDart>(symbol);
    return fn(_handle, value, outBuf);
  }

  /// Create and configure a local MLS client from JSON-serialized CreateClientParams.
  void createClientSync(Object? value) {
    final data = Uint8List.fromList(utf8.encode(jsonEncode(value)));
    _throwIfError(_callInputVoid('messenger_mls_create_client', data));
  }

  Future<void> createClient(Object? value) =>
      Future<void>.microtask(() => createClientSync(value));

  /// Restore previously exported client state bytes.
  void restoreClientSync(Uint8List data) {
    _throwIfError(_callInputVoid('messenger_mls_restore_client', data));
  }

  Future<void> restoreClient(Uint8List data) =>
      Future<void>.microtask(() => restoreClientSync(data));

  /// Export client state as opaque bytes for persistence.
  Uint8List exportClientStateSync() {
    final outBuf = calloc<MlsBuffer>();
    try {
      _throwIfError(
        _callNoInputOut('messenger_mls_export_client_state', outBuf),
      );
      return _takeBytes(outBuf.ref);
    } finally {
      calloc.free(outBuf);
    }
  }

  Future<Uint8List> exportClientState() =>
      Future<Uint8List>.microtask(exportClientStateSync);

  /// Return current client id JSON.
  Object? getClientIdSync() {
    final outBuf = calloc<MlsBuffer>();
    try {
      _throwIfError(_callNoInputOut('messenger_mls_get_client_id', outBuf));
      return jsonDecode(_takeJson(outBuf.ref));
    } finally {
      calloc.free(outBuf);
    }
  }

  Future<Object?> getClientId() => Future<Object?>.microtask(getClientIdSync);

  /// Generate key packages and return JSON KeyPackageBundle.
  Object? createKeyPackagesSync(int value) {
    final outBuf = calloc<MlsBuffer>();
    try {
      _throwIfError(
        _callU32Out('messenger_mls_create_key_packages', value, outBuf),
      );
      return jsonDecode(_takeJson(outBuf.ref));
    } finally {
      calloc.free(outBuf);
    }
  }

  Future<Object?> createKeyPackages(int value) =>
      Future<Object?>.microtask(() => createKeyPackagesSync(value));

  /// Mark generated key packages as uploaded using JSON KeyPackageBundle.
  void markKeyPackagesUploadedSync(Object? value) {
    final data = Uint8List.fromList(utf8.encode(jsonEncode(value)));
    _throwIfError(
      _callInputVoid('messenger_mls_mark_key_packages_uploaded', data),
    );
  }

  Future<void> markKeyPackagesUploaded(Object? value) =>
      Future<void>.microtask(() => markKeyPackagesUploadedSync(value));

  /// Create a group from JSON GroupId and return JSON GroupState.
  Object? createGroupSync(Object? value) {
    final data = Uint8List.fromList(utf8.encode(jsonEncode(value)));
    final outBuf = calloc<MlsBuffer>();
    try {
      _throwIfError(_callInputOut('messenger_mls_create_group', data, outBuf));
      return jsonDecode(_takeJson(outBuf.ref));
    } finally {
      calloc.free(outBuf);
    }
  }

  Future<Object?> createGroup(Object? value) =>
      Future<Object?>.microtask(() => createGroupSync(value));

  /// Return JSON array of GroupState objects.
  Object? listGroupsSync() {
    final outBuf = calloc<MlsBuffer>();
    try {
      _throwIfError(_callNoInputOut('messenger_mls_list_groups', outBuf));
      return jsonDecode(_takeJson(outBuf.ref));
    } finally {
      calloc.free(outBuf);
    }
  }

  Future<Object?> listGroups() => Future<Object?>.microtask(listGroupsSync);

  /// Return JSON GroupState for provided JSON GroupId.
  Object? getGroupStateSync(Object? value) {
    final data = Uint8List.fromList(utf8.encode(jsonEncode(value)));
    final outBuf = calloc<MlsBuffer>();
    try {
      _throwIfError(
        _callInputOut('messenger_mls_get_group_state', data, outBuf),
      );
      return jsonDecode(_takeJson(outBuf.ref));
    } finally {
      calloc.free(outBuf);
    }
  }

  Future<Object?> getGroupState(Object? value) =>
      Future<Object?>.microtask(() => getGroupStateSync(value));

  /// Return JSON array of group members for JSON GroupId.
  Object? listMembersSync(Object? value) {
    final data = Uint8List.fromList(utf8.encode(jsonEncode(value)));
    final outBuf = calloc<MlsBuffer>();
    try {
      _throwIfError(_callInputOut('messenger_mls_list_members', data, outBuf));
      return jsonDecode(_takeJson(outBuf.ref));
    } finally {
      calloc.free(outBuf);
    }
  }

  Future<Object?> listMembers(Object? value) =>
      Future<Object?>.microtask(() => listMembersSync(value));

  /// Invite a member using JSON InviteRequest and return JSON InviteResult.
  Object? inviteSync(Object? value) {
    final data = Uint8List.fromList(utf8.encode(jsonEncode(value)));
    final outBuf = calloc<MlsBuffer>();
    try {
      _throwIfError(_callInputOut('messenger_mls_invite', data, outBuf));
      return jsonDecode(_takeJson(outBuf.ref));
    } finally {
      calloc.free(outBuf);
    }
  }

  Future<Object?> invite(Object? value) =>
      Future<Object?>.microtask(() => inviteSync(value));

  /// Join from raw welcome message bytes and return JSON GroupState.
  Object? joinFromWelcomeSync(Uint8List data) {
    final outBuf = calloc<MlsBuffer>();
    try {
      _throwIfError(
        _callInputOut('messenger_mls_join_from_welcome', data, outBuf),
      );
      return jsonDecode(_takeJson(outBuf.ref));
    } finally {
      calloc.free(outBuf);
    }
  }

  Future<Object?> joinFromWelcome(Uint8List data) =>
      Future<Object?>.microtask(() => joinFromWelcomeSync(data));

  /// Remove a member using JSON RemoveRequest and return JSON RemoveResult.
  Object? removeSync(Object? value) {
    final data = Uint8List.fromList(utf8.encode(jsonEncode(value)));
    final outBuf = calloc<MlsBuffer>();
    try {
      _throwIfError(_callInputOut('messenger_mls_remove', data, outBuf));
      return jsonDecode(_takeJson(outBuf.ref));
    } finally {
      calloc.free(outBuf);
    }
  }

  Future<Object?> remove(Object? value) =>
      Future<Object?>.microtask(() => removeSync(value));

  /// Run self-update for JSON GroupId and return JSON SelfUpdateResult.
  Object? selfUpdateSync(Object? value) {
    final data = Uint8List.fromList(utf8.encode(jsonEncode(value)));
    final outBuf = calloc<MlsBuffer>();
    try {
      _throwIfError(_callInputOut('messenger_mls_self_update', data, outBuf));
      return jsonDecode(_takeJson(outBuf.ref));
    } finally {
      calloc.free(outBuf);
    }
  }

  Future<Object?> selfUpdate(Object? value) =>
      Future<Object?>.microtask(() => selfUpdateSync(value));

  /// Encrypt application message from JSON request and return JSON-encoded ciphertext bytes.
  Object? encryptMessageSync(Object? value) {
    final data = Uint8List.fromList(utf8.encode(jsonEncode(value)));
    final outBuf = calloc<MlsBuffer>();
    try {
      _throwIfError(
        _callInputOut('messenger_mls_encrypt_message', data, outBuf),
      );
      return jsonDecode(_takeJson(outBuf.ref));
    } finally {
      calloc.free(outBuf);
    }
  }

  Future<Object?> encryptMessage(Object? value) =>
      Future<Object?>.microtask(() => encryptMessageSync(value));

  /// Handle incoming message JSON and return JSON Event.
  Object? handleIncomingSync(Object? value) {
    final data = Uint8List.fromList(utf8.encode(jsonEncode(value)));
    final outBuf = calloc<MlsBuffer>();
    try {
      _throwIfError(
        _callInputOut('messenger_mls_handle_incoming', data, outBuf),
      );
      return jsonDecode(_takeJson(outBuf.ref));
    } finally {
      calloc.free(outBuf);
    }
  }

  Future<Object?> handleIncoming(Object? value) =>
      Future<Object?>.microtask(() => handleIncomingSync(value));

  /// Return JSON boolean pending-commit state for JSON GroupId.
  Object? hasPendingCommitSync(Object? value) {
    final data = Uint8List.fromList(utf8.encode(jsonEncode(value)));
    final outBuf = calloc<MlsBuffer>();
    try {
      _throwIfError(
        _callInputOut('messenger_mls_has_pending_commit', data, outBuf),
      );
      return jsonDecode(_takeJson(outBuf.ref));
    } finally {
      calloc.free(outBuf);
    }
  }

  Future<Object?> hasPendingCommit(Object? value) =>
      Future<Object?>.microtask(() => hasPendingCommitSync(value));

  /// Merge pending commit for JSON GroupId and return JSON GroupState.
  Object? mergePendingCommitSync(Object? value) {
    final data = Uint8List.fromList(utf8.encode(jsonEncode(value)));
    final outBuf = calloc<MlsBuffer>();
    try {
      _throwIfError(
        _callInputOut('messenger_mls_merge_pending_commit', data, outBuf),
      );
      return jsonDecode(_takeJson(outBuf.ref));
    } finally {
      calloc.free(outBuf);
    }
  }

  Future<Object?> mergePendingCommit(Object? value) =>
      Future<Object?>.microtask(() => mergePendingCommitSync(value));

  /// Clear pending commit for JSON GroupId.
  void clearPendingCommitSync(Object? value) {
    final data = Uint8List.fromList(utf8.encode(jsonEncode(value)));
    _throwIfError(_callInputVoid('messenger_mls_clear_pending_commit', data));
  }

  Future<void> clearPendingCommit(Object? value) =>
      Future<void>.microtask(() => clearPendingCommitSync(value));

  /// Drop local group state for JSON GroupId.
  void dropGroupSync(Object? value) {
    final data = Uint8List.fromList(utf8.encode(jsonEncode(value)));
    _throwIfError(_callInputVoid('messenger_mls_drop_group', data));
  }

  Future<void> dropGroup(Object? value) =>
      Future<void>.microtask(() => dropGroupSync(value));
}
