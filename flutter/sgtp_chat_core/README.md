# sgtp_chat_core

Flutter package for `chat_core` bindings generated via `flutter_rust_bridge`.

`export_client_state` / `restore_client` roundtrip full client runtime for the
current OpenMLS backend, including persisted MLS groups and provider storage.

The package supports a mixed delivery flow:

- Desktop/mobile native libraries are resolved from GitHub Releases by default.
- Local Rust builds can be forced for path/monorepo development with
  `hooks.user_defines.sgtp_chat_core.prefer_local_build: true`.
- Web loads the generated `wasm/js` bundle from CI-published hosting.
  By default it uses:
  `https://securegrouptp.github.io/chat_core/releases/latest/pkg/chat_core`
  and can be overridden with the Dart define
  `SGTP_CHAT_CORE_WEB_MODULE_ROOT`.

Example git dependency:

```yaml
dependencies:
  sgtp_chat_core:
    git:
      url: https://github.com/SecureGroupTP/chat_core.git
      ref: v0.0.11
      path: flutter/sgtp_chat_core

hooks:
  user_defines:
    sgtp_chat_core:
      release_tag: v0.0.11
      github_owner: SecureGroupTP
      github_repo: chat_core
      allow_static_linking: false
```

Example local development override:

```yaml
dependencies:
  sgtp_chat_core:
    path: ../chat_core/flutter/sgtp_chat_core

hooks:
  user_defines:
    sgtp_chat_core:
      prefer_local_build: true
```
