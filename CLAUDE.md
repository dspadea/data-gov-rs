# Development Guide — data-gov-rs

## Project overview

Rust workspace with three crates:

- `data-gov-ckan` — async, type-safe CKAN client (low-level)
- `data-gov` — high-level client + CLI binary
- `data-gov-mcp-server` — MCP server for AI integration

Rust 2024 edition, MSRV **1.90**, Apache-2.0 license.

## Testing philosophy: TDD + specification-driven

### Write tests first

Every change — bug fix, new feature, refactor — starts with a failing test. The sequence is:

1. **Red** — Write a test that captures the expected behavior. Run it; confirm it fails.
2. **Green** — Write the minimum code to make the test pass.
3. **Refactor** — Clean up while keeping all tests green.

Do not skip step 1. If you are fixing a bug, the first commit should be a test that reproduces it.

### Specification-driven tests

Tests should document *what* the code does, not *how*. Structure tests around the public API contract:

- **Name tests after the behavior they verify**, not the function they call:
  `test_search_with_empty_query_returns_all_datasets` not `test_search_3`.
- **Group tests by concern** using `mod tests` blocks within the module or dedicated test files.
- **Prefer property-based assertions** over exact value checks when the data is external (e.g., data.gov results). Assert on structure, types, invariants — not on specific dataset names that can change.

### Test organization

```
crate/
  src/
    module.rs          # Unit tests in #[cfg(test)] mod tests { ... } at bottom
  tests/
    feature_tests.rs   # Integration tests that exercise cross-module behavior
    integration_*.rs   # Tests that hit the live API (run separately in CI)
```

- **Unit tests** (`cargo test --lib`): Fast, no network, no filesystem side effects. Use these for pure logic — parsing, filtering, filename generation, command parsing, error construction.
- **Integration tests** (`cargo test --test <name>`): May use the network or filesystem. Keep these in `tests/` directories. The CI workflow runs them in a separate job.
- **Ignored tests** (`#[ignore]`): For expensive or flaky network tests. Run with `--ignored` flag.

### What to test

For every public function or method, test:

1. **Happy path** — normal inputs produce correct output
2. **Edge cases** — empty strings, zero/negative values, None/missing optional fields
3. **Error cases** — invalid input returns the correct error variant, not a panic
4. **Boundary conditions** — pagination limits, filename conflicts, path traversal attempts

For the CKAN client specifically, test:
- Query parameter encoding (Solr special characters, Unicode)
- Response deserialization for each endpoint (use recorded/mock JSON fixtures where possible)
- Error response parsing (404, 403, 500, malformed JSON)
- Authentication header construction (API key, bearer token, basic auth)

### Running tests

```bash
cargo test --lib --all-features           # Unit tests only (fast, no network)
cargo test --doc --all-features           # Doc tests
cargo test --test integration_tests       # Live API integration tests
cargo test --test solr_syntax_tests -- --ignored  # Solr syntax tests (network)
cargo clippy --all-targets --all-features -- -D warnings  # Lint
cargo fmt --all -- --check                # Format check
```

## Code organization: single concern, under 1000 lines

### File size rule

No source file should exceed **1000 lines**. If a file grows past this limit, split it. Current files near the limit that should be watched:

| File | Lines | Action needed |
|------|-------|---------------|
| `data-gov-ckan/src/client.rs` | ~1243 | **Split now** — extract endpoint methods into separate modules (e.g., `endpoints/search.rs`, `endpoints/autocomplete.rs`, `endpoints/organization.rs`) |
| `data-gov-mcp-server/src/server.rs` | ~1128 | **Split now** — extract into `types.rs` (request/response/param structs), `tools.rs` (tool specs/descriptors), `handlers.rs` (method dispatch logic) |

All other files are well under the limit.

### Single concern per file

Each file should have one clear responsibility. Signs a file needs splitting:

- Multiple `impl` blocks for unrelated types
- A mix of struct definitions, trait impls, and business logic
- More than ~3 distinct "sections" separated by comment headers
- You need to scroll past type definitions to find the actual logic

### Suggested splits

**`data-gov-ckan/src/client.rs`** (1243 lines) should become:

