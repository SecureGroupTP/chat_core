# chat_core + Flutter: Полный Гайд

Этот гайд показывает, как подключить `chat_core` к Flutter-приложению через `dart:ffi`, как работать с бинарями для разных платформ и как автоматизировать обновления из GitHub Releases.

## 1. Что лежит в релизах

В релизах репозитория есть:

- `chat_core-sdk-<tag>.tar.gz`:
  - Dart binding (`messenger_mls.dart`)
  - C header (`include/messenger_mls.h`)
  - API spec (`api/sdk_api.json`)
- платформенные бинарные артефакты (`chat_core-<platform>...`)

Важно: `Source code (zip/tar.gz)` от GitHub появляется сразу, а бинарные assets догружаются после завершения CI.

## 2. Поддержка Flutter по платформам

- Android: да (`dart:ffi`)
- iOS: да (`dart:ffi`), но обычно нужна отдельная сборка под Apple toolchain
- macOS: да (`dart:ffi`)
- Windows: да (`dart:ffi`)
- Linux: да (`dart:ffi`)
- Web: нет (`dart:ffi` в браузере не работает)

## 3. Базовая интеграция в Flutter

## 3.0. Готовый Flutter package

В репозитории есть готовый пакет:

- `flutter/sgtp_chat_core`

После пуша его можно подключать из Flutter-приложения напрямую:

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
      allow_static_linking: false
```

Пакет использует `hook/build.dart`: во время Flutter native build он берёт
подходящий бинарный asset из GitHub Release. Если динамического asset ещё нет,
а пакет подключён из локального checkout-а `chat_core`, hook может собрать
локальный `cdylib` через `cargo build --release --lib`.

## 3.1. Добавить FFI-зависимость

В `pubspec.yaml`:

```yaml
dependencies:
  ffi: ^2.1.2
```

## 3.2. Положить Dart binding

Из `chat_core-sdk-<tag>.tar.gz` возьми `tmp/dart/messenger_mls.dart` и положи, например, в:

- `lib/native/messenger_mls.dart`

Проверь, что имена динамических библиотек совпадают с тем, что ожидает binding:

- macOS: `libchat_core.dylib`
- Windows: `chat_core.dll`
- Linux/Android: `libchat_core.so`

## 3.3. Минимальный пример использования

```dart
import 'package:your_app/native/messenger_mls.dart';

void initMls() {
  final mls = MessengerMls.create();
  // дальше ваши вызовы API
  // ...
  mls.close();
}
```

## 4. Куда класть бинарники в Flutter-проекте

## 4.1. Android

Рекомендуемая раскладка:

- `android/app/src/main/jniLibs/arm64-v8a/libchat_core.so`
- `android/app/src/main/jniLibs/armeabi-v7a/libchat_core.so`
- `android/app/src/main/jniLibs/x86_64/libchat_core.so`

## 4.2. Windows

Положи `chat_core.dll` рядом с exe (обычно Flutter кладёт рядом в `build/windows/x64/runner/Release/`).
Если собираешь нативный код, может пригодиться также `.lib` (import library).

## 4.3. Linux

Положи `libchat_core.so` рядом с исполняемым файлом Flutter runner или в путь, который попадает в `LD_LIBRARY_PATH`.

## 4.4. macOS / iOS

Для Apple-платформ обычно делают отдельную упаковку:

- macOS: `.dylib` в app bundle
- iOS: `xcframework` (статическая/динамическая линковка через Xcode)

Если готовых Apple-артефактов в релизе нет, их собирают отдельно в CI или локально.

## 5. Что делать, если в релизе не тот формат (например `.a`, а нужен `.so`/`.dll`)

Для `dart:ffi` обычно нужны динамические библиотеки (`.so/.dylib/.dll`).
Если в релизе лежит только статика (`.a/.lib`), есть 2 пути:

- собрать `cdylib` и положить динамический бинарь в Flutter-пакет;
- или делать нативный wrapper/plugin и линковать статику на стороне платформенного кода.

Практически для Flutter почти всегда проще первый путь.

## 6. Автообновление из GitHub: можно ли

Коротко:

- Build-time автообновление: да, это нормальный путь.
- Runtime автообновление нативных библиотек внутри приложения: обычно плохая идея и часто конфликтует с правилами стора/безопасностью.

Рекомендуемый подход:

- фиксировать версию тега (`vX.Y.Z`) в репозитории приложения;
- в CI скачивать assets именно этого тега;
- обновлять версию контролируемо (PR/commit), а не "всегда latest".

## 6.1. Пример build-time скрипта скачивания assets

```bash
#!/usr/bin/env bash
set -euo pipefail

OWNER="SecureGroupTP"
REPO="chat_core"
TAG="${1:-v0.0.6}"

mkdir -p third_party/chat_core
cd third_party/chat_core

base="https://github.com/${OWNER}/${REPO}/releases/download/${TAG}"

# пример для Android arm64 + SDK bundle
curl -fL -o chat_core-android-arm64.a "${base}/chat_core-android-arm64.a"
curl -fL -o chat_core-sdk.tar.gz "${base}/chat_core-sdk-${TAG}.tar.gz"
tar -xzf chat_core-sdk.tar.gz
```

Дальше этот скрипт можно вызвать в CI перед `flutter build`.

## 7. Рекомендуемый прод-поток

- Использовать релизный тег как источник истины.
- Держать в проекте явную версию `chat_core`.
- Обновлять через PR: bump тега + прогон интеграционных тестов Flutter.
- Не скачивать и не подменять нативные бинарники "на лету" в рантайме.
