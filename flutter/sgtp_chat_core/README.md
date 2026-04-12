# sgtp_chat_core

Flutter package for `chat_core` FFI bindings.

The package bundles native libraries from GitHub Releases during Flutter native
asset builds. Configure the release in the app `pubspec.yaml`:

```yaml
dependencies:
  sgtp_chat_core:
    git:
      url: https://github.com/SecureGroupTP/chat_core.git
      ref: master
      path: flutter/sgtp_chat_core

hooks:
  user_defines:
    sgtp_chat_core:
      release_tag: v0.0.9
      github_owner: SecureGroupTP
      github_repo: chat_core
      # Dynamic libraries are preferred. Static archives can be enabled when
      # your Flutter SDK/toolchain can link them for the target.
      allow_static_linking: false
```

For local development inside this monorepo, use:

```yaml
dependencies:
  sgtp_chat_core:
    path: ../chat_core/flutter/sgtp_chat_core
```
