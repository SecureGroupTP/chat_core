use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Serialize)]
struct ApiSpec {
    namespace: String,
    ffi_header: String,
    operations: Vec<Operation>,
}

#[derive(Debug, Deserialize, Clone, Serialize)]
struct Operation {
    name: String,
    ffi_name: String,
    input: InputKind,
    output: OutputKind,
    #[serde(default)]
    doc: String,
}

#[derive(Debug, Deserialize, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum InputKind {
    None,
    Json,
    Bytes,
    U32,
}

#[derive(Debug, Deserialize, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum OutputKind {
    Void,
    Json,
    Bytes,
}

fn main() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let out_dir = args
        .next()
        .unwrap_or_else(|| "tmp/generated_sdk".to_string());

    let spec_path = Path::new("api/sdk_api.json");
    let spec_raw = fs::read_to_string(spec_path)
        .map_err(|e| format!("failed to read {}: {e}", spec_path.display()))?;
    let mut spec: ApiSpec = serde_json::from_str(&spec_raw)
        .map_err(|e| format!("failed to parse {}: {e}", spec_path.display()))?;

    let ffi_docs = collect_ffi_docs(Path::new("src/ffi.rs"))?;
    for op in &mut spec.operations {
        if op.doc.trim().is_empty()
            && let Some(doc) = ffi_docs.get(&op.ffi_name)
        {
            op.doc = doc.clone();
        }
    }

    let out_root = PathBuf::from(out_dir);
    let cpp_dir = out_root.join("cpp");
    let go_dir = out_root.join("go");
    let dart_dir = out_root.join("dart");

    fs::create_dir_all(&cpp_dir)
        .map_err(|e| format!("failed to create {}: {e}", cpp_dir.display()))?;
    fs::create_dir_all(&go_dir)
        .map_err(|e| format!("failed to create {}: {e}", go_dir.display()))?;
    fs::create_dir_all(&dart_dir)
        .map_err(|e| format!("failed to create {}: {e}", dart_dir.display()))?;

    fs::write(cpp_dir.join("messenger_mls.hpp"), render_cpp(&spec))
        .map_err(|e| format!("failed to write cpp sdk: {e}"))?;
    fs::write(cpp_dir.join("smoke.cpp"), render_cpp_smoke())
        .map_err(|e| format!("failed to write cpp smoke: {e}"))?;

    fs::write(go_dir.join("messenger_mls.go"), render_go(&spec))
        .map_err(|e| format!("failed to write go sdk: {e}"))?;
    fs::write(go_dir.join("go.mod"), render_go_mod())
        .map_err(|e| format!("failed to write go.mod: {e}"))?;
    fs::write(go_dir.join("smoke_test.go"), render_go_smoke())
        .map_err(|e| format!("failed to write go smoke: {e}"))?;

    fs::write(dart_dir.join("pubspec.yaml"), render_dart_pubspec())
        .map_err(|e| format!("failed to write dart pubspec: {e}"))?;
    fs::write(dart_dir.join("messenger_mls.dart"), render_dart(&spec))
        .map_err(|e| format!("failed to write dart sdk: {e}"))?;

    fs::write(
        out_root.join("api_spec.resolved.json"),
        serde_json::to_string_pretty(&spec)
            .map_err(|e| format!("failed to encode resolved spec: {e}"))?,
    )
    .map_err(|e| format!("failed to write resolved spec: {e}"))?;

    println!("Generated SDKs into {}", out_root.display());
    Ok(())
}

fn collect_ffi_docs(path: &Path) -> Result<HashMap<String, String>, String> {
    let src =
        fs::read_to_string(path).map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    let mut docs = HashMap::new();
    let mut pending: Vec<String> = Vec::new();

    for line in src.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("///") {
            pending.push(trimmed.trim_start_matches("///").trim().to_string());
            continue;
        }

        if let Some(name) = parse_fn_name(trimmed) {
            if !pending.is_empty() {
                docs.insert(name.to_string(), pending.join("\n"));
                pending.clear();
            }
            continue;
        }

        if !trimmed.starts_with("#[") && !trimmed.is_empty() {
            pending.clear();
        }
    }

    Ok(docs)
}

fn parse_fn_name(line: &str) -> Option<&str> {
    if !line.contains("extern \"C\"") || !line.contains(" fn ") {
        return None;
    }

    let idx = line.find(" fn ")? + 4;
    let tail = &line[idx..];
    let end = tail.find('(')?;
    Some(tail[..end].trim())
}

fn to_camel_case(s: &str) -> String {
    let mut out = String::new();
    let mut upper = false;
    for ch in s.chars() {
        if ch == '_' {
            upper = true;
            continue;
        }
        if upper {
            out.extend(ch.to_uppercase());
            upper = false;
        } else {
            out.push(ch);
        }
    }
    out
}

fn to_pascal_case(s: &str) -> String {
    let camel = to_camel_case(s);
    let mut chars = camel.chars();
    if let Some(first) = chars.next() {
        let mut out = String::new();
        out.extend(first.to_uppercase());
        out.extend(chars);
        out
    } else {
        String::new()
    }
}

