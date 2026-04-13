import 'dart:typed_data';

final class MlsException implements Exception {
  MlsException(this.code, this.message);

  final int code;
  final String message;

  @override
  String toString() => 'MlsException(code: $code, message: $message)';
}

final class MessengerMls {
  MessengerMls._();

  static MessengerMls create({Object? library}) {
    throw UnsupportedError('chat_core FFI bindings are not available on web.');
  }

  void close() {}

  Future<void> createClient(Object? value) => _unsupported();
  void createClientSync(Object? value) => _unsupportedSync();

  Future<void> restoreClient(Uint8List data) => _unsupported();
  void restoreClientSync(Uint8List data) => _unsupportedSync();

  Future<Uint8List> exportClientState() => _unsupported();
  Uint8List exportClientStateSync() => _unsupportedSync();

  Future<Object?> getClientId() => _unsupported();
  Object? getClientIdSync() => _unsupportedSync();

  Future<Object?> createKeyPackages(int value) => _unsupported();
  Object? createKeyPackagesSync(int value) => _unsupportedSync();

  Future<void> markKeyPackagesUploaded(Object? value) => _unsupported();
  void markKeyPackagesUploadedSync(Object? value) => _unsupportedSync();

  Future<Object?> createGroup(Object? value) => _unsupported();
  Object? createGroupSync(Object? value) => _unsupportedSync();

  Future<Object?> listGroups() => _unsupported();
  Object? listGroupsSync() => _unsupportedSync();

  Future<Object?> getGroupState(Object? value) => _unsupported();
  Object? getGroupStateSync(Object? value) => _unsupportedSync();

  Future<Object?> listMembers(Object? value) => _unsupported();
  Object? listMembersSync(Object? value) => _unsupportedSync();

  Future<Object?> invite(Object? value) => _unsupported();
  Object? inviteSync(Object? value) => _unsupportedSync();

  Future<Object?> joinFromWelcome(Uint8List data) => _unsupported();
  Object? joinFromWelcomeSync(Uint8List data) => _unsupportedSync();

  Future<Object?> remove(Object? value) => _unsupported();
  Object? removeSync(Object? value) => _unsupportedSync();

  Future<Object?> selfUpdate(Object? value) => _unsupported();
  Object? selfUpdateSync(Object? value) => _unsupportedSync();

  Future<Object?> encryptMessage(Object? value) => _unsupported();
  Object? encryptMessageSync(Object? value) => _unsupportedSync();

  Future<Object?> handleIncoming(Object? value) => _unsupported();
  Object? handleIncomingSync(Object? value) => _unsupportedSync();

  Future<Object?> hasPendingCommit(Object? value) => _unsupported();
  Object? hasPendingCommitSync(Object? value) => _unsupportedSync();

  Future<Object?> mergePendingCommit(Object? value) => _unsupported();
  Object? mergePendingCommitSync(Object? value) => _unsupportedSync();

  Future<void> clearPendingCommit(Object? value) => _unsupported();
  void clearPendingCommitSync(Object? value) => _unsupportedSync();

  Future<void> dropGroup(Object? value) => _unsupported();
  void dropGroupSync(Object? value) => _unsupportedSync();
}

Never _unsupportedSync() {
  throw UnsupportedError('chat_core FFI bindings are not available on web.');
}

Future<T> _unsupported<T>() async => _unsupportedSync();
