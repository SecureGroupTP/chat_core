use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir(prefix: &str) -> PathBuf {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("{prefix}-{ts}"));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

#[test]
fn sdk_codegen_generates_expected_outputs() {
    let out_dir = temp_dir("chat-core-sdk-gen");

    let bin = env!("CARGO_BIN_EXE_sdk_codegen");
    let status = Command::new(bin)
        .arg(&out_dir)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .status()
        .expect("run sdk_codegen");
    assert!(status.success(), "sdk_codegen should succeed");

    let expected = [
        "cpp/messenger_mls.hpp",
        "cpp/smoke.cpp",
        "go/messenger_mls.go",
        "go/go.mod",
        "go/smoke_test.go",
        "dart/pubspec.yaml",
        "dart/messenger_mls.dart",
        "api_spec.resolved.json",
    ];
    for rel in expected {
        let p = out_dir.join(rel);
        assert!(p.exists(), "missing generated file: {}", p.display());
    }

    let resolved =
        fs::read_to_string(out_dir.join("api_spec.resolved.json")).expect("read resolved spec");
    assert!(
        resolved.contains("messenger_mls_create_client"),
        "resolved spec should contain operation names"
    );
    assert!(
        resolved.contains("\"doc\":")
            && !resolved.contains("\"doc\": \"\"")
            && !resolved.contains("\"doc\":\"\""),
        "resolved spec should contain non-empty doc fields"
    );

    let _ = fs::remove_dir_all(&out_dir);
}

#[test]
fn sdk_codegen_fails_when_api_spec_missing() {
    let out_dir = temp_dir("chat-core-sdk-gen-missing-spec-out");
    let cwd = temp_dir("chat-core-sdk-gen-missing-spec-cwd");

    let bin = env!("CARGO_BIN_EXE_sdk_codegen");
    let output = Command::new(bin)
        .arg(&out_dir)
        .current_dir(&cwd)
        .output()
        .expect("run sdk_codegen for missing spec case");

    assert!(
        !output.status.success(),
        "sdk_codegen should fail when api/sdk_api.json is absent"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("failed to read") || stderr.contains("api/sdk_api.json"),
        "unexpected stderr: {stderr}"
    );

    let _ = fs::remove_dir_all(&out_dir);
    let _ = fs::remove_dir_all(&cwd);
}

#[test]
fn sdk_codegen_fills_empty_docs_from_ffi_comments() {
    let cwd = temp_dir("chat-core-sdk-fill-docs-cwd");
    let out_dir = temp_dir("chat-core-sdk-fill-docs-out");

    fs::create_dir_all(cwd.join("api")).expect("create api dir");
    fs::create_dir_all(cwd.join("src")).expect("create src dir");

    fs::write(
        cwd.join("api/sdk_api.json"),
        r#"{
  "namespace": "MessengerMls",
  "ffi_header": "messenger_mls.h",
  "operations": [
    {
      "name": "create_client",
      "ffi_name": "messenger_mls_create_client",
      "input": "json",
      "output": "void",
      "doc": ""
    }
  ]
}"#,
    )
    .expect("write api spec");

    fs::write(
        cwd.join("src/ffi.rs"),
        r#"
/// Filled from ffi docs.
#[unsafe(no_mangle)]
pub extern "C" fn messenger_mls_create_client(
    handle: *mut core::ffi::c_void,
    input_ptr: *const u8,
    input_len: usize,
) -> u32 {
    let _ = (handle, input_ptr, input_len);
    0
}
"#,
    )
    .expect("write ffi fixture");

    let bin = env!("CARGO_BIN_EXE_sdk_codegen");
    let status = Command::new(bin)
        .arg(&out_dir)
        .current_dir(&cwd)
        .status()
        .expect("run sdk_codegen");
    assert!(
        status.success(),
        "sdk_codegen should succeed in fixture workspace"
    );

    let resolved = fs::read_to_string(out_dir.join("api_spec.resolved.json"))
        .expect("read resolved fixture spec");
    assert!(
        resolved.contains("Filled from ffi docs."),
        "expected resolved doc from ffi comments, got: {resolved}"
    );

    let _ = fs::remove_dir_all(&cwd);
    let _ = fs::remove_dir_all(&out_dir);
}

#[test]
fn sdk_codegen_fails_for_duplicate_operation_names() {
    let cwd = temp_dir("chat-core-sdk-invalid-spec-cwd");
    let out_dir = temp_dir("chat-core-sdk-invalid-spec-out");
    fs::create_dir_all(cwd.join("api")).expect("create api dir");
    fs::create_dir_all(cwd.join("src")).expect("create src dir");

    fs::write(
        cwd.join("api/sdk_api.json"),
        r#"{
  "namespace": "MessengerMls",
  "ffi_header": "messenger_mls.h",
  "operations": [
    { "name": "dup", "ffi_name": "messenger_mls_a", "input": "none", "output": "void", "doc": "" },
    { "name": "dup", "ffi_name": "messenger_mls_b", "input": "none", "output": "void", "doc": "" }
  ]
}"#,
    )
    .expect("write api spec");
    fs::write(cwd.join("src/ffi.rs"), "").expect("write empty ffi");

    let bin = env!("CARGO_BIN_EXE_sdk_codegen");
    let output = Command::new(bin)
        .arg(&out_dir)
        .current_dir(&cwd)
        .output()
        .expect("run sdk_codegen");
    assert!(!output.status.success(), "sdk_codegen should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("duplicate operation name"),
        "unexpected stderr: {stderr}"
    );

    let _ = fs::remove_dir_all(&cwd);
    let _ = fs::remove_dir_all(&out_dir);
}