fn escape_cpp_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn render_cpp(spec: &ApiSpec) -> String {
    let mut out = String::new();
    out.push_str("#pragma once\n\n");
    out.push_str("#include <cstdint>\n");
    out.push_str("#include <cstring>\n");
    out.push_str("#include <stdexcept>\n");
    out.push_str("#include <string>\n");
    out.push_str("#include <string_view>\n");
    out.push_str("#include <utility>\n");
    out.push_str("#include <vector>\n\n");
    out.push_str("extern \"C\" {\n");
    out.push_str(&format!("#include \"{}\"\n", spec.ffi_header));
    out.push_str("}\n\n");
    out.push_str("namespace messenger_mls {\n\n");
    out.push_str("class MlsError : public std::runtime_error {\n");
    out.push_str(" public:\n");
    out.push_str("  explicit MlsError(uint32_t code, std::string message)\n");
    out.push_str("      : std::runtime_error(std::move(message)), code_(code) {}\n\n");
    out.push_str("  uint32_t code() const noexcept { return code_; }\n\n");
    out.push_str(" private:\n");
    out.push_str("  uint32_t code_;\n");
    out.push_str("};\n\n");
    out.push_str(&format!("class {} {{\n", spec.namespace));
    out.push_str(" public:\n");
    out.push_str(&format!(
        "  {}() : handle_(messenger_mls_new()) {{\n",
        spec.namespace
    ));
    out.push_str("    if (!handle_) {\n");
    out.push_str("      throw std::bad_alloc();\n");
    out.push_str("    }\n");
    out.push_str("  }\n\n");
    out.push_str(&format!("  ~{}() {{\n", spec.namespace));
    out.push_str("    if (handle_) {\n");
    out.push_str("      messenger_mls_free(handle_);\n");
    out.push_str("      handle_ = nullptr;\n");
    out.push_str("    }\n");
    out.push_str("  }\n\n");
    out.push_str(&format!(
        "  {}(const {}&) = delete;\n",
        spec.namespace, spec.namespace
    ));
    out.push_str(&format!(
        "  {}& operator=(const {}&) = delete;\n\n",
        spec.namespace, spec.namespace
    ));
    out.push_str(&format!(
        "  {}({}&& other) noexcept : handle_(other.handle_) {{\n",
        spec.namespace, spec.namespace
    ));
    out.push_str("    other.handle_ = nullptr;\n");
    out.push_str("  }\n\n");
    out.push_str(&format!(
        "  {}& operator=({}&& other) noexcept {{\n",
        spec.namespace, spec.namespace
    ));
    out.push_str("    if (this == &other) {\n");
    out.push_str("      return *this;\n");
    out.push_str("    }\n");
    out.push_str("    if (handle_) {\n");
    out.push_str("      messenger_mls_free(handle_);\n");
    out.push_str("    }\n");
    out.push_str("    handle_ = other.handle_;\n");
    out.push_str("    other.handle_ = nullptr;\n");
    out.push_str("    return *this;\n");
    out.push_str("  }\n\n");

    for op in &spec.operations {
        let method = to_camel_case(&op.name);
        if !op.doc.trim().is_empty() {
            out.push_str("  // ");
            out.push_str(&op.doc.replace('\n', " "));
            out.push('\n');
        }
        match (op.input, op.output) {
            (InputKind::Json, OutputKind::Void) => {
                out.push_str(&format!(
                    "  void {}(std::string_view json) {{ callVoidInput(\"{}\", json); }}\n\n",
                    method, op.ffi_name
                ));
            }
            (InputKind::Bytes, OutputKind::Void) => {
                out.push_str(&format!(
                    "  void {}(const std::vector<uint8_t>& data) {{ callVoidBytes(\"{}\", data); }}\n\n",
                    method, op.ffi_name
                ));
            }
            (InputKind::None, OutputKind::Void) => {
                out.push_str(&format!(
                    "  void {}() {{ raiseOnError({}(handle_)); }}\n\n",
                    method, op.ffi_name
                ));
            }
            (InputKind::U32, OutputKind::Json) => {
                out.push_str(&format!(
                    "  std::string {}(uint32_t value) {{ return callOutU32(\"{}\", value); }}\n\n",
                    method, op.ffi_name
                ));
            }
            (InputKind::Json, OutputKind::Json) => {
                out.push_str(&format!(
                    "  std::string {}(std::string_view json) {{ return callOutInput(\"{}\", json); }}\n\n",
                    method, op.ffi_name
                ));
            }
            (InputKind::Bytes, OutputKind::Json) => {
                out.push_str(&format!(
                    "  std::string {}(const std::vector<uint8_t>& data) {{ return callOutBytes(\"{}\", data); }}\n\n",
                    method, op.ffi_name
                ));
            }
            (InputKind::None, OutputKind::Json) => {
                out.push_str(&format!(
                    "  std::string {}() {{ return callOutNoInput(\"{}\"); }}\n\n",
                    method, op.ffi_name
                ));
            }
            (InputKind::None, OutputKind::Bytes) => {
                out.push_str(&format!(
                    "  std::vector<uint8_t> {}() {{ return callBytesOut(\"{}\"); }}\n\n",
                    method, op.ffi_name
                ));
            }
            (InputKind::Json, OutputKind::Bytes) => {
                out.push_str(&format!(
                    "  std::vector<uint8_t> {}(std::string_view json) {{ return callBytesOutInput(\"{}\", json); }}\n\n",
                    method, op.ffi_name
                ));
            }
            (InputKind::Bytes, OutputKind::Bytes) => {
                out.push_str(&format!(
                    "  std::vector<uint8_t> {}(const std::vector<uint8_t>& data) {{ return callBytesOutBytes(\"{}\", data); }}\n\n",
                    method, op.ffi_name
                ));
            }
            (InputKind::U32, OutputKind::Void) => {
                out.push_str(&format!(
                    "  void {}(uint32_t value) {{ raiseOnError({}(handle_, value)); }}\n\n",
                    method, op.ffi_name
                ));
            }
            (InputKind::U32, OutputKind::Bytes) => {
                out.push_str(&format!(
                    "  std::vector<uint8_t> {}(uint32_t value) {{ return callBytesOutU32(\"{}\", value); }}\n\n",
                    method, op.ffi_name
                ));
            }
        }
    }

    out.push_str(" private:\n");
    out.push_str("  static std::string takeBufferUtf8(MlsBuffer buf) {\n");
    out.push_str("    std::string out;\n");
    out.push_str("    if (buf.ptr && buf.len > 0) {\n");
    out.push_str("      out.assign(reinterpret_cast<const char*>(buf.ptr), buf.len);\n");
    out.push_str("    }\n");
    out.push_str("    messenger_mls_buffer_free(buf);\n");
    out.push_str("    return out;\n");
    out.push_str("  }\n\n");
    out.push_str("  static std::vector<uint8_t> takeBufferBytes(MlsBuffer buf) {\n");
    out.push_str("    std::vector<uint8_t> out;\n");
    out.push_str("    if (buf.ptr && buf.len > 0) {\n");
    out.push_str("      out.assign(buf.ptr, buf.ptr + buf.len);\n");
    out.push_str("    }\n");
    out.push_str("    messenger_mls_buffer_free(buf);\n");
    out.push_str("    return out;\n");
    out.push_str("  }\n\n");
    out.push_str("  [[noreturn]] void throwLastError(uint32_t code) const {\n");
    out.push_str("    MlsBuffer err{};\n");
    out.push_str("    messenger_mls_last_error(handle_, &err);\n");
    out.push_str("    auto msg = takeBufferUtf8(err);\n");
    out.push_str("    if (msg.empty()) {\n");
    out.push_str("      msg = \"operation failed\";\n");
    out.push_str("    }\n");
    out.push_str("    throw MlsError(code, std::move(msg));\n");
    out.push_str("  }\n\n");
    out.push_str("  void raiseOnError(uint32_t code) const {\n");
    out.push_str("    if (code != 0) {\n");
    out.push_str("      throwLastError(code);\n");
    out.push_str("    }\n");
    out.push_str("  }\n\n");

    out.push_str("  uint32_t callByNameNoInput(const char* name, MlsBuffer* out_buf) {\n");
    out.push_str("    (void)out_buf;\n");
    for op in &spec.operations {
        if matches!(op.input, InputKind::None) {
            match op.output {
                OutputKind::Void => {
                    out.push_str(&format!(
                        "    if (std::strcmp(name, \"{}\") == 0) return {}(handle_);\n",
                        escape_cpp_string(&op.ffi_name),
                        op.ffi_name
                    ));
                }
                _ => {
                    out.push_str(&format!(
                        "    if (std::strcmp(name, \"{}\") == 0) return {}(handle_, out_buf);\n",
                        escape_cpp_string(&op.ffi_name),
                        op.ffi_name
                    ));
                }
            }
        }
    }
    out.push_str("    return 1;\n");
    out.push_str("  }\n\n");

    out.push_str("  uint32_t callByNameInput(const char* name, const uint8_t* ptr, size_t len, MlsBuffer* out_buf) {\n");
    out.push_str("    (void)out_buf;\n");
    for op in &spec.operations {
        if matches!(op.input, InputKind::Json | InputKind::Bytes) {
            match op.output {
                OutputKind::Void => {
                    out.push_str(&format!(
                        "    if (std::strcmp(name, \"{}\") == 0) return {}(handle_, ptr, len);\n",
                        escape_cpp_string(&op.ffi_name),
                        op.ffi_name
                    ));
                }
                _ => {
                    out.push_str(&format!(
                        "    if (std::strcmp(name, \"{}\") == 0) return {}(handle_, ptr, len, out_buf);\n",
                        escape_cpp_string(&op.ffi_name), op.ffi_name
                    ));
                }
            }
        }
    }
    out.push_str("    return 1;\n");
    out.push_str("  }\n\n");

    out.push_str(
        "  uint32_t callByNameU32(const char* name, uint32_t value, MlsBuffer* out_buf) {\n",
    );
    out.push_str("    (void)out_buf;\n");
    for op in &spec.operations {
        if matches!(op.input, InputKind::U32) {
            match op.output {
                OutputKind::Void => {
                    out.push_str(&format!(
                        "    if (std::strcmp(name, \"{}\") == 0) return {}(handle_, value);\n",
                        escape_cpp_string(&op.ffi_name),
                        op.ffi_name
                    ));
                }
                _ => {
                    out.push_str(&format!(
                        "    if (std::strcmp(name, \"{}\") == 0) return {}(handle_, value, out_buf);\n",
                        escape_cpp_string(&op.ffi_name), op.ffi_name
                    ));
                }
            }
        }
    }
    out.push_str("    return 1;\n");
    out.push_str("  }\n\n");

    out.push_str("  void callVoidInput(const char* name, std::string_view json) {\n");
    out.push_str("    const auto* ptr = reinterpret_cast<const uint8_t*>(json.data());\n");
    out.push_str("    raiseOnError(callByNameInput(name, ptr, json.size(), nullptr));\n");
    out.push_str("  }\n\n");

    out.push_str("  void callVoidBytes(const char* name, const std::vector<uint8_t>& bytes) {\n");
    out.push_str("    const uint8_t* ptr = bytes.empty() ? nullptr : bytes.data();\n");
    out.push_str("    raiseOnError(callByNameInput(name, ptr, bytes.size(), nullptr));\n");
    out.push_str("  }\n\n");

    out.push_str("  std::string callOutNoInput(const char* name) {\n");
    out.push_str("    MlsBuffer out_buf{};\n");
    out.push_str("    raiseOnError(callByNameNoInput(name, &out_buf));\n");
    out.push_str("    return takeBufferUtf8(out_buf);\n");
    out.push_str("  }\n\n");

    out.push_str("  std::string callOutInput(const char* name, std::string_view json) {\n");
    out.push_str("    const auto* ptr = reinterpret_cast<const uint8_t*>(json.data());\n");
    out.push_str("    MlsBuffer out_buf{};\n");
    out.push_str("    raiseOnError(callByNameInput(name, ptr, json.size(), &out_buf));\n");
    out.push_str("    return takeBufferUtf8(out_buf);\n");
    out.push_str("  }\n\n");

    out.push_str(
        "  std::string callOutBytes(const char* name, const std::vector<uint8_t>& bytes) {\n",
    );
    out.push_str("    const uint8_t* ptr = bytes.empty() ? nullptr : bytes.data();\n");
    out.push_str("    MlsBuffer out_buf{};\n");
    out.push_str("    raiseOnError(callByNameInput(name, ptr, bytes.size(), &out_buf));\n");
    out.push_str("    return takeBufferUtf8(out_buf);\n");
    out.push_str("  }\n\n");

    out.push_str("  std::string callOutU32(const char* name, uint32_t value) {\n");
    out.push_str("    MlsBuffer out_buf{};\n");
    out.push_str("    raiseOnError(callByNameU32(name, value, &out_buf));\n");
    out.push_str("    return takeBufferUtf8(out_buf);\n");
    out.push_str("  }\n\n");

    out.push_str("  std::vector<uint8_t> callBytesOut(const char* name) {\n");
    out.push_str("    MlsBuffer out_buf{};\n");
    out.push_str("    raiseOnError(callByNameNoInput(name, &out_buf));\n");
    out.push_str("    return takeBufferBytes(out_buf);\n");
    out.push_str("  }\n\n");

    out.push_str(
        "  std::vector<uint8_t> callBytesOutInput(const char* name, std::string_view json) {\n",
    );
    out.push_str("    const auto* ptr = reinterpret_cast<const uint8_t*>(json.data());\n");
    out.push_str("    MlsBuffer out_buf{};\n");
    out.push_str("    raiseOnError(callByNameInput(name, ptr, json.size(), &out_buf));\n");
    out.push_str("    return takeBufferBytes(out_buf);\n");
    out.push_str("  }\n\n");

    out.push_str("  std::vector<uint8_t> callBytesOutBytes(const char* name, const std::vector<uint8_t>& bytes) {\n");
    out.push_str("    const uint8_t* ptr = bytes.empty() ? nullptr : bytes.data();\n");
    out.push_str("    MlsBuffer out_buf{};\n");
    out.push_str("    raiseOnError(callByNameInput(name, ptr, bytes.size(), &out_buf));\n");
    out.push_str("    return takeBufferBytes(out_buf);\n");
    out.push_str("  }\n\n");

    out.push_str("  std::vector<uint8_t> callBytesOutU32(const char* name, uint32_t value) {\n");
    out.push_str("    MlsBuffer out_buf{};\n");
    out.push_str("    raiseOnError(callByNameU32(name, value, &out_buf));\n");
    out.push_str("    return takeBufferBytes(out_buf);\n");
    out.push_str("  }\n\n");

    out.push_str("  MessengerMlsHandle* handle_;\n");
    out.push_str("};\n\n");
    out.push_str("}  // namespace messenger_mls\n");
    out
}

