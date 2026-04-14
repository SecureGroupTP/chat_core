import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated_web.dart';

import 'chat_core_release_info.dart';

const _kWasmBindgenName = 'wasm_bindgen';

Future<ExternalLibrary?> createChatCoreWebExternalLibrary() async {
  await initializeWasmModule(
    root: kDefaultChatCoreWebModuleRoot,
    wasmBindgenName: _kWasmBindgenName,
  );
  return const ExternalLibrary(
    debugInfo: 'remote chat_core web module',
    wasmBindgenName: _kWasmBindgenName,
  );
}
