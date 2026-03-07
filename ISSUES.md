# Code Review Issues

Findings from a full code review of the data-gov-rs workspace, organized by priority.

## Critical

### 1. ~~Massive HTTP boilerplate duplication in CKAN client~~ ✅ FIXED

Extracted `call_action<T>` generic helper. File reduced from ~1243 to ~771 lines.
Also eliminated `urlencoding` dependency by using reqwest's `.query()`.

---

### 2. ~~No unit tests for CKAN client endpoints~~ ✅ FIXED

Added 21 unit tests in `data-gov-ckan/tests/unit_tests.rs` using `wiremock`.
Covers all endpoints, URL construction, response parsing, and all error paths.

---

### 3. ~~No tests for MCP server~~ ✅ FIXED

Added 38 unit tests in `data-gov-mcp-server/src/server.rs` `#[cfg(test)]` module.
Covers all pure functions, request/response serialization, tool specs, and error codes.

---

## Bugs

### 4. ~~UTF-8 string truncation at byte boundary~~ ✅ FIXED

Changed `&notes[..100]` to `notes.chars().take(100).collect()` in all 3 locations:
- `data-gov/tools/cli/ui/handlers.rs`
- `data-gov/tools/cli/ui/display.rs`
- `data-gov/examples/demo.rs`

---

### 5. ~~`setdir` discards user config~~ ✅ FIXED

Added `config()` getter to `DataGovClient`. `handle_setdir` now clones the existing
config instead of creating a fresh `DataGovConfig::new()`.

---

## Security

### 6. ~~`output_dir` MCP parameter not validated for path traversal~~ ✅ FIXED

Added `..` check on `output_dir` parameter in `server.rs`, returning
`InvalidParams` error if detected.

---

## Medium

### 7. ~~Duplicated path sanitization logic (3 copies)~~ ✅ FIXED

Extracted `data_gov::util::sanitize_path_component()` with unit tests.
CLI handlers and MCP server both use the shared function now.

---

### 8. ~~Hand-rolled URL building instead of using reqwest~~ ✅ FIXED

Fixed as part of Critical #1 — `call_action` uses `.query()`.

---

### 9. ~~Panicking `Default` impl for `DataGovClient`~~ ✅ FIXED

Removed the `Default` impl entirely. It was unused.

---

### 10. ~~Hardcoded version strings~~ ✅ FIXED

CLI version now uses `env!("CARGO_PKG_VERSION")`. User agent strings in both
`data-gov-ckan` and `data-gov` use `concat!("data-gov-rs/", env!("CARGO_PKG_VERSION"))`.

---

### 11. ~~Missing `Cargo.lock`~~ ✅ FIXED

Removed `Cargo.lock` from `.gitignore`. The lock file exists and should be committed
since the workspace produces binaries.

---

### 12. ~~Magic string detection for `download-dir` default~~ ✅ FIXED

Removed the `default_value("./downloads")` and the `!= "./downloads"` string
comparison. Now uses `Option`-based detection — `with_download_dir` is only called
if the user explicitly provides the flag.

---

## Minor

### 13. ~~`tool_specs()` allocates on every call~~ ✅ FIXED

Converted to `static TOOL_SPECS: LazyLock<Vec<ToolSpec>>`. Callers now borrow
from the static. `find_tool_spec` and `find_tool_spec_by_method` return
`Option<&'static ToolSpec>`.

---

### 14. ~~Unnecessary `extern crate` declarations~~ ✅ FIXED

Removed all four `extern crate` declarations from `data-gov-ckan/src/lib.rs`.

---

### 15. ~~Overly broad `#[allow(unused_imports)]`~~ ✅ FIXED

Removed crate-level `#![allow(unused_imports)]` and cleaned up 16 unused
`use crate::models;` imports across model files, plus unused `Serialize`/`Value`
imports in `client.rs`.

---

### 16. ~~Download progress events clone on every chunk~~ ✅ FIXED

Moved `DownloadProgress` construction before the loop. Only `downloaded_bytes`
is updated per chunk — no more cloning strings or paths on every iteration.

---

### 17. ~~`_jsonrpc` field parsed but not validated~~ ✅ FIXED

Renamed field from `_jsonrpc` to `jsonrpc`. Added validation in `handle_request`:
if present and not `"2.0"`, returns `InvalidRequest` error. Missing field is
still accepted (lenient for MCP clients).

---

### 18. ~~`actions/cache@v3` in CI is outdated~~ ✅ FIXED

Updated to `actions/cache@v4` across both `ci.yml` and `release.yml`.

---

### 19. ~~`actions/create-release@v1` in release workflow is deprecated~~ ✅ FIXED

Replaced with `softprops/action-gh-release@v2`.
