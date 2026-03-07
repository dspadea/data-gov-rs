# Development Guide — data-gov-rs

## Project overview

Rust workspace with three crates:

- `data-gov-ckan` — async, type-safe CKAN client (low-level)
- `data-gov` — high-level client + CLI binary
- `data-gov-mcp-server` — MCP server for AI integration

Rust 2024 edition, MSRV **1.90**, Apache-2.0 license.

## Code quality gates

Every commit must pass all of these. CI enforces them; run locally before pushing.

```bash
cargo fmt --all -- --check                                # Formatting
cargo clippy --all-targets --all-features -- -D warnings  # Lint (warnings = errors)
cargo test --all-features                                 # All tests
cargo doc --all-features --no-deps                        # Rustdoc builds clean
```

### Warnings are fatal

The workspace treats all compiler and clippy warnings as errors (`-D warnings`).
Do not suppress warnings with `#[allow(...)]` unless there is a documented reason
in a comment on the same line. Fix the root cause instead.

### Code formatting

All code is formatted with `rustfmt` using default settings. No exceptions, no
overrides. Run `cargo fmt --all` before committing. CI rejects unformatted code.

## Documentation

### Rustdoc rules

1. **Every public item gets a doc comment.** `pub fn`, `pub struct`, `pub enum`,
   `pub trait`, `pub mod`, and `pub type` all require `///` doc comments. If
   clippy's `missing_docs` lint fires, add the doc — don't suppress it.

2. **Module-level docs** (`//!`) go at the top of each `lib.rs` and any module
   that represents a major subsystem. Explain *what* the module provides and
   *when* a consumer would use it.

3. **Doc comments describe the contract, not the implementation.** Say what the
   function does, what it returns, and when it errors. Don't narrate the code
   line by line.

4. **Use `# Examples` sections** for non-obvious public APIs. Mark examples
   `no_run` if they require network or filesystem access.

5. **`# Errors` section** on any function that returns `Result`. List the
   conditions that produce each error variant.

