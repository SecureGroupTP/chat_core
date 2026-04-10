set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

android_targets := "aarch64-linux-android armv7-linux-androideabi x86_64-linux-android"
linux_targets := "x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu armv7-unknown-linux-gnueabihf"
windows_targets := "x86_64-pc-windows-gnu aarch64-pc-windows-gnullvm"

[doc("Show available tasks with documentation.")]
default:
    @just --list --unsorted

[doc("Run local preflight checks for required toolchain binaries.")]
preflight:
    command -v cargo >/dev/null
    command -v g++ >/dev/null
    command -v go >/dev/null
    command -v dart >/dev/null

[doc("Apply Rust formatting changes.")]
fmt:
    cargo fmt --all

[doc("Verify formatting without changing files (CI-safe).")]
fmt-check:
    cargo fmt --all -- --check

[doc("Run strict Rust lints and fail on warnings.")]
lint:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

[doc("Type-check and build all targets/features in dev profile.")]
check:
    cargo check --workspace --all-targets --all-features

[doc("Run all tests in workspace.")]
test:
    cargo test --workspace --all-targets --all-features

[doc("Run the default binary.")]
run:
    cargo run

[doc("Regenerate temporary SDK bindings and smoke fixtures.")]
sdk-gen:
    cargo run --quiet --bin sdk_codegen -- tmp

[doc("Build and validate generated SDKs (C++, Go, Dart).")]
sdk-verify: preflight sdk-gen
    cargo build --release --lib
    g++ -std=c++20 -Iinclude -Itmp/cpp -c tmp/cpp/smoke.cpp -o tmp/cpp/smoke.o
    (cd tmp/go && go test ./...)
    (cd tmp/dart && dart pub get && dart analyze messenger_mls.dart)

[doc("Build public docs and deny rustdoc warnings.")]
doc:
    RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps

[doc("Build docs including private items and deny rustdoc warnings.")]
doc-private:
    RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --document-private-items

[doc("Run the standard local quality gate.")]
validate: fmt-check lint check test

[doc("Run full CI pipeline including SDK verification.")]
ci: validate sdk-verify

[doc("Install Rust targets for Android/Windows/Linux builds.")]
targets-add:
    for t in {{android_targets}} {{linux_targets}} {{windows_targets}}; do rustup target add "$t"; done

[doc("Install cross-build tooling (cargo-zigbuild and cargo-ndk).")]
tooling-install:
    cargo install cargo-zigbuild cargo-ndk

[doc("Run preflight checks for cross-build dependencies.")]
cross-preflight:
    command -v rustup >/dev/null
    command -v zig >/dev/null
    command -v cargo-ndk >/dev/null
    cargo zigbuild --version >/dev/null
    if [[ -z "${ANDROID_NDK_HOME:-}" && -z "${ANDROID_NDK_ROOT:-}" ]]; then \
      echo "Set ANDROID_NDK_HOME or ANDROID_NDK_ROOT for Android builds."; \
      exit 1; \
    fi

[doc("Build Android static libraries for arm64-v8a, armeabi-v7a, x86_64.")]
build-android: cross-preflight
    cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 build --release --lib

[doc("Build Linux static libraries for amd64, arm64, arm32 (armv7).")]
build-linux:
    for t in {{linux_targets}}; do cargo zigbuild --release --target "$t" --lib; done

[doc("Build Windows static libraries for amd64 and arm64 via Zig toolchain.")]
build-windows:
    for t in x86_64-pc-windows-gnu aarch64-pc-windows-gnullvm; do cargo zigbuild --release --target "$t" --lib; done

[doc("Windows ARM32 is unavailable on stable Rust 1.93.1 (recipe returns an explicit error).")]
build-windows-arm32:
    @echo "Windows ARM32 target is unavailable on Rust 1.93.1 (no rust-std for thumbv7a-pc-windows-msvc)."
    @echo "Use a different supported target, or move to an experimental toolchain with build-std."
    @exit 1

[doc("Build all currently supported targets: Android, Linux, Windows (amd64/arm64).")]
build-all-targets: targets-add build-android build-linux build-windows

[doc("Remove generated local artifacts for a clean slate.")]
[confirm]
clean:
    cargo clean
    rm -f tmp/cpp/smoke.o
