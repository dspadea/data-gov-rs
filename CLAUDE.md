# Development Guide — data-gov-rs

## Project overview

Rust workspace with four crates:

- `data-gov-catalog` — async client for the data.gov Catalog API (current
  backend; DCAT-US 3, cursor-paginated)
- `data-gov` — high-level client + CLI binary (built on `data-gov-catalog`)
- `data-gov-mcp-server` — MCP server for AI integration
- `data-gov-ckan` — async CKAN Action API client. data.gov retired its CKAN
  endpoint in 2026; this crate is retained as a general-purpose client for
  other CKAN-compatible portals (European, state, municipal, university).

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

Library crates (`data-gov-catalog`, `data-gov-ckan`, `data-gov`, `data-gov-mcp-server`) must not
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

### No `unsafe`

This project has no need for `unsafe`. Do not add `unsafe` blocks, `unsafe fn`,
or `unsafe impl`. If a dependency requires unsafe at its boundary, wrap it in a
safe abstraction — but that situation should not arise here.

### No blocking in async contexts

Never call blocking operations (synchronous I/O, `std::thread::sleep`,
`Mutex::lock` on a long-held lock, CPU-heavy computation) inside an `async fn`
or while a tokio runtime is active. Blocking the executor starves other tasks.

- Use `tokio::fs` instead of `std::fs` in async code.
- Use `tokio::time::sleep` instead of `std::thread::sleep`.
- If you must run blocking code, use `tokio::task::spawn_blocking`.
- The REPL uses `Runtime::block_on` at the top level — that's fine since it
  owns the runtime. Don't nest `block_on` inside an already-async context.

## Rust idioms

### Prefer borrowing over cloning

Accept `&str` instead of `String`, `&[T]` instead of `Vec<T>`, and `&Path`
instead of `PathBuf` in function parameters when the function doesn't need
ownership. Clone only when you genuinely need an independent owned copy.

Common smells:
- `.clone()` immediately before passing to a function — the function should
  probably take a reference instead.
- `.to_string()` on a `&str` just to satisfy a `String` parameter — change the
  parameter.
- Cloning inside a loop when a reference would work.

### Logging discipline

The workspace uses `tracing`. Use the right level:

| Level   | When to use                                                     |
|---------|-----------------------------------------------------------------|
| `error` | Something failed and the operation cannot continue.             |
| `warn`  | Something is wrong but the operation can recover or degrade.    |
| `info`  | High-level lifecycle events: server started, request completed. |
| `debug` | Detailed internal state useful for development debugging.       |
| `trace` | Very fine-grained: loop iterations, individual field values.    |

**Rules:**
- Never log secrets (API keys, tokens, passwords) at any level.
- Log at `warn` or `error` when discarding an error you can't propagate.
- Don't log in hot paths at `info` or above — keep `info` to startup, shutdown,
  and per-request summaries, not per-chunk download progress.
- Structured fields (`tracing::info!(method = %name, "request handled")`) are
  preferred over string interpolation.

## Versioning

### Semver discipline

Follow [Semantic Versioning](https://semver.org/). While the project is `0.x`,
minor bumps may include breaking changes, but still be intentional about it.

| Change type                                    | Bump    |
|------------------------------------------------|---------|
| Breaking API change (removed/renamed pub item) | Minor   |
| New public API, new feature, new dependency     | Minor   |
| Bug fix, internal refactor, doc improvement    | Patch   |
| Security fix                                   | Patch   |

**Checklist before a version bump:**
- Update version in all four `Cargo.toml` files (`data-gov-catalog`,
  `data-gov-ckan`, `data-gov`, `data-gov-mcp-server`).
- Update inter-crate dependency versions if they reference each other.
- Update version strings in README dependency snippets.
- Add a `CHANGELOG.md` entry under the new version.
- Tag the release after merging to `main`.

### Release branches

A release is assembled on a **version integration branch**, not merged piecemeal
to `main`:

1. Cut `release/X.Y.Z` from `main`.
2. Do each work item on its own branch off the release branch.
3. Merge finished items into the release branch — never directly into `main`.
4. Validate the release as a whole.
5. Tag and publish, then merge the release branch to `main`.

`main` therefore always reflects a released state, and a partially-landed release
never sits on the default branch.

### Changelog

`CHANGELOG.md` follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Every change that a consumer could notice — API changes, bug fixes, behavioural
changes, CLI or MCP surface changes — **adds an entry under the current
`## [X.Y.Z] - Unreleased` heading in the same PR that makes the change.** Purely
internal refactors with no observable effect may be omitted.

Group entries under `Breaking`, `Added`, `Fixed`, `Changed`, `Removed`,
`Deprecated`, or `Infrastructure`. Reference the issue number where one exists.
On release, replace `- Unreleased` with the release date and open a fresh
`## [X.Y.Z] - Unreleased` heading above it.

### Dependency freshness and advisories

Every release must ship with dependencies current and free of significant
vulnerabilities. Before validating a release, run **both** halves:

```bash
cargo update                       # Semver-compatible moves
cargo outdated --root-deps-only    # Major-version drift needing a decision
cargo audit                        # RustSec advisories
```

plus an **OSV/GHSA scan of the lockfile**. `cargo audit` alone is not sufficient:
in the 2026-07 review it reported two advisories while OSV surfaced seven further
`openssl` CVEs (five High) that are GHSA-only and absent from the RustSec
database. Re-run at release time, not just at the start of the work — advisories
land after the fact, so a clean run expires.

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

GitHub Actions CI (`.github/workflows/ci.yml`) runs on pushes and pull requests
targeting `main` **and any `release/**` branch**, so release-branch work is gated
the same way `main` is.

**Test Suite** (matrix: stable, beta, MSRV 1.90)
1. **Format check** (`cargo fmt --all -- --check`, stable only)
2. **Clippy** with `-D warnings` (stable only)
3. **Build** (`--all-features --workspace`)
4. **Tests** (`cargo test --all-features --workspace`)

Test selection matters here. `--workspace` covers unit tests, every `tests/`
integration target, and doc tests. Selecting `--lib` instead silently skips both
binary crates (`data-gov-mcp-server` has no `lib.rs`; the CLI is a `[[bin]]`) and
every `tests/` target — 37 of 181 tests would run, while `clippy --all-targets`
still compiles the rest so nothing looks wrong. Do not narrow this back.

**Other jobs**
5. **Examples** — compiles all workspace examples (they make real API calls, so
   they are built but never run)
6. **Documentation build** (`cargo doc --no-deps`)
7. **Security audit** — `cargo audit` *plus* an OSV/GHSA lockfile scan. Both are
   required; see the dependency-freshness section for why `cargo audit` alone is
   insufficient.
8. **Live API Tests** — opt-in via `workflow_dispatch`, runs the `#[ignore]`d
   network tests. Deliberately not gating: a data.gov outage must not turn PRs
   red.

All gating checks must pass before merging.