fn render_cpp_smoke() -> String {
    let mut out = String::new();
    out.push_str("#include \"messenger_mls.hpp\"\n\n");
    out.push_str("int main() {\n");
    out.push_str("  messenger_mls::MessengerMls client;\n");
    out.push_str("  (void)client;\n");
    out.push_str("  return 0;\n");
    out.push_str("}\n");
    out
}

fn render_go(spec: &ApiSpec) -> String {
    let mut out = String::new();
    out.push_str("package messengermls\n\n");
    out.push_str("/*\n");
    out.push_str("#cgo CFLAGS: -I../../include\n");
    out.push_str("#cgo LDFLAGS: -L../../target/release -l:libchat_core.a\n");
    out.push_str("#include <stdlib.h>\n");
    out.push_str(&format!("#include \"{}\"\n", spec.ffi_header));
    out.push_str("*/\n");
    out.push_str("import \"C\"\n\n");
    out.push_str("import (\n");
    out.push_str("\t\"encoding/json\"\n");
    out.push_str("\t\"errors\"\n");
    out.push_str("\t\"fmt\"\n");
    out.push_str("\t\"unsafe\"\n");
    out.push_str(")\n\n");
    out.push_str("type Client struct {\n");
    out.push_str("\thandle *C.MessengerMlsHandle\n");
    out.push_str("}\n\n");
    out.push_str("func New() (*Client, error) {\n");
    out.push_str("\th := C.messenger_mls_new()\n");
    out.push_str("\tif h == nil {\n");
    out.push_str("\t\treturn nil, errors.New(\"messenger_mls_new returned nil\")\n");
    out.push_str("\t}\n");
    out.push_str("\treturn &Client{handle: h}, nil\n");
    out.push_str("}\n\n");
    out.push_str("func (c *Client) Close() {\n");
    out.push_str("\tif c != nil && c.handle != nil {\n");
    out.push_str("\t\tC.messenger_mls_free(c.handle)\n");
    out.push_str("\t\tc.handle = nil\n");
    out.push_str("\t}\n");
    out.push_str("}\n\n");

    out.push_str("func (c *Client) statusError(code C.uint32_t) error {\n");
    out.push_str("\tif code == 0 {\n");
    out.push_str("\t\treturn nil\n");
    out.push_str("\t}\n");
    out.push_str("\tmsg := c.lastError()\n");
    out.push_str("\tif msg == \"\" {\n");
    out.push_str("\t\tmsg = \"operation failed\"\n");
    out.push_str("\t}\n");
    out.push_str("\treturn fmt.Errorf(\"mls error %d: %s\", uint32(code), msg)\n");
    out.push_str("}\n\n");

    out.push_str("func (c *Client) lastError() string {\n");
    out.push_str("\tif c == nil || c.handle == nil {\n");
    out.push_str("\t\treturn \"\"\n");
    out.push_str("\t}\n");
    out.push_str("\tvar buf C.MlsBuffer\n");
    out.push_str("\t_ = C.messenger_mls_last_error(c.handle, &buf)\n");
    out.push_str("\tdefer C.messenger_mls_buffer_free(buf)\n");
    out.push_str("\tif buf.ptr == nil || buf.len == 0 {\n");
    out.push_str("\t\treturn \"\"\n");
    out.push_str("\t}\n");
    out.push_str("\treturn C.GoStringN((*C.char)(unsafe.Pointer(buf.ptr)), C.int(buf.len))\n");
    out.push_str("}\n\n");

    out.push_str("func withBytes(data []byte) (*C.uint8_t, C.size_t) {\n");
    out.push_str("\tif len(data) == 0 {\n");
    out.push_str("\t\treturn nil, 0\n");
    out.push_str("\t}\n");
    out.push_str("\treturn (*C.uint8_t)(unsafe.Pointer(&data[0])), C.size_t(len(data))\n");
    out.push_str("}\n\n");

    out.push_str("func takeBuffer(buf C.MlsBuffer) []byte {\n");
    out.push_str("\tdefer C.messenger_mls_buffer_free(buf)\n");
    out.push_str("\tif buf.ptr == nil || buf.len == 0 {\n");
    out.push_str("\t\treturn nil\n");
    out.push_str("\t}\n");
    out.push_str("\treturn C.GoBytes(unsafe.Pointer(buf.ptr), C.int(buf.len))\n");
    out.push_str("}\n\n");

    out.push_str("func mustJSON(v any) ([]byte, error) {\n");
    out.push_str("\tbytes, err := json.Marshal(v)\n");
    out.push_str("\tif err != nil {\n");
    out.push_str("\t\treturn nil, err\n");
    out.push_str("\t}\n");
    out.push_str("\treturn bytes, nil\n");
    out.push_str("}\n\n");

    for op in &spec.operations {
        let method = to_pascal_case(&op.name);
        if !op.doc.trim().is_empty() {
            out.push_str("// ");
            out.push_str(&method);
            out.push(' ');
            out.push_str(&op.doc.replace('\n', " "));
            out.push('\n');
        }

        match (op.input, op.output) {
            (InputKind::Json, OutputKind::Void) => {
                out.push_str(&format!(
                    "func (c *Client) {}(value any) error {{\n",
                    method
                ));
                out.push_str("\tif c == nil || c.handle == nil {\n\t\treturn errors.New(\"client is closed\")\n\t}\n");
                out.push_str(
                    "\tdata, err := mustJSON(value)\n\tif err != nil {\n\t\treturn err\n\t}\n",
                );
                out.push_str("\tptr, n := withBytes(data)\n");
                out.push_str(&format!("\tcode := C.{}(c.handle, ptr, n)\n", op.ffi_name));
                out.push_str("\treturn c.statusError(code)\n");
                out.push_str("}\n\n");
            }
            (InputKind::Bytes, OutputKind::Void) => {
                out.push_str(&format!(
                    "func (c *Client) {}(data []byte) error {{\n",
                    method
                ));
                out.push_str("\tif c == nil || c.handle == nil {\n\t\treturn errors.New(\"client is closed\")\n\t}\n");
                out.push_str("\tptr, n := withBytes(data)\n");
                out.push_str(&format!("\tcode := C.{}(c.handle, ptr, n)\n", op.ffi_name));
                out.push_str("\treturn c.statusError(code)\n");
                out.push_str("}\n\n");
            }
            (InputKind::None, OutputKind::Void) => {
                out.push_str(&format!("func (c *Client) {}() error {{\n", method));
                out.push_str("\tif c == nil || c.handle == nil {\n\t\treturn errors.New(\"client is closed\")\n\t}\n");
                out.push_str(&format!("\tcode := C.{}(c.handle)\n", op.ffi_name));
                out.push_str("\treturn c.statusError(code)\n");
                out.push_str("}\n\n");
            }
            (InputKind::U32, OutputKind::Void) => {
                out.push_str(&format!(
                    "func (c *Client) {}(value uint32) error {{\n",
                    method
                ));
                out.push_str("\tif c == nil || c.handle == nil {\n\t\treturn errors.New(\"client is closed\")\n\t}\n");
                out.push_str(&format!(
                    "\tcode := C.{}(c.handle, C.uint32_t(value))\n",
                    op.ffi_name
                ));
                out.push_str("\treturn c.statusError(code)\n");
                out.push_str("}\n\n");
            }
            (InputKind::None, OutputKind::Bytes) => {
                out.push_str(&format!(
                    "func (c *Client) {}() ([]byte, error) {{\n",
                    method
                ));
                out.push_str("\tif c == nil || c.handle == nil {\n\t\treturn nil, errors.New(\"client is closed\")\n\t}\n");
                out.push_str("\tvar outBuf C.MlsBuffer\n");
                out.push_str(&format!("\tcode := C.{}(c.handle, &outBuf)\n", op.ffi_name));
                out.push_str(
                    "\tif err := c.statusError(code); err != nil {\n\t\treturn nil, err\n\t}\n",
                );
                out.push_str("\treturn takeBuffer(outBuf), nil\n");
                out.push_str("}\n\n");
            }
            (InputKind::Json, OutputKind::Bytes) => {
                out.push_str(&format!(
                    "func (c *Client) {}(value any) ([]byte, error) {{\n",
                    method
                ));
                out.push_str("\tif c == nil || c.handle == nil {\n\t\treturn nil, errors.New(\"client is closed\")\n\t}\n");
                out.push_str(
                    "\tdata, err := mustJSON(value)\n\tif err != nil {\n\t\treturn nil, err\n\t}\n",
                );
                out.push_str("\tptr, n := withBytes(data)\n\tvar outBuf C.MlsBuffer\n");
                out.push_str(&format!(
                    "\tcode := C.{}(c.handle, ptr, n, &outBuf)\n",
                    op.ffi_name
                ));
                out.push_str(
                    "\tif err := c.statusError(code); err != nil {\n\t\treturn nil, err\n\t}\n",
                );
                out.push_str("\treturn takeBuffer(outBuf), nil\n");
                out.push_str("}\n\n");
            }
            (InputKind::Bytes, OutputKind::Bytes) => {
                out.push_str(&format!(
                    "func (c *Client) {}(data []byte) ([]byte, error) {{\n",
                    method
                ));
                out.push_str("\tif c == nil || c.handle == nil {\n\t\treturn nil, errors.New(\"client is closed\")\n\t}\n");
                out.push_str("\tptr, n := withBytes(data)\n\tvar outBuf C.MlsBuffer\n");
                out.push_str(&format!(
                    "\tcode := C.{}(c.handle, ptr, n, &outBuf)\n",
                    op.ffi_name
                ));
                out.push_str(
                    "\tif err := c.statusError(code); err != nil {\n\t\treturn nil, err\n\t}\n",
                );
                out.push_str("\treturn takeBuffer(outBuf), nil\n");
                out.push_str("}\n\n");
            }
            (InputKind::U32, OutputKind::Bytes) => {
                out.push_str(&format!(
                    "func (c *Client) {}(value uint32) ([]byte, error) {{\n",
                    method
                ));
                out.push_str("\tif c == nil || c.handle == nil {\n\t\treturn nil, errors.New(\"client is closed\")\n\t}\n");
                out.push_str("\tvar outBuf C.MlsBuffer\n");
                out.push_str(&format!(
                    "\tcode := C.{}(c.handle, C.uint32_t(value), &outBuf)\n",
                    op.ffi_name
                ));
                out.push_str(
                    "\tif err := c.statusError(code); err != nil {\n\t\treturn nil, err\n\t}\n",
                );
                out.push_str("\treturn takeBuffer(outBuf), nil\n");
                out.push_str("}\n\n");
            }
            (InputKind::None, OutputKind::Json) => {
                out.push_str(&format!(
                    "func (c *Client) {}() (json.RawMessage, error) {{\n",
                    method
                ));
                out.push_str("\tif c == nil || c.handle == nil {\n\t\treturn nil, errors.New(\"client is closed\")\n\t}\n");
                out.push_str("\tvar outBuf C.MlsBuffer\n");
                out.push_str(&format!("\tcode := C.{}(c.handle, &outBuf)\n", op.ffi_name));
                out.push_str(
                    "\tif err := c.statusError(code); err != nil {\n\t\treturn nil, err\n\t}\n",
                );
                out.push_str("\treturn json.RawMessage(takeBuffer(outBuf)), nil\n");
                out.push_str("}\n\n");
            }
            (InputKind::Json, OutputKind::Json) => {
                out.push_str(&format!(
                    "func (c *Client) {}(value any) (json.RawMessage, error) {{\n",
                    method
                ));
                out.push_str("\tif c == nil || c.handle == nil {\n\t\treturn nil, errors.New(\"client is closed\")\n\t}\n");
                out.push_str(
                    "\tdata, err := mustJSON(value)\n\tif err != nil {\n\t\treturn nil, err\n\t}\n",
                );
                out.push_str("\tptr, n := withBytes(data)\n\tvar outBuf C.MlsBuffer\n");
                out.push_str(&format!(
                    "\tcode := C.{}(c.handle, ptr, n, &outBuf)\n",
                    op.ffi_name
                ));
                out.push_str(
                    "\tif err := c.statusError(code); err != nil {\n\t\treturn nil, err\n\t}\n",
                );
                out.push_str("\treturn json.RawMessage(takeBuffer(outBuf)), nil\n");
                out.push_str("}\n\n");
            }
            (InputKind::Bytes, OutputKind::Json) => {
                out.push_str(&format!(
                    "func (c *Client) {}(data []byte) (json.RawMessage, error) {{\n",
                    method
                ));
                out.push_str("\tif c == nil || c.handle == nil {\n\t\treturn nil, errors.New(\"client is closed\")\n\t}\n");
                out.push_str("\tptr, n := withBytes(data)\n\tvar outBuf C.MlsBuffer\n");
                out.push_str(&format!(
                    "\tcode := C.{}(c.handle, ptr, n, &outBuf)\n",
                    op.ffi_name
                ));
                out.push_str(
                    "\tif err := c.statusError(code); err != nil {\n\t\treturn nil, err\n\t}\n",
                );
                out.push_str("\treturn json.RawMessage(takeBuffer(outBuf)), nil\n");
                out.push_str("}\n\n");
            }
            (InputKind::U32, OutputKind::Json) => {
                out.push_str(&format!(
                    "func (c *Client) {}(value uint32) (json.RawMessage, error) {{\n",
                    method
                ));
                out.push_str("\tif c == nil || c.handle == nil {\n\t\treturn nil, errors.New(\"client is closed\")\n\t}\n");
                out.push_str("\tvar outBuf C.MlsBuffer\n");
                out.push_str(&format!(
                    "\tcode := C.{}(c.handle, C.uint32_t(value), &outBuf)\n",
                    op.ffi_name
                ));
                out.push_str(
                    "\tif err := c.statusError(code); err != nil {\n\t\treturn nil, err\n\t}\n",
                );
                out.push_str("\treturn json.RawMessage(takeBuffer(outBuf)), nil\n");
                out.push_str("}\n\n");
            }
        }
    }

    out
}

