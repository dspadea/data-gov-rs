# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html)
(0.x releases may include breaking changes on minor bumps).

## [0.4.0] - 2026-03-07

### Breaking

- **Removed `Default` impl for `DataGovClient`** — use `DataGovClient::new()?`
  instead. The previous impl could panic if the HTTP client failed to build.
- **Upgraded reqwest from 0.12 to 0.13** across `data-gov-ckan` and `data-gov`.
  If you depend on reqwest types re-exported from these crates, you may need to
  update your own reqwest dependency.
- **`rustls-tls` feature** in `data-gov-ckan` now maps to `reqwest/rustls`
  (was `reqwest/rustls-tls`). The feature name on `data-gov-ckan` is unchanged;
  only the underlying reqwest feature differs.
- Default user-agent string now reflects the actual crate version
  (`data-gov-rs/0.4.0`) instead of the previously hardcoded `data-gov-rs/1.0`.

### Added

- **Comprehensive test suite** — 130+ tests across the workspace:
  - 21 wiremock-based unit tests for all CKAN client endpoints
    (`data-gov-ckan/tests/unit_tests.rs`)
  - 38 unit tests for the MCP server's pure functions, serialization, tool
    specs, and error codes (spread across `types.rs`, `tools.rs`, `server.rs`)
  - 11 fixture-based tests for the high-level `DataGovClient` using captured
    API responses (`data-gov/tests/client_tests.rs`)
  - 5 tests for path sanitization (`data-gov/src/util.rs`)
- `DataGovClient::config()` — read access to the current configuration.
- `DataGovConfig::with_base_url()` — override the CKAN API base URL (useful
  for testing with mock servers).
- `data_gov::util::sanitize_path_component()` — shared path sanitization
  function used by the CLI and MCP server.
- `CLAUDE.md` development guide covering TDD workflow, file organization,
  dependency management, and security checklist.
- `CHANGELOG.md` (this file).
- `Cargo.lock` is now committed for reproducible binary builds.

### Fixed

- **Parallel download progress bars** — replaced independent `ProgressBar`
  instances with `indicatif::MultiProgress` so concurrent downloads render
  correctly instead of overwriting each other.
- **UTF-8 string truncation panic** — `&notes[..100]` byte slicing replaced
  with `chars().take(100)` in three locations to prevent panics on multi-byte
  characters.
- **`setdir` REPL command discarded user config** — now clones the existing
  configuration instead of creating a fresh default, preserving API key,
  timeouts, and other settings.
- **`output_dir` MCP parameter path traversal** — rejects paths containing
  `..` to prevent writing outside the intended directory.
- **Download progress per-chunk cloning** — `DownloadProgress` struct is now
  constructed once before the download loop; only `downloaded_bytes` is updated
  per chunk.

### Changed

- **CKAN client refactored** — extracted `call_action<T>` generic helper,
  reducing `client.rs` from ~1243 to ~771 lines and eliminating 10 copies of
  HTTP boilerplate. Uses reqwest's `.query()` instead of manual URL encoding,
  removing the `urlencoding` dependency.
- **MCP server tool specs** — converted from a function returning `Vec<ToolSpec>`
  to a `static TOOL_SPECS: LazyLock<Vec<ToolSpec>>` (allocated once).
- **JSON-RPC version validation** — the MCP server now rejects requests where
  `jsonrpc` is present but not `"2.0"`.
- **CLI version** — now uses `env!("CARGO_PKG_VERSION")` instead of hardcoded
  `"1.0"`.
- **`download-dir` CLI flag** — removed magic string default detection;
  the flag is now purely optional.
- Path sanitization logic deduplicated into `data_gov::util`.
- **MCP server modularized** — split monolithic `server.rs` (1548 lines) into
  four focused modules: `server.rs` (run loop), `types.rs` (request/response
  types and param structs), `tools.rs` (tool specs and lookup), `handlers.rs`
  (method dispatch and handler logic).

### Removed

- `urlencoding` dependency from `data-gov-ckan`.
- `extern crate` declarations from `data-gov-ckan/src/lib.rs` (unnecessary
  since Rust 2018).
- Crate-level `#![allow(unused_imports)]` from `data-gov-ckan`; 17 unused
  imports cleaned up.

### Infrastructure

- Updated `actions/cache` from v3 to v4 in CI and release workflows.
- Replaced deprecated `actions/create-release@v1` with
  `softprops/action-gh-release@v2` in release workflow.

## [0.3.1] - 2025-12-15

Previous release. See git history for details.