6. **`# Panics` section** if the function can panic (it usually shouldn't).

### Code comments

- **Don't comment obvious code.** `// increment counter` above `counter += 1` is noise.
- **Do comment *why*, not *what*.** If the reason for a block isn't obvious from
  the code itself, a short comment explaining the intent is valuable.
- **Mark workarounds** with `// HACK:` or `// WORKAROUND:` and a brief explanation
  so they can be found and revisited later.
- **No commented-out code.** Delete it; git remembers.

## Testing philosophy: TDD + specification-driven

### Write tests first

Every change — bug fix, new feature, refactor — starts with a failing test:

1. **Red** — Write a test that captures the expected behavior. Run it; confirm it fails.
2. **Green** — Write the minimum code to make the test pass.
3. **Refactor** — Clean up while keeping all tests green.

If you are fixing a bug, the first commit should be a test that reproduces it.

### Specification-driven tests

- **Name tests after the behavior they verify:**
  `test_search_with_empty_query_returns_all_datasets` not `test_search_3`.
- **Group tests by concern** using `mod tests` blocks or dedicated test files.
- **Assert on structure and invariants**, not exact external data that can change.

### Test organization

```
crate/
  src/
    module.rs          # Unit tests in #[cfg(test)] mod tests { ... } at bottom
  tests/
    fixtures/          # Captured JSON responses for mock-based tests
    feature_tests.rs   # Integration tests (cross-module, may use mocks)
    integration_*.rs   # Live API tests (run separately in CI)
```

- **Unit tests** (`cargo test --lib`): Fast, no network, no filesystem. Pure logic.
- **Fixture-based tests**: Use `wiremock` with captured API responses in `tests/fixtures/`.
  These verify deserialization and client logic without hitting the network.
- **Integration tests** (`cargo test --test <name>`): Hit the live data.gov API.
  Run in a separate CI job.
- **Ignored tests** (`#[ignore]`): Expensive or flaky. Run with `--ignored`.

### What to test

For every public function or method:

1. **Happy path** — normal inputs produce correct output
2. **Edge cases** — empty strings, zero/negative values, None/missing fields
3. **Error cases** — invalid input returns the correct error variant, not a panic
4. **Boundary conditions** — pagination limits, filename conflicts, path traversal

### Running tests

```bash
cargo test --lib --all-features           # Unit tests only (fast, no network)
cargo test --doc --all-features           # Doc tests
cargo test --test client_tests            # Fixture-based mock tests
cargo test --test integration_tests       # Live API tests
cargo test --test solr_syntax_tests -- --ignored  # Solr syntax (network)
```

## Error handling

### No `unwrap()` or `expect()` in library code

Library crates (`data-gov-ckan`, `data-gov`, `data-gov-mcp-server`) must not
use `.unwrap()` or `.expect()` in non-test code. Propagate errors with `?` or
convert them into the crate's error type. If a condition is truly unreachable,
use `unreachable!()` with a comment explaining why.

**Allowed uses of `unwrap`/`expect`:**
- Test code (`#[cfg(test)]`)
- One-time static initialization (e.g., `LazyLock` with infallible operations
  like compiling a known-good regex)
- CLI `main()` or top-level binary entry points where a panic is the intended
  behavior on misconfiguration

### Don't silently swallow errors

Never discard an error with `.ok()`, `let _ =`, or an empty `Err(_) => {}`
unless the error genuinely doesn't matter. If you can't propagate it, at
minimum log it (`tracing::warn!`, `eprintln!`). A silently swallowed error
is a debugging nightmare — the operation fails and nothing explains why.

**Acceptable silent discards:**
- Fire-and-forget side effects where failure is expected and harmless (e.g.,
  removing a temp file that may not exist)
- Logging calls themselves (if writing a log line fails, retrying won't help)

Everything else should either propagate (`?`), log, or surface to the user.

### Error messages

- **Be specific.** Say what went wrong and what was expected:
  `"invalid jsonrpc version: expected \"2.0\", got \"1.0\""` not `"bad version"`.
- **Include context.** Name the method, field, or value that caused the error:
  `"data_gov.search: missing parameters"` not `"missing parameters"`.
- **Don't dump internals.** Error messages are for consumers — omit stack
  traces, memory addresses, and internal type names. Keep them to one or two
  sentences.
- **Use error enums.** Each crate defines a clear error enum (e.g.,
  `ServerError`, `DataGovError`, `CkanError`). Map external errors with `#[from]`
  or explicit conversions — don't stringify them prematurely.

## Code organization

### Modularization principles

Code should be organized into **discrete, single-purpose modules and functions**
that are easy to read and reason about in isolation.

1. **One concern per file.** A file should have one clear responsibility. If you
   need a comment header like `// === Section ===` to separate unrelated logic,
   it's time to split.

2. **One concern per function.** A function should do one thing. If it has
   multiple levels of nesting, multiple sequential phases, or you're tempted to
   add section comments inside it — extract helper functions.

3. **No file exceeds 1000 lines.** If a file approaches this limit, split it.
   Prefer many small files over few large ones.

4. **Public API surface is intentional.** Only `pub` what consumers need.
   Internal helpers should be `pub(crate)` or private. A module's public items
   are its contract — keep it narrow.

5. **Flat over deep.** Prefer `mod foo; mod bar;` siblings over deep nesting.
   One level of `submodule/` is fine; two levels is a smell.

### File layout convention

Within a single `.rs` file, order items as:

1. Module-level doc comment (`//!`)
2. `use` imports (stdlib, then external crates, then `crate::`/`super::`)
3. Constants and type aliases
4. Structs and enums (with their `impl` blocks immediately after each)
5. Trait definitions
6. Trait implementations
7. Free functions
8. `#[cfg(test)] mod tests { ... }` at the bottom

## Dependencies

### Use latest stable versions

Keep dependencies at their **latest stable release** unless a specific version
is required for MSRV compatibility. Run `cargo outdated --root-deps-only`
periodically and update proactively.

```bash
cargo update                       # Apply semver-compatible updates
cargo outdated --root-deps-only    # Check for new major versions
cargo audit                        # Check for security advisories
```

### Current state (as of 2026-03)

- **reqwest** `0.13.x` — note that `query` is now an explicit feature in 0.13
- **serde** `1.0.x`, **tokio** `1.x`, **clap** `4.5.x`, **thiserror** `2.0.x`

### Dependency hygiene

1. **Pin to the narrowest range that works.** `"^X.Y"` for external crates.
   Never `"*"`.
2. **One version per dependency across the workspace.** Use
   `[workspace.dependencies]` in the root `Cargo.toml` to centralize versions
   when practical.
3. **Run `cargo audit` before releasing.** CI does this automatically.
4. **Test after every update.** `cargo test --all-features` and
   `cargo clippy --all-targets`.

### MSRV constraint

The workspace targets **Rust 1.90**. All dependency versions must be compatible.
The `Cargo.lock` is committed and tested in CI against stable, beta, and MSRV.

## Security checklist

Before any release:

- [ ] `cargo audit` passes
- [ ] User-supplied paths are sanitized via `data_gov::util::sanitize_path_component()`
- [ ] MCP `output_dir` parameter rejects `..`
- [ ] No secrets (API keys, tokens) in logs or error messages
- [ ] Download URLs are not constructed from unvalidated user input

## CI pipeline

GitHub Actions CI (`.github/workflows/ci.yml`) runs:

1. **Format check** (`cargo fmt`, stable only)
2. **Clippy** with `-D warnings` (stable only)
3. **Build** on stable, beta, and MSRV 1.90
4. **Unit tests** (`--lib`)
5. **Doc tests** (`--doc`)
6. **Integration tests** (separate job, stable only)
7. **Documentation build**
8. **Security audit** (`cargo audit`)

All checks must pass before merging to `main`.