```
data-gov-ckan/src/
  client.rs           # CkanClient struct, Configuration, new/default, shared helpers
  endpoints/
    mod.rs
    search.rs         # package_search, package_show, package_list
    autocomplete.rs   # dataset_, tag_, user_, group_, org_, format_ autocomplete
    organization.rs   # organization_list, group_list
    status.rs         # status_show, site_read
```

**`data-gov-mcp-server/src/server.rs`** (1128 lines) should become:

```
data-gov-mcp-server/src/
  server.rs           # DataGovMcpServer struct, run loop, bootstrap
  types.rs            # Request, Response, ResponseError, all *Params structs, ServerError
  tools.rs            # ToolSpec, ToolDescriptor, ToolResponse, tool_specs(), tool_descriptors()
  handlers.rs         # invoke_method dispatch, helper methods (to_dataset_summary, etc.)
```

## Dependencies: keeping current

### MSRV constraint

The workspace targets **Rust 1.90** (`rust-version = "1.90"` in each Cargo.toml). All dependency versions must be compatible with this MSRV. The `Cargo.lock` is committed and tested in CI against stable, beta, and the MSRV.

### Updating dependencies

```bash
# See what's outdated within semver-compatible ranges:
cargo update --dry-run

# Apply compatible updates:
cargo update

# Check for newer major versions (review changelogs before bumping):
cargo install cargo-outdated  # if not installed
cargo outdated --root-deps-only
```

### Current dependency notes (as of 2026-03)

- **reqwest** is pinned to `0.12.x` across the workspace. Version `0.13.x` exists but may require a higher MSRV. Before bumping, verify MSRV compatibility and test on all three CI matrix entries.
- **serde** `1.0.x` — keep at latest patch. Versions vary slightly across crates (`1.0.226` vs `1.0.228`); normalize to the same version workspace-wide.
- **tokio** `1.x` — keep at latest patch. The `data-gov` crate pins `1.47.1` while `data-gov-ckan` pins `1.48.0` in dev-deps; normalize.
- **clap** `4.5.x`, **colored** `3.0.x`, **thiserror** `2.0.x` — all current.

### Dependency hygiene rules

1. **Pin to the narrowest range that works.** Use `"X.Y.Z"` for workspace crates, `"^X.Y"` for external crates, never `"*"`.
2. **One version per dependency across the workspace.** If two crates use `serde`, they should agree on the minimum version. Consider using `[workspace.dependencies]` in the root `Cargo.toml` to centralize versions.
3. **Run `cargo audit` before releasing.** The CI already does this. Fix or document any advisories before tagging a release.
4. **Review changelogs for minor bumps.** Even semver-compatible updates can change behavior. Read the changelog, especially for `reqwest`, `tokio`, and `serde`.
5. **Test after every update.** Run `cargo test --all-features` and `cargo clippy --all-targets` after updating `Cargo.lock`.

### Workspace dependency centralization (recommended)

To avoid version drift, move shared dependencies to the workspace root:

```toml
# Cargo.toml (workspace root)
[workspace.dependencies]
serde = { version = "1.0.228", features = ["derive"] }
serde_json = "1.0"
tokio = { version = "1.48", features = ["full"] }
reqwest = { version = "0.12.28", default-features = false }
thiserror = "2.0"
futures = "0.3"
```

Then in each crate's `Cargo.toml`:

```toml
[dependencies]
serde = { workspace = true }
```

## Security checklist

Before any release, verify:

- [ ] `cargo audit` passes with no unaddressed advisories
- [ ] All user-supplied dataset IDs and filenames are sanitized before use in filesystem paths (the `sanitize_dataset_id` pattern in `handlers.rs` and `server.rs`)
- [ ] No secrets (API keys, tokens) are logged or included in error messages
- [ ] Download URLs are not constructed from user input without validation
- [ ] The `sanitized_message()` method on `DataGovError` strips filesystem paths before exposing errors externally

## CI pipeline

The GitHub Actions CI (`.github/workflows/ci.yml`) runs:

1. **Format check** (stable only)
2. **Clippy** with `-D warnings` (stable only)
3. **Build** on stable, beta, and MSRV 1.90
4. **Unit tests** (`--lib`)
5. **Doc tests** (`--doc`)
6. **Integration tests** (separate job, stable only)
7. **Documentation build**
8. **Security audit** (`cargo audit`)

All checks must pass before merging to `main`.