fn render_go_mod() -> &'static str {
    "module messengermls\n\ngo 1.22\n"
}

fn render_go_smoke() -> String {
    let mut out = String::new();
    out.push_str("package messengermls\n\n");
    out.push_str("import \"testing\"\n\n");
    out.push_str("func TestNewClose(t *testing.T) {\n");
    out.push_str("\tclient, err := New()\n");
    out.push_str("\tif err != nil {\n");
    out.push_str("\t\tt.Fatalf(\"New() failed: %v\", err)\n");
    out.push_str("\t}\n");
    out.push_str("\tclient.Close()\n");
    out.push_str("}\n");
    out
}

fn render_dart_pubspec() -> &'static str {
    "name: messenger_mls_generated\ndescription: Generated Dart FFI wrapper for chat_core\nversion: 0.0.1\nenvironment:\n  sdk: '>=3.3.0 <4.0.0'\ndependencies:\n  ffi: ^2.1.2\n"
}

fn render_dart(spec: &ApiSpec) -> String {
    let mut out = String::new();
    out.push_str("// ignore_for_file: unused_element\n\n");
    out.push_str("import 'dart:async';\n");
    out.push_str("import 'dart:convert';\n");
    out.push_str("import 'dart:ffi' as ffi;\n");
    out.push_str("import 'dart:io';\n");
    out.push_str("import 'dart:typed_data';\n\n");
    out.push_str("import 'package:ffi/ffi.dart';\n\n");
    out.push_str("final class MlsException implements Exception {\n");
    out.push_str("  MlsException(this.code, this.message);\n");
    out.push_str("  final int code;\n");
    out.push_str("  final String message;\n\n");
    out.push_str("  @override\n");
    out.push_str("  String toString() => 'MlsException(code: $code, message: $message)';\n");
    out.push_str("}\n\n");

    out.push_str("final class MlsBuffer extends ffi.Struct {\n");
    out.push_str("  external ffi.Pointer<ffi.Uint8> ptr;\n\n");
    out.push_str("  @ffi.Size()\n");
    out.push_str("  external int len;\n");
    out.push_str("}\n\n");
    out.push_str("typedef _NewNative = ffi.Pointer<ffi.Void> Function();\n");
    out.push_str("typedef _NewDart = ffi.Pointer<ffi.Void> Function();\n");
    out.push_str("typedef _FreeNative = ffi.Void Function(ffi.Pointer<ffi.Void>);\n");
    out.push_str("typedef _FreeDart = void Function(ffi.Pointer<ffi.Void>);\n");
    out.push_str("typedef _LastErrorNative = ffi.Uint32 Function(ffi.Pointer<ffi.Void>, ffi.Pointer<MlsBuffer>);\n");
    out.push_str(
        "typedef _LastErrorDart = int Function(ffi.Pointer<ffi.Void>, ffi.Pointer<MlsBuffer>);\n",
    );
    out.push_str("typedef _BufferFreeNative = ffi.Void Function(MlsBuffer);\n");
    out.push_str("typedef _BufferFreeDart = void Function(MlsBuffer);\n");
    out.push_str("typedef _NoInputVoidNative = ffi.Uint32 Function(ffi.Pointer<ffi.Void>);\n");
    out.push_str("typedef _NoInputVoidDart = int Function(ffi.Pointer<ffi.Void>);\n");
    out.push_str("typedef _NoInputOutNative = ffi.Uint32 Function(ffi.Pointer<ffi.Void>, ffi.Pointer<MlsBuffer>);\n");
    out.push_str(
        "typedef _NoInputOutDart = int Function(ffi.Pointer<ffi.Void>, ffi.Pointer<MlsBuffer>);\n",
    );
    out.push_str("typedef _InputVoidNative = ffi.Uint32 Function(ffi.Pointer<ffi.Void>, ffi.Pointer<ffi.Uint8>, ffi.Size);\n");
    out.push_str("typedef _InputVoidDart = int Function(ffi.Pointer<ffi.Void>, ffi.Pointer<ffi.Uint8>, int);\n");
    out.push_str("typedef _InputOutNative = ffi.Uint32 Function(ffi.Pointer<ffi.Void>, ffi.Pointer<ffi.Uint8>, ffi.Size, ffi.Pointer<MlsBuffer>);\n");
    out.push_str("typedef _InputOutDart = int Function(ffi.Pointer<ffi.Void>, ffi.Pointer<ffi.Uint8>, int, ffi.Pointer<MlsBuffer>);\n");
    out.push_str(
        "typedef _U32VoidNative = ffi.Uint32 Function(ffi.Pointer<ffi.Void>, ffi.Uint32);\n",
    );
    out.push_str("typedef _U32VoidDart = int Function(ffi.Pointer<ffi.Void>, int);\n");
    out.push_str("typedef _U32OutNative = ffi.Uint32 Function(ffi.Pointer<ffi.Void>, ffi.Uint32, ffi.Pointer<MlsBuffer>);\n");
    out.push_str("typedef _U32OutDart = int Function(ffi.Pointer<ffi.Void>, int, ffi.Pointer<MlsBuffer>);\n\n");
    out.push_str("final class MessengerMls implements ffi.Finalizable {\n");
    out.push_str("  MessengerMls._(this._lib, this._handle) {\n");
    out.push_str("    _finalizer.attach(this, _handle);\n");
    out.push_str("  }\n\n");
    out.push_str("  final ffi.DynamicLibrary _lib;\n");
    out.push_str("  ffi.Pointer<ffi.Void> _handle;\n\n");
    out.push_str("  static final _finalizer = ffi.NativeFinalizer(\n");
    out.push_str(
        "    _resolveLib().lookup<ffi.NativeFunction<_FreeNative>>('messenger_mls_free').cast(),\n",
    );
    out.push_str("  );\n\n");
    out.push_str("  static ffi.DynamicLibrary _resolveLib() {\n");
    out.push_str(
        "    if (Platform.isMacOS) return ffi.DynamicLibrary.open('libchat_core.dylib');\n",
    );
    out.push_str("    if (Platform.isWindows) return ffi.DynamicLibrary.open('chat_core.dll');\n");
    out.push_str("    return ffi.DynamicLibrary.open('libchat_core.so');\n");
    out.push_str("  }\n\n");
    out.push_str("  static MessengerMls create({ffi.DynamicLibrary? library}) {\n");
    out.push_str("    final lib = library ?? _resolveLib();\n");
    out.push_str(
        "    final newFn = lib.lookupFunction<_NewNative, _NewDart>('messenger_mls_new');\n",
    );
    out.push_str("    final handle = newFn();\n");
    out.push_str("    if (handle == ffi.nullptr) {\n");
    out.push_str("      throw StateError('messenger_mls_new returned nullptr');\n");
    out.push_str("    }\n");
    out.push_str("    return MessengerMls._(lib, handle);\n");
    out.push_str("  }\n\n");
    out.push_str("  void close() {\n");
    out.push_str("    if (_handle == ffi.nullptr) return;\n");
    out.push_str(
        "    final freeFn = _lib.lookupFunction<_FreeNative, _FreeDart>('messenger_mls_free');\n",
    );
    out.push_str("    _finalizer.detach(this);\n");
    out.push_str("    freeFn(_handle);\n");
    out.push_str("    _handle = ffi.nullptr;\n");
    out.push_str("  }\n\n");
    out.push_str("  String _lastError() {\n");
    out.push_str("    final fn = _lib.lookupFunction<_LastErrorNative, _LastErrorDart>('messenger_mls_last_error');\n");
    out.push_str("    final freeBuf = _lib.lookupFunction<_BufferFreeNative, _BufferFreeDart>('messenger_mls_buffer_free');\n");
    out.push_str("    final outBuf = calloc<MlsBuffer>();\n");
    out.push_str("    try {\n");
    out.push_str("      fn(_handle, outBuf);\n");
    out.push_str("      final ptr = outBuf.ref.ptr;\n");
    out.push_str("      final len = outBuf.ref.len;\n");
    out.push_str("      if (ptr == ffi.nullptr || len == 0) return '';\n");
    out.push_str("      final bytes = ptr.asTypedList(len);\n");
    out.push_str("      return utf8.decode(bytes, allowMalformed: true);\n");
    out.push_str("    } finally {\n");
    out.push_str("      freeBuf(outBuf.ref);\n");
    out.push_str("      calloc.free(outBuf);\n");
    out.push_str("    }\n");
    out.push_str("  }\n\n");

    out.push_str("  void _throwIfError(int code) {\n");
    out.push_str("    if (code == 0) return;\n");
    out.push_str("    final msg = _lastError();\n");
    out.push_str("    throw MlsException(code, msg.isEmpty ? 'operation failed' : msg);\n");
    out.push_str("  }\n\n");

    out.push_str("  Uint8List _takeBytes(MlsBuffer buf) {\n");
    out.push_str("    final freeBuf = _lib.lookupFunction<_BufferFreeNative, _BufferFreeDart>('messenger_mls_buffer_free');\n");
    out.push_str("    try {\n");
    out.push_str("      if (buf.ptr == ffi.nullptr || buf.len == 0) return Uint8List(0);\n");
    out.push_str("      return Uint8List.fromList(buf.ptr.asTypedList(buf.len));\n");
    out.push_str("    } finally {\n");
    out.push_str("      freeBuf(buf);\n");
    out.push_str("    }\n");
    out.push_str("  }\n\n");

    out.push_str("  String _takeJson(MlsBuffer buf) => utf8.decode(_takeBytes(buf), allowMalformed: true);\n\n");
    out.push_str("  int _callNoInputVoid(String symbol) {\n");
    out.push_str(
        "    final fn = _lib.lookupFunction<_NoInputVoidNative, _NoInputVoidDart>(symbol);\n",
    );
    out.push_str("    return fn(_handle);\n");
    out.push_str("  }\n\n");
    out.push_str("  int _callNoInputOut(String symbol, ffi.Pointer<MlsBuffer> outBuf) {\n");
    out.push_str(
        "    final fn = _lib.lookupFunction<_NoInputOutNative, _NoInputOutDart>(symbol);\n",
    );
    out.push_str("    return fn(_handle, outBuf);\n");
    out.push_str("  }\n\n");
    out.push_str("  int _callInputVoid(String symbol, Uint8List data) {\n");
    out.push_str("    final fn = _lib.lookupFunction<_InputVoidNative, _InputVoidDart>(symbol);\n");
    out.push_str("    if (data.isEmpty) return fn(_handle, ffi.nullptr, 0);\n");
    out.push_str("    final ptr = calloc<ffi.Uint8>(data.length);\n");
    out.push_str("    try {\n");
    out.push_str("      ptr.asTypedList(data.length).setAll(0, data);\n");
    out.push_str("      return fn(_handle, ptr, data.length);\n");
    out.push_str("    } finally {\n");
    out.push_str("      calloc.free(ptr);\n");
    out.push_str("    }\n");
    out.push_str("  }\n\n");
    out.push_str(
        "  int _callInputOut(String symbol, Uint8List data, ffi.Pointer<MlsBuffer> outBuf) {\n",
    );
    out.push_str("    final fn = _lib.lookupFunction<_InputOutNative, _InputOutDart>(symbol);\n");
    out.push_str("    if (data.isEmpty) return fn(_handle, ffi.nullptr, 0, outBuf);\n");
    out.push_str("    final ptr = calloc<ffi.Uint8>(data.length);\n");
    out.push_str("    try {\n");
    out.push_str("      ptr.asTypedList(data.length).setAll(0, data);\n");
    out.push_str("      return fn(_handle, ptr, data.length, outBuf);\n");
    out.push_str("    } finally {\n");
    out.push_str("      calloc.free(ptr);\n");
    out.push_str("    }\n");
    out.push_str("  }\n\n");
    out.push_str("  int _callU32Void(String symbol, int value) {\n");
    out.push_str("    final fn = _lib.lookupFunction<_U32VoidNative, _U32VoidDart>(symbol);\n");
    out.push_str("    return fn(_handle, value);\n");
    out.push_str("  }\n\n");
    out.push_str("  int _callU32Out(String symbol, int value, ffi.Pointer<MlsBuffer> outBuf) {\n");
    out.push_str("    final fn = _lib.lookupFunction<_U32OutNative, _U32OutDart>(symbol);\n");
    out.push_str("    return fn(_handle, value, outBuf);\n");
    out.push_str("  }\n\n");

    for op in &spec.operations {
        let method = to_camel_case(&op.name);
        if !op.doc.trim().is_empty() {
            out.push_str("  /// ");
            out.push_str(&op.doc.replace('\n', " "));
            out.push('\n');
        }
        match (op.input, op.output) {
            (InputKind::Json, OutputKind::Void) => {
                out.push_str(&format!("  void {}Sync(Object? value) {{\n", method));
                out.push_str(
                    "    final data = Uint8List.fromList(utf8.encode(jsonEncode(value)));\n",
                );
                out.push_str(&format!(
                    "    _throwIfError(_callInputVoid('{}', data));\n",
                    op.ffi_name
                ));
                out.push_str("  }\n");
                out.push_str(&format!("  Future<void> {}(Object? value) => Future<void>.microtask(() => {}Sync(value));\n\n", method, method));
            }
            (InputKind::Bytes, OutputKind::Void) => {
                out.push_str(&format!("  void {}Sync(Uint8List data) {{\n", method));
                out.push_str(&format!(
                    "    _throwIfError(_callInputVoid('{}', data));\n",
                    op.ffi_name
                ));
                out.push_str("  }\n");
                out.push_str(&format!("  Future<void> {}(Uint8List data) => Future<void>.microtask(() => {}Sync(data));\n\n", method, method));
            }
            (InputKind::None, OutputKind::Void) => {
                out.push_str(&format!("  void {}Sync() {{\n", method));
                out.push_str(&format!(
                    "    _throwIfError(_callNoInputVoid('{}'));\n",
                    op.ffi_name
                ));
                out.push_str("  }\n");
                out.push_str(&format!(
                    "  Future<void> {}() => Future<void>.microtask({}Sync);\n\n",
                    method, method
                ));
            }
            (InputKind::U32, OutputKind::Void) => {
                out.push_str(&format!("  void {}Sync(int value) {{\n", method));
                out.push_str(&format!(
                    "    _throwIfError(_callU32Void('{}', value));\n",
                    op.ffi_name
                ));
                out.push_str("  }\n");
                out.push_str(&format!("  Future<void> {}(int value) => Future<void>.microtask(() => {}Sync(value));\n\n", method, method));
            }
            (InputKind::None, OutputKind::Bytes) => {
                out.push_str(&format!("  Uint8List {}Sync() {{\n", method));
                out.push_str("    final outBuf = calloc<MlsBuffer>();\n");
                out.push_str("    try {\n");
                out.push_str(&format!(
                    "      _throwIfError(_callNoInputOut('{}', outBuf));\n",
                    op.ffi_name
                ));
                out.push_str("      return _takeBytes(outBuf.ref);\n");
                out.push_str("    } finally {\n");
                out.push_str("      calloc.free(outBuf);\n");
                out.push_str("    }\n");
                out.push_str("  }\n");
                out.push_str(&format!(
                    "  Future<Uint8List> {}() => Future<Uint8List>.microtask({}Sync);\n\n",
                    method, method
                ));
            }
            (InputKind::Json, OutputKind::Bytes) => {
                out.push_str(&format!("  Uint8List {}Sync(Object? value) {{\n", method));
                out.push_str(
                    "    final data = Uint8List.fromList(utf8.encode(jsonEncode(value)));\n",
                );
                out.push_str("    final outBuf = calloc<MlsBuffer>();\n");
                out.push_str("    try {\n");
                out.push_str(&format!(
                    "      _throwIfError(_callInputOut('{}', data, outBuf));\n",
                    op.ffi_name
                ));
                out.push_str("      return _takeBytes(outBuf.ref);\n");
                out.push_str("    } finally {\n");
                out.push_str("      calloc.free(outBuf);\n");
                out.push_str("    }\n");
                out.push_str("  }\n");
                out.push_str(&format!("  Future<Uint8List> {}(Object? value) => Future<Uint8List>.microtask(() => {}Sync(value));\n\n", method, method));
            }
            (InputKind::Bytes, OutputKind::Bytes) => {
                out.push_str(&format!("  Uint8List {}Sync(Uint8List data) {{\n", method));
                out.push_str("    final outBuf = calloc<MlsBuffer>();\n");
                out.push_str("    try {\n");
                out.push_str(&format!(
                    "      _throwIfError(_callInputOut('{}', data, outBuf));\n",
                    op.ffi_name
                ));
                out.push_str("      return _takeBytes(outBuf.ref);\n");
                out.push_str("    } finally {\n");
                out.push_str("      calloc.free(outBuf);\n");
                out.push_str("    }\n");
                out.push_str("  }\n");
                out.push_str(&format!("  Future<Uint8List> {}(Uint8List data) => Future<Uint8List>.microtask(() => {}Sync(data));\n\n", method, method));
            }
            (InputKind::U32, OutputKind::Bytes) => {
                out.push_str(&format!("  Uint8List {}Sync(int value) {{\n", method));
                out.push_str("    final outBuf = calloc<MlsBuffer>();\n");
                out.push_str("    try {\n");
                out.push_str(&format!(
                    "      _throwIfError(_callU32Out('{}', value, outBuf));\n",
                    op.ffi_name
                ));
                out.push_str("      return _takeBytes(outBuf.ref);\n");
                out.push_str("    } finally {\n");
                out.push_str("      calloc.free(outBuf);\n");
                out.push_str("    }\n");
                out.push_str("  }\n");
                out.push_str(&format!("  Future<Uint8List> {}(int value) => Future<Uint8List>.microtask(() => {}Sync(value));\n\n", method, method));
            }
            (InputKind::None, OutputKind::Json) => {
                out.push_str(&format!("  Object? {}Sync() {{\n", method));
                out.push_str("    final outBuf = calloc<MlsBuffer>();\n");
                out.push_str("    try {\n");
                out.push_str(&format!(
                    "      _throwIfError(_callNoInputOut('{}', outBuf));\n",
                    op.ffi_name
                ));
                out.push_str("      return jsonDecode(_takeJson(outBuf.ref));\n");
                out.push_str("    } finally {\n");
                out.push_str("      calloc.free(outBuf);\n");
                out.push_str("    }\n");
                out.push_str("  }\n");
                out.push_str(&format!(
                    "  Future<Object?> {}() => Future<Object?>.microtask({}Sync);\n\n",
                    method, method
                ));
            }
            (InputKind::Json, OutputKind::Json) => {
                out.push_str(&format!("  Object? {}Sync(Object? value) {{\n", method));
                out.push_str(
                    "    final data = Uint8List.fromList(utf8.encode(jsonEncode(value)));\n",
                );
                out.push_str("    final outBuf = calloc<MlsBuffer>();\n");
                out.push_str("    try {\n");
                out.push_str(&format!(
                    "      _throwIfError(_callInputOut('{}', data, outBuf));\n",
                    op.ffi_name
                ));
                out.push_str("      return jsonDecode(_takeJson(outBuf.ref));\n");
                out.push_str("    } finally {\n");
                out.push_str("      calloc.free(outBuf);\n");
                out.push_str("    }\n");
                out.push_str("  }\n");
                out.push_str(&format!("  Future<Object?> {}(Object? value) => Future<Object?>.microtask(() => {}Sync(value));\n\n", method, method));
            }
            (InputKind::Bytes, OutputKind::Json) => {
                out.push_str(&format!("  Object? {}Sync(Uint8List data) {{\n", method));
                out.push_str("    final outBuf = calloc<MlsBuffer>();\n");
                out.push_str("    try {\n");
                out.push_str(&format!(
                    "      _throwIfError(_callInputOut('{}', data, outBuf));\n",
                    op.ffi_name
                ));
                out.push_str("      return jsonDecode(_takeJson(outBuf.ref));\n");
                out.push_str("    } finally {\n");
                out.push_str("      calloc.free(outBuf);\n");
                out.push_str("    }\n");
                out.push_str("  }\n");
                out.push_str(&format!("  Future<Object?> {}(Uint8List data) => Future<Object?>.microtask(() => {}Sync(data));\n\n", method, method));
            }
            (InputKind::U32, OutputKind::Json) => {
                out.push_str(&format!("  Object? {}Sync(int value) {{\n", method));
                out.push_str("    final outBuf = calloc<MlsBuffer>();\n");
                out.push_str("    try {\n");
                out.push_str(&format!(
                    "      _throwIfError(_callU32Out('{}', value, outBuf));\n",
                    op.ffi_name
                ));
                out.push_str("      return jsonDecode(_takeJson(outBuf.ref));\n");
                out.push_str("    } finally {\n");
                out.push_str("      calloc.free(outBuf);\n");
                out.push_str("    }\n");
                out.push_str("  }\n");
                out.push_str(&format!("  Future<Object?> {}(int value) => Future<Object?>.microtask(() => {}Sync(value));\n\n", method, method));
            }
        }
    }

    out.push_str("}\n");

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn full_spec() -> ApiSpec {
        let mut ops = Vec::new();
        let mut push = |name: &str, input: InputKind, output: OutputKind| {
            ops.push(Operation {
                name: name.to_string(),
                ffi_name: format!("ffi_{name}"),
                input,
                output,
                doc: format!("doc for {name}"),
            });
        };

        push("json_void", InputKind::Json, OutputKind::Void);
        push("bytes_void", InputKind::Bytes, OutputKind::Void);
        push("none_void", InputKind::None, OutputKind::Void);
        push("u32_void", InputKind::U32, OutputKind::Void);
        push("none_json", InputKind::None, OutputKind::Json);
        push("json_json", InputKind::Json, OutputKind::Json);
        push("bytes_json", InputKind::Bytes, OutputKind::Json);
        push("u32_json", InputKind::U32, OutputKind::Json);
        push("none_bytes", InputKind::None, OutputKind::Bytes);
        push("json_bytes", InputKind::Json, OutputKind::Bytes);
        push("bytes_bytes", InputKind::Bytes, OutputKind::Bytes);
        push("u32_bytes", InputKind::U32, OutputKind::Bytes);

        ApiSpec {
            namespace: "MessengerMls".to_string(),
            ffi_header: "messenger_mls.h".to_string(),
            operations: ops,
        }
    }

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
    fn util_helpers_work() {
        assert_eq!(
            parse_fn_name("pub extern \"C\" fn abc(x: i32) -> u32 {"),
            Some("abc")
        );
        assert_eq!(parse_fn_name("fn nope() {}"), None);

        assert_eq!(to_camel_case("hello_world"), "helloWorld");
        assert_eq!(to_pascal_case("hello_world"), "HelloWorld");
        assert_eq!(to_pascal_case(""), "");
        assert_eq!(escape_cpp_string("a\\\"b"), "a\\\\\\\"b");
    }

    #[test]
    fn collect_ffi_docs_extracts_preceding_blocks() {
        let dir = temp_dir("chat-core-sdk-docs");
        let file = dir.join("ffi.rs");
        fs::write(
            &file,
            r#"
/// First line
/// second line
#[unsafe(no_mangle)]
pub extern "C" fn messenger_mls_abc() -> u32 { 0 }

/// Another doc
pub extern "C" fn messenger_mls_def(x: i32) -> u32 { 0 }
"#,
        )
        .expect("write ffi fixture");

        let docs = collect_ffi_docs(&file).expect("collect docs");
        assert!(
            docs.get("messenger_mls_abc")
                .expect("abc doc")
                .contains("First line")
        );
        assert!(
            docs.get("messenger_mls_def")
                .expect("def doc")
                .contains("Another doc")
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn renderers_cover_all_input_output_shapes() {
        let spec = full_spec();

        let cpp = render_cpp(&spec);
        assert!(cpp.contains("class MessengerMls"));
        assert!(cpp.contains("jsonVoid"));
        assert!(cpp.contains("u32Bytes"));

        let go = render_go(&spec);
        assert!(go.contains("func (c *Client) JsonVoid"));
        assert!(go.contains("func (c *Client) U32Bytes"));

        let dart = render_dart(&spec);
        assert!(dart.contains("void jsonVoidSync"));
        assert!(dart.contains("Uint8List u32BytesSync"));
    }

    #[test]
    fn static_renderers_are_non_empty() {
        assert!(render_cpp_smoke().contains("messenger_mls::MessengerMls"));
        assert!(render_go_mod().contains("module messengermls"));
        assert!(render_go_smoke().contains("TestNewClose"));
        assert!(render_dart_pubspec().contains("messenger_mls_generated"));
    }
}
