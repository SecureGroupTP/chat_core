# chat_core

Rust-библиотека с C ABI фасадом для MLS-клиента мессенджера.

## Зависимости

Для локальной разработки и запуска команд из `justfile` нужны:

- `just` `1.40.0` (текущая используемая версия)
- `rustc`/`cargo` `1.93.1` (зафиксировано в `rust-toolchain.toml`)
- компоненты Rust toolchain: `rustfmt`, `clippy`
- `g++` с поддержкой C++20 (проверено на `g++ 13.3.0`)
- `go` (проверено на `go1.23.0`)
- `dart` (проверено на `3.11.4`)

Для кросс-компиляции (Android/Windows/Linux multi-arch) дополнительно нужны:

- `zig` (используется `cargo-zigbuild` для Linux/Windows кросс-линковки)
- `cargo-zigbuild`
- `cargo-ndk`
- Android NDK (`ANDROID_NDK_HOME` или `ANDROID_NDK_ROOT` должен быть установлен)

Быстрая проверка версий:

```bash
just --version
rustc --version
cargo --version
g++ --version
go version
dart --version
```

Установка cross-tooling:

```bash
just tooling-install
```

## Документация

- Сгенерировать публичную HTML-документацию:
  - `just doc`
- Сгенерировать расширенную документацию, включая private items:
  - `just doc-private`
- Основной результат:
  - `target/doc/chat_core/index.html`
- Дополнительный гайд по API:
  - `docs/API_USAGE.md`
- Журнал итераций документирования:
  - `docs/DOCUMENTATION_ITERATIONS.md`

## Генерация SDK (C++ / Go / Dart)

В репозитории есть `sdk_codegen`, который генерирует обёртки в `tmp/` на основе `api/sdk_api.json`.

- Только генерация обёрток:
  - `just sdk-gen`
- Генерация и проверка smoke-сценариев:
  - `just sdk-verify`

`just sdk-verify` запускает:

1. `cargo run --bin sdk_codegen -- tmp`
2. `cargo build --release --lib`
3. Компиляцию C++ smoke-теста (`tmp/cpp/smoke.cpp`)
4. Go-тесты (`tmp/go`)
5. Dart-анализ (`tmp/dart`)

## Покрытие тестов (Coverage)

- Показать текущее покрытие в терминале (без файлов отчёта):
  - `just cov`

## Кросс-компиляция

Поддерживаемые целевые платформы:

- Android: `arm64` (`aarch64-linux-android`), `arm32` (`armv7-linux-androideabi`), `amd64` (`x86_64-linux-android`)
- Windows: `amd64` (`x86_64-pc-windows-gnu`), `arm64` (`aarch64-pc-windows-gnullvm`)
- Linux: `amd64` (`x86_64-unknown-linux-gnu`), `arm64` (`aarch64-unknown-linux-gnu`), `arm32` (`armv7-unknown-linux-gnueabihf`)

Команды:

- Добавить все Rust targets:
  - `just targets-add`
- Android (все 3 архитектуры):
  - `just build-android`
- Linux (все 3 архитектуры):
  - `just build-linux`
- Windows `amd64/arm64`:
  - `just build-windows`
- Полный прогон по всем таргетам:
  - `just build-all-targets`

Примечание по Windows ARM32:

- В Rust `1.93.1` target `thumbv7a-pc-windows-msvc` недоступен через `rustup` (нет `rust-std`), поэтому стабильная сборка ARM32 Windows сейчас отключена.

## CI/CD

Workflow GitHub Actions:

- Файл: `.github/workflows/sdk_codegen.yml`
- Триггеры: `pull_request`, `push` в `master`, `push` тега `v*`, `workflow_dispatch`
- Команда job: `just sdk-verify`
- Runner: `ubuntu-24.04`
- Пинning версий toolchain: Rust `1.93.1`, Go `1.23.0`, Dart `3.11.4`, just `1.40.0`

Кросс-платформенная сборка артефактов:

- Файл: `.github/workflows/cross_targets.yml`
- Платформы: Android (arm64/arm32/amd64), Windows (amd64/arm64), Linux (amd64/arm64/arm32)
- Runner-ы: `ubuntu-24.04`
- Пинning версий tooling: Rust `1.93.1`, just `1.40.0`, Zig `0.14.0`, cargo-zigbuild `0.22.1`, cargo-ndk `4.1.2`, Android NDK `26.3.11579264`

## Текущее состояние

- C ABI фасад (`extern "C"`) с opaque handle и буферами `uint8_t* + len`
- Транспорт JSON DTO через FFI-границу
- Основные операции на OpenMLS:
  - конфигурация идентичности клиента из постоянного Ed25519 signing key
  - генерация key package
  - создание группы
  - приглашение (Add/Commit + Welcome)
  - join из Welcome
  - self update
  - удаление участника через маппинг leaf-индекса
  - шифрование сообщения (MLS application message)
  - обработка входящих group message (включая merge staged commit)
  - проверка/очистка pending commit и удаление группы
- локальная persisted-модель для идентичности, снимков групп, инвентаря key package и очереди входящих

## Примечание по персистентности

Экспортируемый `serialized_client_state` сейчас сохраняет состояние уровня приложения и снимки.
Полная персистентность OpenMLS provider storage между перезапусками процесса пока требует
отдельной интеграции persistent storage backend.
На текущий момент `restore_client` восстанавливает только идентичность/инвентарь key package
и возвращает `Unsupported`, если в снимке присутствуют группы, чтобы избежать частичного
и небезопасного восстановления групп.
