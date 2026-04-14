import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'rust/frb_api.dart';
import 'rust/frb_generated.dart';

final class MlsException implements Exception {
  MlsException(this.code, this.message);

  final int code;
  final String message;

  @override
  String toString() => 'MlsException(code: $code, message: $message)';
}

final class MessengerMls {
  MessengerMls._(this._bridge);

  final MessengerMlsBridge _bridge;
  bool _closed = false;

  static Future<void>? _initFuture;

  static Future<void> ensureInitialized() {
    final existing = _initFuture;
    if (existing != null) {
      return existing;
    }
    final future = ChatCoreBridge.init();
    _initFuture = future;
    return future;
  }

  static MessengerMls create() {
    if (!ChatCoreBridge.instance.initialized) {
      throw StateError(
        'sgtp_chat_core is not initialized. Call await MessengerMls.ensureInitialized() first.',
      );
    }
    return MessengerMls._(MessengerMlsBridge.create());
  }

  void close() {
    if (_closed) {
      return;
    }
    _closed = true;
    _bridge.dispose();
  }

  void _ensureOpen() {
    if (_closed) {
      throw StateError('MessengerMls is closed');
    }
  }

  void _throwIfStatus(ApiStatus status) {
    if (status.code == 0) {
      return;
    }
    throw MlsException(
      status.code,
      status.message.isEmpty ? 'operation failed' : status.message,
    );
  }

  Uint8List _takeBytes(ApiBytesResponse response) {
    if (response.code != 0) {
      throw MlsException(
        response.code,
        response.message.isEmpty ? 'operation failed' : response.message,
      );
    }
    return Uint8List.fromList(response.data);
  }

  Object? _takeJson(ApiJsonResponse response) {
    if (response.code != 0) {
      throw MlsException(
        response.code,
        response.message.isEmpty ? 'operation failed' : response.message,
      );
    }
    return response.json.isEmpty ? null : jsonDecode(response.json);
  }

  void createClientSync(Object? value) {
    _ensureOpen();
    _throwIfStatus(_bridge.createClient(inputJson: jsonEncode(value)));
  }

  Future<void> createClient(Object? value) =>
      Future<void>.microtask(() => createClientSync(value));

  void restoreClientSync(Uint8List data) {
    _ensureOpen();
    _throwIfStatus(_bridge.restoreClient(data: data));
  }

  Future<void> restoreClient(Uint8List data) =>
      Future<void>.microtask(() => restoreClientSync(data));

  Uint8List exportClientStateSync() {
    _ensureOpen();
    return _takeBytes(_bridge.exportClientState());
  }

  Future<Uint8List> exportClientState() =>
      Future<Uint8List>.microtask(exportClientStateSync);

  Object? getClientIdSync() {
    _ensureOpen();
    return _takeJson(_bridge.getClientId());
  }

  Future<Object?> getClientId() => Future<Object?>.microtask(getClientIdSync);

  Object? createKeyPackagesSync(int value) {
    _ensureOpen();
    return _takeJson(_bridge.createKeyPackages(count: value));
  }

  Future<Object?> createKeyPackages(int value) =>
      Future<Object?>.microtask(() => createKeyPackagesSync(value));

  void markKeyPackagesUploadedSync(Object? value) {
    _ensureOpen();
    _throwIfStatus(_bridge.markKeyPackagesUploaded(inputJson: jsonEncode(value)));
  }

  Future<void> markKeyPackagesUploaded(Object? value) =>
      Future<void>.microtask(() => markKeyPackagesUploadedSync(value));

  Object? createGroupSync(Object? value) {
    _ensureOpen();
    return _takeJson(_bridge.createGroup(inputJson: jsonEncode(value)));
  }

  Future<Object?> createGroup(Object? value) =>
      Future<Object?>.microtask(() => createGroupSync(value));

  Object? listGroupsSync() {
    _ensureOpen();
    return _takeJson(_bridge.listGroups());
  }

  Future<Object?> listGroups() => Future<Object?>.microtask(listGroupsSync);

  Object? getGroupStateSync(Object? value) {
    _ensureOpen();
    return _takeJson(_bridge.getGroupState(inputJson: jsonEncode(value)));
  }

  Future<Object?> getGroupState(Object? value) =>
      Future<Object?>.microtask(() => getGroupStateSync(value));

  Object? listMembersSync(Object? value) {
    _ensureOpen();
    return _takeJson(_bridge.listMembers(inputJson: jsonEncode(value)));
  }

  Future<Object?> listMembers(Object? value) =>
      Future<Object?>.microtask(() => listMembersSync(value));

  Object? inviteSync(Object? value) {
    _ensureOpen();
    return _takeJson(_bridge.invite(inputJson: jsonEncode(value)));
  }

  Future<Object?> invite(Object? value) =>
      Future<Object?>.microtask(() => inviteSync(value));

  Object? joinFromWelcomeSync(Uint8List data) {
    _ensureOpen();
    return _takeJson(_bridge.joinFromWelcome(welcomeMessage: data));
  }

  Future<Object?> joinFromWelcome(Uint8List data) =>
      Future<Object?>.microtask(() => joinFromWelcomeSync(data));

  Object? removeSync(Object? value) {
    _ensureOpen();
    return _takeJson(_bridge.remove(inputJson: jsonEncode(value)));
  }

  Future<Object?> remove(Object? value) =>
      Future<Object?>.microtask(() => removeSync(value));

  Object? selfUpdateSync(Object? value) {
    _ensureOpen();
    return _takeJson(_bridge.selfUpdate(inputJson: jsonEncode(value)));
  }

  Future<Object?> selfUpdate(Object? value) =>
      Future<Object?>.microtask(() => selfUpdateSync(value));

  Object? encryptMessageSync(Object? value) {
    _ensureOpen();
    return _takeJson(_bridge.encryptMessage(inputJson: jsonEncode(value)));
  }

  Future<Object?> encryptMessage(Object? value) =>
      Future<Object?>.microtask(() => encryptMessageSync(value));

  Object? handleIncomingSync(Object? value) {
    _ensureOpen();
    return _takeJson(_bridge.handleIncoming(inputJson: jsonEncode(value)));
  }

  Future<Object?> handleIncoming(Object? value) =>
      Future<Object?>.microtask(() => handleIncomingSync(value));

  Object? hasPendingCommitSync(Object? value) {
    _ensureOpen();
    return _takeJson(_bridge.hasPendingCommit(inputJson: jsonEncode(value)));
  }

  Future<Object?> hasPendingCommit(Object? value) =>
      Future<Object?>.microtask(() => hasPendingCommitSync(value));

  Object? mergePendingCommitSync(Object? value) {
    _ensureOpen();
    return _takeJson(_bridge.mergePendingCommit(inputJson: jsonEncode(value)));
  }

  Future<Object?> mergePendingCommit(Object? value) =>
      Future<Object?>.microtask(() => mergePendingCommitSync(value));

  void clearPendingCommitSync(Object? value) {
    _ensureOpen();
    _throwIfStatus(_bridge.clearPendingCommit(inputJson: jsonEncode(value)));
  }

  Future<void> clearPendingCommit(Object? value) =>
      Future<void>.microtask(() => clearPendingCommitSync(value));

  void dropGroupSync(Object? value) {
    _ensureOpen();
    _throwIfStatus(_bridge.dropGroup(inputJson: jsonEncode(value)));
  }

  Future<void> dropGroup(Object? value) =>
      Future<void>.microtask(() => dropGroupSync(value));
}