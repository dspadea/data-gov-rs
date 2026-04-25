# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html)
(0.x releases may include breaking changes on minor bumps).

## [Unreleased]

### Breaking — Catalog API migration

- **data.gov retired its CKAN Action API.** The workspace now targets the new
  [Catalog API](https://resources.data.gov/catalog-api/) (cursor-paginated,
  DCAT-US 3 payloads, no API keys).
- **New `data-gov-catalog` crate** replaces `data-gov-ckan` as the backend
  for `data-gov` and `data-gov-mcp-server`. The CKAN crate is retained as a
  general-purpose client for other CKAN-compatible portals, but is no longer
  used by data.gov.
- **`DataGovClient::search` signature** changed: `offset`/`format` parameters
  removed; a cursor-based `after: Option<&str>` replaces offset. Returns
  `SearchResponse` (from the catalog crate) instead of CKAN's
  `PackageSearchResult`.
- **`DataGovClient::get_dataset(slug)`** now returns a `SearchHit` (not
  `Package`) and resolves strictly by slug; harvest-record UUIDs go through
  the new `get_dataset_by_harvest_record(id)`.
- **`DataGovClient::download_resources`** renamed to `download_distributions`
  and takes `&[Distribution]`. `download_resource` → `download_distribution`.
- **`DataGovClient::get_downloadable_resources`** renamed to
  `get_downloadable_distributions` and takes `&Dataset`.
- **`DataGovClient::get_resource_filename`** renamed to
  `get_distribution_filename`.
- **`DataGovClient::ckan_client()`** replaced by `catalog_client()`.
- **`DataGovConfig::with_api_key` removed** — the Catalog API is unauthenticated.
- **`data_gov::ckan` re-export** replaced by `data_gov::catalog`.
- **`DATA_GOV_BASE_URL`** constant now points at `https://catalog.data.gov`
  (was `https://catalog.data.gov/api/3`).
- **CLI `--api-key` flag removed** (Catalog API is public).
- **MCP server** drops the `ckan.packageSearch`, `ckan.packageShow`, and
  `ckan.organizationList` tools. The `data_gov.search` params lost `offset`
  and `format`; the new `after` cursor and `organizationContains` client-side
  filter remain. `data_gov.downloadResources` replaces `resourceIds` with
  `distributionIndexes`; the `formats` filter is now matched client-side
  against both `format` and `mediaType`.

### Added

- **`data-gov-catalog`** — new crate wrapping the Catalog API with typed
  models for DCAT-US 3 (`Dataset`, `Distribution`, `Publisher`,
  `ContactPoint`), search envelopes (`SearchResponse`, `SearchHit`),
  organizations, keywords, locations, and harvest records. Endpoint coverage:
  `/search` (with `SearchParams` builder), `/api/organizations`,
  `/api/keywords`, `/api/locations/search`, `/api/location/{id}`,
  `/harvest_record/{id}`, `/harvest_record/{id}/raw`,
  `/harvest_record/{id}/transformed`.

### Deprecated

- **`data-gov-ckan`** crate-level docs and README now note that data.gov no
  longer uses CKAN. The crate remains published and functional for use against
  other CKAN-compatible instances (European, state, municipal, university
  portals).

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
