# Development Guide - data-gov-rs

## Project overview

Read [AGENTS.md](./AGENTS.md) first. It states what this project is for, which
crate owns what, and the invariants a change must not break. This file covers
how to work here; AGENTS.md covers what the work is for.

Rust 2024 edition, MSRV **1.90**, Apache-2.0 license.

## Code quality gates

```bash
just check      # The full gate: format, lint, build, test, rustls, examples, docs
just            # List every recipe
```

**`just check` is the gate.** CI calls the same recipes from the same
`justfile`, so a green run locally and a green pipeline mean the same thing and
cannot drift into two dialects of the same command. A fresh clone needs `just`
and a Rust 1.90 toolchain, and nothing else.

Keep recipes short. When one grows past a few lines, move the body into
`scripts/` and call the script from the recipe.

### Warnings are fatal

The workspace denies all compiler and clippy warnings through a
`[workspace.lints]` table in the root manifest, inherited by every member via
`[lints] workspace = true`. A plain `cargo build` hard-fails on a warning, with
no flag to remember and no way to be outside the gate.

**Mechanical beats vigilant, and this is the general rule, not a detail about
lints.** A standard that depends on someone remembering it is not enforced; it
is merely written down. When you find yourself writing "always pass `-D
warnings`" or "remember to run X", ask what configuration would make the
reminder unnecessary.

Do not suppress warnings with `#[allow(...)]` unless there is a documented
reason in a comment on the same line. Fix the root cause instead. A pre-existing
warning in a file you touch is yours to clear, not to build on.

Never `--no-verify` past a hook. If a hook is genuinely broken, fix it in its own
change and say why.

### `--all-features` does not test every configuration

**A feature that is mutually exclusive in practice is not covered by
`--all-features`, and needs its own gate step naming the feature.**

Concretely: `data-gov-catalog` and `data-gov-ckan` each expose `native-tls` and
`rustls-tls`. `--all-features` enables both at once, linking `native-tls` *and*
`rustls` simultaneously - a configuration no consumer ever selects. The
rustls-only build, which consumers do select, is never compiled or tested by the
gate at all. A single-feature build can fail on a missing import, a `cfg` typo,
or a dependency that only one backend pulls in, and nothing here would notice.

`just check-rustls` builds that configuration, and `just check` runs it.

So when adding a feature, ask whether `--all-features` genuinely exercises it or
merely compiles a superset, and add a recipe naming the feature when the answer
is the latter.

### Code formatting

All code is formatted with `rustfmt` using default settings. No exceptions, no
overrides. Run `cargo fmt --all` before committing. CI rejects unformatted code.

## Documentation

### Rustdoc rules

1. **Every public item gets a doc comment.** `pub fn`, `pub struct`, `pub enum`,
   `pub trait`, `pub mod`, and `pub type` all require `///` doc comments. If
   clippy's `missing_docs` lint fires, add the doc - don't suppress it.

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

Every change - bug fix, new feature, refactor - starts with a failing test:

1. **Red** - Write a test that captures the expected behavior. Run it; confirm it fails.
2. **Green** - Write the minimum code to make the test pass.
3. **Refactor** - Clean up while keeping all tests green.

If you are fixing a bug, the first commit should be a test that reproduces it.
**Commit the failing tests before the implementation - that commit is the
spec.**

Two habits that separate a real test from a hopeful one:

- **Assert the desired outcome, not that a call returned `Ok`.** A test that
  only checks the absence of an error passes for an implementation that does
  nothing.
- **Enumerate the unhappy paths deliberately**: empty and missing input,
  boundary and maximum values, concurrent or racy access, partial reads and
  dropped streams, malformed input, and the not-found case. A test list with
  none of these is testing wishes.

### Test conditions come from the real world, never from the code

**Derive every assertion from the specification or from real API data. Never from
what the implementation currently does.** A test written by reading the code
cannot detect a wrong implementation, because it was copied from one - it
converts a bug into a guarantee and makes the fix look like the regression.

This is not hypothetical. Two examples from this workspace:

- The MCP server emitted a `{"type":"json"}` content block, which is not a
  member of MCP's closed content union. Two tests asserted that exact shape, so
  a non-conformant server passed its own suite for as long as it existed.
- `initialize` omitted the required `protocolVersion`, making the server
  unusable from any spec-compliant client. Its test asserted
  `serverInfo.is_some() || protocolVersion.is_some()` - and `serverInfo` is
  always present.

So: open the spec, or capture a real response, and assert what *that* says. If
the implementation disagrees, the test is doing its job.

### Every test must be able to fail - prove it, don't reason about it

**Revert the fix and watch the suite go red. Record the mutations in the commit
message.** Do not settle for asking yourself what a wrong implementation would
look like.

That distinction is the whole rule, and it is written from failure. An earlier
version of this section asked you to name "the simplest wrong implementation
that still passes." Ten tests were written against that rule on the day it was
added, and an adversarial review defeated all ten. Introspection does not work
here: having just written the correct implementation, your model of *wrong* is
anchored to it. You imagine the mistakes you nearly made, not the ones you
cannot see.

Running the mutation takes about thirty seconds and does not care what you
imagined.

```
1. Commit the fix and its tests.
2. Revert the fix in the working tree, leaving the tests.
3. Run the suite. It MUST fail, and the failure MUST name the right test.
4. git reset --hard and move on.
```

Three ways this goes wrong, each of which happened:

- **Mutating a dirty tree destroys work.** Reverting a mutation with
  `git checkout -- file` also reverts uncommitted edits in that file. Commit
  first. Step 1 is not optional.
- **A mutation that silently fails to apply is indistinguishable from a test
  that fails to catch it.** A `sed` whose pattern spanned a newline matched
  nothing, and "202 passed" read exactly like an uncaught mutation. Confirm the
  mutation is present in the file before trusting a green run.
- **A defensive guard can mask the bug its own test targets.** `from_value`
  quietly dropped a non-object `structuredContent`, so a handler emitting a bare
  array produced valid output and the test that existed to catch it stayed
  green. If a guard makes bad input safe, a test downstream of the guard cannot
  see the upstream defect - assert on what the *producer* emitted.

Vacuity patterns that have all appeared here:

| Pattern | Why it cannot fail |
|---|---|
| `assert!(a.is_some() \|\| b.is_some())` | Short-circuits on an always-present `a`; `b` is never checked |
| Asserting presence rather than value | Satisfied by any hardcoded constant |
| wiremock asserting a request was *sent* | Says nothing about whether the response was *understood* - a field that silently deserializes to `None` passes |
| A mock body with only the fields the code reads | Too minimal for a deserialization bug to surface |
| Asserting a count or a shape the code just produced | Restates the implementation |

The wiremock case is worth calling out because the whole `client_tests.rs`
suite has it: those tests verify request shaping, and cannot catch a model that
drops a field. That is why `fixture_parity_tests.rs` exists separately - it
deserializes captured responses and asserts the fields actually arrive.

### Verify across the whole set, never one instance

**Drive assertions from the registry, not from a hand-picked example**, so that
a new member fails the test rather than being silently skipped. Where a list
exists in code - `TOOL_SPECS`, `SUPPORTED_PROTOCOL_VERSIONS` - iterate it.

One-instance verification failed three separate times in a single review, and it
looks like diligence every time:

| What was checked | What was actually true |
|---|---|
| `data_gov_search` returns an object, so tool results are conformant | 2 of the 5 tools returned a bare array |
| `org_slug=noaa-gov` returns nothing, so the filter is broken | The real slug is `noaa`; the filter works perfectly |
| `LATEST == SUPPORTED.last()`, so the newest is advertised | `.last()` is positional; a reordered list passes while downgrading every client |

The second is its own rule: **source test values from the API, never invent
them.** A guessed identifier that returns nothing proves nothing about the code.

Iterating the registry is not sufficient on its own, either. Asserting
`for x in LIST { f(x) == x }` proves only `find(x in LIST) == x`, which holds for
any contents of `LIST` - including one that has never heard of the value you
care about. Anchor at least one assertion to a literal drawn from outside the
code, as `PUBLISHED_MCP_REVISIONS` does against `SUPPORTED_PROTOCOL_VERSIONS`.

### One grep is not coverage

For work whose size is unknown - every call site, every unused item, every place
a parameter is honoured - **do not stop at a number you picked in advance, and
do not search only one way.** Keep going until two consecutive rounds turn up
nothing new, and search along axes that are blind to each other:

| Axis | Finds what the others miss |
|---|---|
| Symbol name | The direct callers |
| String literal | Reflection, config keys, MCP tool names, CLI subcommands dispatched by string |
| Import and dependency graph | Re-exports - `data_gov::catalog` re-exports catalog types, so a grep for `data_gov_catalog` misses every consumer that goes through it |
| Manifests and workflows | `Cargo.toml` features, `justfile` recipes, CI steps |
| Git history | Things deleted and half-restored, and why something is the way it is |

The re-export row is not hypothetical: `data-gov-mcp-server` reached catalog
types through the `data_gov::catalog` re-export, so a dependency audit keyed on
the direct crate name drew the wrong conclusion about who used what.

State which axes you ran. A sweep that reports "no other callers" without saying
how it looked is an assertion, not a result.

### Prefer real captured data to synthetic fixtures

Synthetic bodies contain exactly the fields the author thought of, so they
cannot surface the problems real payloads cause: nulls where a `Vec` is
expected, absent optional fields that vary by publisher, 90-character truncated
slugs, unicode in titles, numbers exceeding `i32`. Capture with `just fixtures`
and serve the real thing.

**A hand-written fixture encodes the author's assumption, and then the test
confirms it.** `data-gov-ckan` is the standing example: all 23 of its wiremock
tests are driven by inline bodies, and those bodies use UUID-shaped ids such as
`aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee`. The model types `id` as `uuid::Uuid`.
CKAN's `id` column is unconstrained text. The suite therefore *guarantees* #63
rather than catching it - every test feeds the model exactly the assumption the
model got wrong. No amount of care in writing those bodies would have helped,
because the author of the body and the author of the type held the same belief.

Reserve hand-written bodies for cases you genuinely cannot capture - a malformed
response, a value the live API does not currently produce. A specific error
status is *not* on that list: capture it.

**Capture the negatives too, not only the success case.** Both are now pinned,
and they are not the same shape:

| Fixture | Endpoint | Status | Why it matters |
|---|---|---|---|
| `dataset_not_found.json` | `/api/dataset/{absent}` | 404 | A well-formed, empty `SearchResponse`. This is what proves `dataset_by_slug` can tell "no such dataset" from "the API is down" |
| `search_no_matches.json` | `/search?q={no-match}` | 200 | `{"results": [], "sort": "relevance"}` - **no `total`, no cursor**. A different envelope from the 404 body, which is why every envelope field must stay optional |

### Every fixture records where it came from

`tests/fixtures/MANIFEST.json` holds the endpoint, HTTP status, source host, and
capture date for every fixture. `just fixtures` writes it; `fixture_parity_tests`
enforces it. A fixture added without an entry fails the build.

This is mechanical for the usual reason: a fixture with no recorded origin is
indistinguishable from one somebody hand-wrote to match the code, and that is
the exact failure the fixture suite exists to prevent. The capture date also
makes a stale set visible without reading the git log.

**A fixture that cannot be captured goes under `unverified` with a reason.**
`harvest_record_transformed.json` is the current one: the endpoint 404s for every
sampled record (#83), so the committed file is the last capture from when it
still answered. Recording that is the point - an unverified fixture is
acceptable, an unverified fixture that looks verified is not.

Re-capture when the API changes, and before changing any model. Fixtures
addressed by slug are pinned to specific long-lived datasets (one ordinary, one
at the 90-character truncation cap), because tests assert against their
contents and a fixture whose subject changes every capture cannot be asserted
on. The script warns loudly if a pinned dataset stops resolving, and never
truncates a good fixture when a request fails.

### Cover the edges, and backfill

Happy path alone is close to worthless; the defects live at the boundaries.
Every behaviour needs wrong types, nulls, empty strings, absent fields,
duplicates, boundary values, malformed input, and unusual orderings.

Backfilling tests for behaviour that predates the current change is expected
work, not scope creep - particularly when touching an area whose existing
coverage turns out to be vacuous.

### Specification-driven tests

- **Name tests after the behavior they verify:**
  `test_search_with_empty_query_returns_all_datasets` not `test_search_3`.
- **Group tests by concern** using `mod tests` blocks or dedicated test files.
- **Assert on structure and invariants**, not exact external data that can change.

**Each acceptance criterion becomes its own named test**, so a failing run names
the unmet requirement rather than a line number. When an issue lists three
criteria, the change adds three tests whose names read back as those criteria.
One test asserting all three tells you something broke, not what.

### Coverage is a floor, never a target

Measure coverage to find the code nobody tested. Do not write tests to move the
number: a test written to raise coverage is optimised for executing lines, which
is exactly the vacuous test this project has already been bitten by. Uncovered
code is a question to answer, not a defect to paper over.

Unit tests are deterministic - no network, no wall clock, no filesystem, no
randomness. Where behaviour depends on one of those, inject it. The live-API
tests are `#[ignore]`d and run in their own job for exactly this reason.

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

## The live API is the source of truth

The API's actual behaviour outranks this guide, the rustdoc, the READMEs, and
the tests. Each of those has been wrong about it.

### Read the machine-readable spec before the prose docs

**`https://catalog.data.gov/openapi.json`** is the Catalog API's own OpenAPI
document, and it is authoritative in a way the human-readable pages are not.
Check it - and the equivalent `/openapi.json`, `/swagger.json` or
`/.well-known/` for any other service - before concluding what an API does or
does not offer.

This is not a stylistic preference. `/api/dataset/{slug_or_id}`, the exact
dataset-lookup endpoint, is declared there with its parameter and its 200/404
responses. It appears nowhere in the prose documentation at
`resources.data.gov/catalog-api`. Working from the prose alone, the endpoint was
written off as undocumented and the crate shipped a full-text-search workaround
in its place for an entire release - a workaround that failed for 15% of
datasets. The same document also shows `/search` has no `slug` parameter at all,
which is the direct explanation for why `SearchParams::slug` was silently
ignored.

Prose documentation for this API is incomplete and, in places, stale. Treat it
as a starting point, not as the contract.

### Never assert a parameter works without probing it

`SearchParams::slug` shipped as a documented, builder-exposed filter that the
Catalog API silently ignores: it returns a full unfiltered page with HTTP 200,
so a caller receives arbitrary results and no error. The fixture test "covering"
it could not catch this - wiremock asserts the parameter was *sent*, never that
the server *honours* it.

Before claiming a query parameter filters, probe the live endpoint and compare
against an unfiltered baseline:

```bash
curl -s 'https://catalog.data.gov/search?per_page=3'
curl -s 'https://catalog.data.gov/search?per_page=3&<param>=<value>'
```

**This API does not reject invalid filter values**, so the two failure modes read
differently and must not be confused:

| Result | Meaning |
|--------------------------------|-------------------------------------------|
| Identical to baseline | Parameter **ignored** - the filter is a no-op |
| Zero results | Parameter **honoured**, value simply matched nothing |

**Source test values from the API, never invent them.** A guessed value that
returns nothing proves nothing. `org_slug=noaa-gov` looks broken; the real slug
from `/api/organizations` is `noaa`, and the filter works correctly.

### Changing a model requires fresh fixtures first

Any time a model or document structure looks like it needs to change, the order
is fixed and must not be reversed:

1. **Capture fresh fixtures** - `scripts/capture-fixtures.sh`
2. **Prove the change against them** - show the field really is absent, renamed,
   or a different type in current responses
3. **Then** change the model and update the tests

Never change a struct from reasoning about the code, the docs, or a stale
fixture alone.

**Removing a field is a high bar; removing an identifier is higher.** Fields
vanishing from a public open-data API is unlikely. A field missing from one
sample far more often means the sample is unrepresentative - one publisher
omitting an optional field - than that the API dropped it. Check several records
from different publishers, and prefer `Option<T>` with `#[serde(default)]` over
deletion. Widening a type (`i32` -> `i64`) or adding a `rename` is a lower bar,
but still needs a captured response showing the real shape.

This is not hypothetical: `ContactPoint::fn_` keys on the literal name `fn_`
while payloads send `fn`, so every contact name is silently dropped. Only
fixtures refreshed from reality catch that class of defect - and only if they
are refreshed from the API rather than hand-edited to match the code.

### What to test

For every public function or method:

1. **Happy path** - normal inputs produce correct output
2. **Edge cases** - empty strings, zero/negative values, None/missing fields
3. **Error cases** - invalid input returns the correct error variant, not a panic
4. **Boundary conditions** - pagination limits, filename conflicts, path traversal

### Running tests

```bash
just test                                 # Everything CI runs (181+ tests)
just test-unit                            # Unit tests only (fast, no network)
just test-live                            # Network + known-defect acceptance tests
```

To run one target while working on it, call cargo directly:

```bash
cargo test --doc --all-features           # Doc tests
cargo test --test client_tests            # Request shaping, via wiremock
cargo test --test fixture_parity_tests    # Model vs. captured API responses
cargo test --test integration_tests       # Live API tests
```

Refresh the captured responses with:

```bash
just fixtures                             # Recapture from the live Catalog API
```

See **Every fixture records where it came from** for what that writes and what
the parity tests enforce about it.

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
is a debugging nightmare - the operation fails and nothing explains why.

**Acceptable silent discards:**
- Fire-and-forget side effects where failure is expected and harmless (e.g.,
  removing a temp file that may not exist)
- Logging calls themselves (if writing a log line fails, retrying won't help)

Everything else should either propagate (`?`), log, or surface to the user.

### A normal negative answer is not a failure

**Distinguish "the answer is no" from "the request did not work."** They need
different types, different log levels, and different behaviour from the caller -
one is the API working correctly, the other needs somebody's attention.

`dataset_by_slug` is the worked example. A 404 means the dataset does not exist,
which is a legitimate answer to a legitimate question, so it returns `Ok(None)`.
A 500, a timeout, or a connection reset means the request did not work, so it
returns `Err`. Collapsing the two in either direction is a bug: report an outage
as `Ok(None)` and a data.gov failure looks to the user like a missing dataset;
report a 404 as `Err` and every "does this exist?" caller has to parse error
strings to find out.

The same split applies elsewhere:

- **Log levels.** A missing dataset is `debug`. A failed request is `warn` or
  `error`. Logging expected negatives at `error` trains everyone to ignore the
  error log, which is worse than not logging.
- **MCP tools.** A tool that declines for a business reason returns a result
  with `isError: true` and an explanatory content block. A JSON-RPC error object
  is for protocol and transport faults - a malformed request, an unknown method
  - not for "no datasets matched".
- **Empty results.** Zero search hits is a successful search. Never an error.

When it is genuinely unclear which of the two a case is, ask rather than
guessing.

### Repeating an operation must be safe

**Every operation is idempotent: running it again with the same inputs gives the
same result and causes no additional effect.** Networks retry, users re-run
commands, and an MCP client may redeliver a request. None of those should
produce a second outcome.

Reads are idempotent for free. Writes are where this is earned, and downloads
are this project's writes:

- **Downloading the same distribution twice converges on one correct file.** It
  does not produce `file.csv` and `file (1).csv`, and it does not leave a
  half-written file where a complete one stood. Write to a temporary path and
  rename onto the destination only once the transfer finished, so the final path
  only ever holds a complete file and an interrupted run loses nothing.
- **Distinct resources get distinct paths.** Two distributions sharing a title
  must not resolve to the same filename - that is not idempotency, it is data
  loss reported as success (#52).
- **Config writes replace, never append.** Setting an API key twice leaves one
  key.

Statements about this belong in tests: run the operation twice and assert the
second run leaves the same state as the first.

### Error messages

- **Be specific.** Say what went wrong and what was expected:
  `"invalid jsonrpc version: expected \"2.0\", got \"1.0\""` not `"bad version"`.
- **Include context.** Name the method, field, or value that caused the error:
  `"data_gov.search: missing parameters"` not `"missing parameters"`.
- **Don't dump internals.** Error messages are for consumers - omit stack
  traces, memory addresses, and internal type names. Keep them to one or two
  sentences.
- **Use error enums.** Each crate defines a clear error enum (e.g.,
  `ServerError`, `DataGovError`, `CkanError`). Map external errors with `#[from]`
  or explicit conversions - don't stringify them prematurely.

### No `unsafe`

This project has no need for `unsafe`. Do not add `unsafe` blocks, `unsafe fn`,
or `unsafe impl`. If a dependency requires unsafe at its boundary, wrap it in a
safe abstraction - but that situation should not arise here.

### No blocking in async contexts

Never call blocking operations (synchronous I/O, `std::thread::sleep`,
`Mutex::lock` on a long-held lock, CPU-heavy computation) inside an `async fn`
or while a tokio runtime is active. Blocking the executor starves other tasks.

- Use `tokio::fs` instead of `std::fs` in async code.
- Use `tokio::time::sleep` instead of `std::thread::sleep`.
- If you must run blocking code, use `tokio::task::spawn_blocking`.
- The REPL uses `Runtime::block_on` at the top level - that's fine since it
  owns the runtime. Don't nest `block_on` inside an already-async context.

## Rust idioms

### Type strictly outbound, tolerantly inbound

Both directions want types rather than bare strings, but they fail in opposite
ways, and the general "model fixed value sets as enums" rule needs inverting at
the inbound boundary.

**Outbound - values we choose - get enums.** A `sort` order, an operating mode,
a colour setting, an output format: the valid set is closed and we own it, so an
enum makes the invalid state unrepresentable and hands the caller compile-time
errors and completion instead of a runtime 400. `OperatingMode`, `ColorMode`,
and `ReplCommand` are the existing examples. A parameter typed `Option<String>`
that the API accepts only four values for is a missing enum.

**Inbound - values the server chooses - get permissive types.** A strict type on
a deserialised field converts one unexpected value into a total failure of the
whole response. This is the single most productive bug family in this workspace:

| Field | Modelled as | Reality | Result |
|---|---|---|---|
| CKAN `id` (#63) | `uuid::Uuid` | The column is unconstrained text | Any non-UUID id fails the entire response |
| `Resource.size` (#62) | `i32` | Byte counts, unbounded | One resource over 2 GiB fails the entire response |
| `ContactPoint::fn_` (#61) | keyed on `fn_` | The wire name is `fn` | Every contact name silently `None` |

So for anything arriving from the network: `String` over a parsed identifier,
`i64` over `i32`, `Option<T>` with `#[serde(default)]` over a required field,
and `#[serde(other)]` on any enum you do model, so an unknown variant lands in a
catch-all rather than aborting the parse. Validate after deserialising, where a
bad value costs you one record instead of the page.

A useful check: **ask what happens when this field holds a value nobody
anticipated.** If the answer is "the whole request fails", the type is too
strict for a boundary.

### Prefer borrowing over cloning

Accept `&str` instead of `String`, `&[T]` instead of `Vec<T>`, and `&Path`
instead of `PathBuf` in function parameters when the function doesn't need
ownership. Clone only when you genuinely need an independent owned copy.

Common smells:
- `.clone()` immediately before passing to a function - the function should
  probably take a reference instead.
- `.to_string()` on a `&str` just to satisfy a `String` parameter - change the
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
- Don't log in hot paths at `info` or above - keep `info` to startup, shutdown,
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
3. Merge finished items into the release branch - never directly into `main`.
4. Validate the release as a whole.
5. Tag and publish, then merge the release branch to `main`.

`main` therefore always reflects a released state, and a partially-landed release
never sits on the default branch.

**This is the repository's override of the general "open a PR to `main` and ask
before merging" default.** Concretely, what it authorises and what it does not:

| Action | Authorised? |
|---|---|
| Merge a green, complete work-item PR into `release/X.Y.Z` | Yes - this is the workflow |
| Merge anything into `main` | No. Ask, every time |
| Tag or publish to crates.io | No. Ask, every time |
| Commit directly to `release/X.Y.Z` without a PR | No. Work items get branches and PRs |

"Green" means every gating check passed on the PR itself, not that it passed
somewhere else earlier.

**Never stack a pull request on another work branch.** Every work branch targets
the release branch directly, even when one change logically builds on another -
merge the first, then merge the release branch into the second.

CI triggers on `main` and `release/**` only, so a PR whose base is another
feature branch receives **no checks at all** and says so quietly:
`no checks reported`. Retargeting an existing PR does not fix it either, because
`pull_request` fires on `opened`, `synchronize` and `reopened` - not `edited`.
It takes a fresh push. A stacked PR that looks reviewable while having been
tested by nothing is the failure mode here, and it is silent.

**`Closes #N` does not fire on a release-branch merge.** GitHub only auto-closes
from the default branch, so issues stay open until the release reaches `main`.
That is correct - the fix is not released yet - but it looks broken. Note on
the issue where the work landed rather than closing it by hand.

Expect `CHANGELOG.md` to conflict on every parallel PR, since they all append
under the same headings. The conflicts are additive and resolve by keeping both
sides; if it becomes tiresome, changelog fragments would remove the class.

### Changelog

`CHANGELOG.md` follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Every change that a consumer could notice - API changes, bug fixes, behavioural
changes, CLI or MCP surface changes - **adds an entry under the current
`## [X.Y.Z] - Unreleased` heading in the same PR that makes the change.** Purely
internal refactors with no observable effect may be omitted.

#### Section order is mandatory

**`Breaking` comes first, `Changed` second, always.** These are the two
categories that cost a reader real money if missed - one stops their build, the
other silently alters what their working code does - so they must never be
buried beneath a list of additions and fixes. Within a release, sections appear
in exactly this order, omitting any that are empty:

| Order | Section | What belongs here |
|-------|-----------------|-------------------------------------------------------------|
| 1 | `Breaking` | Anything requiring a consumer to change their code to compile: removed or renamed public items, changed signatures or field types, altered feature or wire formats. **Removing public API is `Breaking`, not `Removed`.** |
| 2 | `Changed` | Behaviour changes that still compile: different defaults, different output, different ordering, a fix that alters an existing contract. The dangerous ones - call these out even when they look minor. |
| 3 | `Security` | Advisories cleared and vulnerabilities fixed. Name the CVE/RUSTSEC/GHSA ID. |
| 4 | `Added` | New APIs, features, flags, tools. |
| 5 | `Fixed` | Bugs fixed without changing an intended contract. |
| 6 | `Deprecated` | Still present, slated for removal. Say what replaces it. |
| 7 | `Removed` | Non-API removals only: dependencies, internal machinery, dead files. |
| 8 | `Infrastructure` | CI, build, tooling. No consumer impact. |

Where a single release has several distinct breaking themes, use qualified
headings (`### Breaking - Catalog API migration`) and keep them adjacent at the
top rather than scattering them.

Each `Breaking` and `Changed` entry states **what a consumer must do**, not only
what changed. `download_resources` renamed to `download_distributions` is a fact;
"rename your calls, and pass `&[Distribution]` instead of `&[Resource]`" is
actionable.

Reference the issue number where one exists. On release, replace `- Unreleased`
with the release date and open a fresh `## [X.Y.Z] - Unreleased` heading above it.

The ordering above is worth enforcing mechanically rather than trusting. It was
violated in the next PR after it was written, by its own author, who put `Fixed`
ahead of `Security` without noticing. A short CI step comparing the `###`
headings under the current version against the canonical sequence would catch
that in seconds; a written rule did not.

### Dependency freshness and advisories

Every release must ship with dependencies current and free of significant
vulnerabilities. Before validating a release, run **both** halves:

```bash
cargo update                       # Semver-compatible moves
just outdated                      # Major-version drift needing a decision
just audit                         # RustSec advisories, then OSV/GHSA
```

`just audit` runs both halves. `cargo audit` alone is not sufficient:
in the 2026-07 review it reported two advisories while OSV surfaced seven further
`openssl` CVEs (five High) that are GHSA-only and absent from the RustSec
database. Re-run at release time, not just at the start of the work - advisories
land after the fact, so a clean run expires.

**Scan between the lockfile changing and the first build, not after.** Build
scripts (`build.rs`) execute arbitrary code at compile time - under `clippy` as
much as under `build` - so a scan run after the first compile has already run
whatever it was meant to warn you about.

**The scan must be unable to pass by accident.** A green result has to mean the
advisory database was consulted, not that the check quietly degraded:

| Condition | Required behaviour |
|-------------------------------------|--------------------------------|
| `cargo-audit` not installed | Hard failure, printing the install command |
| Advisory database unfetchable | Hard failure. Never fall back to a cached database silently; if a possibly stale database is acceptable, that is an explicit opt-in that prints the database age loudly |
| Vulnerability reported | Hard failure |
| Informational advisory (unmaintained, unsound, yanked) | Reported loudly, does not fail the gate; treat as a review item |
| Advisory suppressed by an ignore list | Named in the output. Suppression is a reviewed decision, but the run must never present itself as a clean scan |

**High and critical advisories are hard blockers.** Patch in the same change,
prove the path unreachable and document why, or file a tracked follow-up
referenced from the change. Never ship past one silently, and never pin around
one without a comment naming the advisory.

## Configuration and file locations

**Configuration lives in the XDG base directories, never in `$HOME` directly.**
Use the `dirs` crate (already a dependency) rather than hand-building paths, so
the behaviour is correct on every platform:

| Purpose | Call | Linux | Path used |
|---------------|-----------------------|--------------------------|--------------------------|
| Configuration | `dirs::config_dir()` | `$XDG_CONFIG_HOME`, else `~/.config` | `<config>/data-gov/` |
| Cached data | `dirs::cache_dir()` | `$XDG_CACHE_HOME`, else `~/.cache` | `<cache>/data-gov/` |
| State/history | `dirs::data_dir()` | `$XDG_DATA_HOME`, else `~/.local/share` | `<data>/data-gov/` |
| Downloads | `dirs::download_dir()` | `$XDG_DOWNLOAD_DIR` | user's own choice |

Never write a dotfile to `$HOME` (`~/.data-gov`, `~/.data-gov-api-key`), and
never hardcode `~/.config` - that string is wrong on macOS and Windows, and
ignores `XDG_CONFIG_HOME` on Linux. Honour the environment variable by going
through `dirs`.

**Precedence**, highest first: command-line flag, then environment variable,
then config file, then built-in default. A setting that a flag cannot override
is a bug.

### Secrets

API keys and tokens are read from a file under `dirs::config_dir()` or from the
environment. Never commit one, never log one, and **never accept one as a
command-line argument** - `argv` is visible to every process on the machine via
`ps`.

**Permissions are part of the contract, on the directory as well as the file:**

| Path | Mode | Why |
|-----------------------------|--------|-----------------------------------------------|
| `<config>/data-gov/` | `0700` | `0755` lets anyone list the directory and see that a key exists, even when they cannot read it |
| `<config>/data-gov/api-key` | `0600` | `rw-------`. The execute bit in `0700` is meaningless on a key file; `0600` is what ssh, gpg, and netrc use |

When creating either, set the mode at creation rather than `chmod`-ing
afterwards - a file created `0644` and tightened a moment later was
world-readable in between. In Rust, use
`std::os::unix::fs::OpenOptionsExt::mode()` on the `OpenOptions` (and
`DirBuilderExt::mode()` for the directory), behind `#[cfg(unix)]`.

When *reading* a secret, check the mode first and refuse or warn if it is group-
or world-accessible, the way ssh does. A key with the wrong permissions has
already leaked; failing loudly is the only useful response.

## Code organization

### Modularization principles

Code should be organized into **discrete, single-purpose modules and functions**
that are easy to read and reason about in isolation.

1. **One concern per file.** A file should have one clear responsibility. If you
   need a comment header like `// === Section ===` to separate unrelated logic,
   it's time to split.

2. **One concern per function.** A function should do one thing. If it has
   multiple levels of nesting, multiple sequential phases, or you're tempted to
   add section comments inside it - extract helper functions.

3. **No file exceeds 1000 lines.** If a file approaches this limit, split it.
   Prefer many small files over few large ones.

4. **Public API surface is intentional.** Only `pub` what consumers need.
   Internal helpers should be `pub(crate)` or private. A module's public items
   are its contract - keep it narrow.

5. **Flat over deep.** Prefer `mod foo; mod bar;` siblings over deep nesting.
   One level of `submodule/` is fine; two levels is a smell.

### Keep transport at the edges

The crate split is already a ports-and-adapters boundary, and it is worth
keeping deliberately:

| Crate | Role |
|---|---|
| `data-gov-catalog`, `data-gov-ckan` | Adapters. HTTP, serde, wire shapes, one upstream API each |
| `data-gov` | Domain and orchestration: search, download, path handling, config |
| `data-gov-mcp-server`, the CLI | Transports. Parse a request, call the domain, format a response |

So: no `reqwest::Response`, `serde_json::Value`, or `clap` type in a signature
that domain logic depends on, and no download or search logic inside a request
handler. A handler that is more than argument parsing, one domain call, and
response formatting has domain logic in the wrong layer - the MCP server and the
CLI should be two thin faces over identical behaviour, and they diverge the
moment either grows logic of its own.

The practical test: domain behaviour is unit-testable without a server, a mock
HTTP endpoint, or an argv array.

### Earn abstractions

Do not introduce a new abstraction until roughly three call sites prove the
pattern; premature indirection is harder to remove than to add. When one new
type would unify several existing needs, justify the unification explicitly
rather than assuming generality is free. Reuse the existing layering and traits
before inventing parallel ones, and extend an existing module over adding one
unless the seam is obvious.

Keep each change small enough to land on its own with a clear deliverable.

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
is required for MSRV compatibility. Run `just outdated` periodically and update
proactively.

```bash
cargo update       # Apply semver-compatible updates
just outdated      # Check for new major versions
just audit         # Check for security advisories
```

**Check the current release before adding a dependency**, with `cargo search` or
crates.io, rather than reaching for a version you remember. A remembered version
is usually behind, sometimes by a major release, and occasionally by an
advisory. A new dependency is a review item: say in the PR why the standard
library or an existing dependency does not cover it.

### Current state (as of 2026-03)

- **reqwest** `0.13.x` - note that `query` is now an explicit feature in 0.13
- **serde** `1.0.x`, **tokio** `1.x`, **clap** `4.5.x`, **thiserror** `2.0.x`

### Dependency hygiene

1. **Pin to the narrowest range that works.** `"^X.Y"` for external crates.
   Never `"*"`.
2. **One version per dependency across the workspace.** Use
   `[workspace.dependencies]` in the root `Cargo.toml` to centralize versions
   when practical.
3. **Run `just audit` before releasing.** CI does this automatically.
4. **Run `just check` after every update.**

### MSRV constraint

The workspace targets **Rust 1.90**. All dependency versions must be compatible.
The `Cargo.lock` is committed and tested in CI against stable, beta, and MSRV.

The MSRV is declared as `rust-version = "1.90"` in each of the four member
manifests, so cargo refuses to build on an older toolchain and says why. There
is deliberately **no `rust-toolchain.toml`**: that file overrides the toolchain
rustup selects, which would pin the beta and MSRV legs of the CI matrix to a
single channel and quietly reduce three-way coverage to one. Reproducibility
here comes from the committed lockfile plus the declared `rust-version`.

A fresh clone needs `just` and any Rust from 1.90 up. `just check` then builds
and tests everything, with no other machine-local state.

## Security checklist

### Nothing machine-specific or personal reaches git

**No detail of the machine you happen to be working on belongs in anything that
lands in the repository** - not in code, configs, manifests, docs, comments,
commit messages, or PR descriptions. That covers absolute home paths, usernames,
internal hostnames, IP addresses, personal ports, email addresses, and account
identifiers.

The committed VS Code MCP example hardcodes an absolute developer home path
(#74), which is the whole failure mode in one line: it does not work for anyone
else, and it publishes the author's directory layout and account name for no
benefit.

Use workspace-relative paths, read anything machine-specific from configuration
or the environment, and put obvious placeholders in examples:
`/home/<user>` or `$HOME`, `example.com`, `user@example.com`, `192.0.2.0/24`.
Values shared by CI or by every user of the crate are fine; a specific
workstation is not.

Scan the diff for real paths, hostnames, and addresses before committing. A
`grep` for your own username and home directory takes a second and catches
nearly all of it.

### Test data stays synthetic or public

Fixtures in this workspace are captured from data.gov, whose contents are public
federal metadata, so there is no personal data in them by construction. Keep it
that way: never commit a payload from a private or authenticated endpoint, and
never paste a real credential into a fixture even in a redacted-looking form.
Hand-written fixtures use obviously fake values.

### Read your own diff adversarially - twice

Before requesting review, reread the change looking for: untrusted input
crossing a trust boundary (network responses, MCP tool arguments, CLI
arguments, filenames from remote metadata), secrets reaching logs or error
messages, panics reachable from input, and paths or shell strings built by
concatenation. Every one of those classes has produced a real defect in this
workspace.

**Then review the fixes, as a second pass over the new diff.** A fix is fresh
code written under time pressure by someone who has stopped looking for
problems, and it gets no scrutiny at all unless you deliberately turn around and
look at it.

This is written from a fix that shipped a fresh bug. Correcting the MCP content
blocks (#60) introduced a `structuredContent` that was a bare JSON array for two
of the five tools - a new spec violation, created by the change that existed to
remove a spec violation. It survived because verification exercised one tool and
generalised, and because nobody re-reviewed the repair.

Anything you deliberately leave unfixed gets a one-line reason recorded in the
PR, so a reviewer can tell a considered decision from an oversight.

### Self-review is the weakest review; run it from a fresh context

**Reading your own diff harder does not work, and this project has the
measurement.** Ten MCP tests were written to a rule that asked the author to
name the wrong implementation each test would catch. A separate adversarial run,
given the same code and none of the author's reasoning, defeated **all ten**.
The author had read them twice.

The mechanism is not carelessness. Having just written the implementation, your
model of *wrong* is anchored to it - you check the mistakes you nearly made, not
the ones you cannot see. A fresh context does not share the anchor. So:

- Give the reviewer **the requirement and the diff, never your conclusion**. The
  moment a reviewer knows what you decided, they are checking your reasoning
  rather than the code.
- Where more than one reviewer runs, give each a **distinct lens** - correctness,
  security, does-the-failure-actually-reproduce - rather than repeating one pass.
  Separate lenses catch what redundancy cannot.
- This applies to every deliverable, not only code. A design, a fixture set, or
  a document gets the same two passes against its own bar.

### A finding has to survive an attempt to kill it

**The burden of proof sits on the finding, not on the code.** Before acting on
one, name the concrete failure - the inputs, the state, the wrong result - and
check it against reality. Drop it when you cannot show that failure.

This is the most expensive failure mode in this repository's review history.
Four findings in a single sweep were confidently wrong, and each one looked like
diligence:

| Finding as raised | What was actually true |
|---|---|
| `spatial_filter` is a phantom filter that does nothing | It was probed with an invalid value. With a valid one it filters correctly |
| Organizations have no `slug` field | The probe printed four keys per object. All 120 have one |
| `org_slug` is broken | The value was guessed as `noaa-gov`. The real slug is `noaa` |
| `cargo audit` passes silently when it cannot fetch the database (#99) | It already exits 1. Measured: online 0, offline 1, offline with `--stale` 1. Filed, then closed as invalid |

Three of the four came from testing an invented value against a live API and
reading "no result" as "broken" - which is why sourcing test values from the API
is its own rule. The fourth was a premise nobody checked before filing.

A plausible-but-wrong finding costs more than the defect it claims to find:
somebody changes working code, or files an issue that a later reader trusts. A
finding that is expensive to fix, or that would change the design, gets an
independent check from someone who did not raise it.

### Report the limits of what you checked

**When you sample, stop early, skip an area, or abandon a path, say so in the
summary, the PR, or the issue.** Silent truncation reads as full coverage to
whoever receives it, and they cannot see the gap you saw.

Both sides of this happened here. The slug investigation reported its bounds -
"15% unresolvable on a uniform sample of 578, 27% past cursor depth 400" - and
those numbers were what made the fix arguable. The MCP verification did not: it
exercised one of five tools and reported the result as though it covered the
set, which is precisely how a bare-array `structuredContent` reached a release.

"I checked the three largest callers, not all forty" is a useful result. "I
checked the callers" is not.

Scan dependencies whenever the lockfile changed, before the first build.

Before any release:

- [ ] `just audit` passes - RustSec *and* OSV
- [ ] User-supplied paths are sanitized via `data_gov::util::sanitize_path_component()`
- [ ] MCP `output_dir` parameter rejects `..`
- [ ] No secrets (API keys, tokens) in logs or error messages
- [ ] Download URLs are not constructed from unvalidated user input
- [ ] No absolute home path, username, hostname, or email in the diff

## CI pipeline

GitHub Actions CI (`.github/workflows/ci.yml`) runs on pushes and pull requests
targeting `main` **and any `release/**` branch**, so release-branch work is gated
the same way `main` is.

**Every step calls a `just` recipe**, so the pipeline and a local `just check`
run identical commands. Changing a gate command means editing the `justfile`,
once.

**Test Suite** (matrix: stable, beta, MSRV 1.90)
1. **Format check** (`just fmt-check`, stable only)
2. **Plain-ASCII check** (`just check-ascii`, stable only) - the working docs
3. **Clippy** (`just lint`, stable only)
4. **Build** (`just build`)
5. **Tests** (`just test`)
6. **rustls-only build** (`just check-rustls`) - the configuration
   `--all-features` never compiles

Test selection matters here. `--workspace` covers unit tests, every `tests/`
integration target, and doc tests. Selecting `--lib` instead silently skips both
binary crates (`data-gov-mcp-server` has no `lib.rs`; the CLI is a `[[bin]]`) and
every `tests/` target - 37 of 181 tests would run, while `clippy --all-targets`
still compiles the rest so nothing looks wrong. Do not narrow this back.

**Other jobs**
7. **Examples** (`just examples`) - compiles all workspace examples; they make
   real API calls, so they are built but never run
8. **Documentation build** (`just docs`)
9. **Security audit** - `cargo audit` *plus* an OSV/GHSA lockfile scan. Both are
   required; see the dependency-freshness section for why `cargo audit` alone is
   insufficient. This job does not use `just audit`: locally that recipe calls
   the `osv-scanner` CLI, while CI runs the upstream scanner action. Same two
   databases, different delivery.
10. **Live API Tests** (`just test-live`) - opt-in via `workflow_dispatch`, runs
   the `#[ignore]`d network tests. Deliberately not gating: a data.gov outage
   must not turn PRs red.

All gating checks must pass before merging.

## Working practice

### Worktrees

Do code work in a git worktree on its own branch off the release branch, never
in the primary checkout, so concurrent sessions never collide on the working
tree, the checked-out branch, or build artefacts. Convention:
`<repo>/.worktrees/<slug>/`. Remove it after the branch merges.

**Anything that edits files gets its own tree.** A background agent pointed at a
live worktree will write into it, and the result is a working copy of unknown
provenance - during the 2026-07 review two agent runs added three files and
modified five while a change was being built in the same directory, and the test
failures could no longer be attributed. Give parallel work isolated worktrees,
brief each on its scope, and check first for shared files: a manifest edit or a
changelog entry in two trees at once serialises whether you planned it or not.

### Decide at the top, build in the branches

Do the investigation, the API probing, and the design decision **before** cutting
the work up - those need the whole problem in view, and this API punishes
guessing. Then give each unit its own branch and its own worktree, with a brief
that carries the decisions already made, so nobody re-derives a settled question
or re-probes an endpoint that has already been characterised.

**A brief that a fresh reader cannot act on means the investigation is not
finished.** It needs: the problem and why it matters, the files and interfaces
in play, the constraints, and what "done" looks like. Point at the issue rather
than restating it - that is what the tracker entry is for.

The one thing that stays out of a brief is your own conclusion, and only when you
want independent judgement. A reviewer who knows what you decided reviews your
reasoning instead of the code.

### Weigh more than one design before committing to one

Where the solution space is genuinely open, sketch two or three approaches from
different angles and score them against the requirement and the failure cases,
rather than refining the first idea. Skip it where the existing system already
fixes the shape.

The slug fix is what this looks like when it works. Two candidates were live:
raise `per_page` so the full-text search finds more, or use the exact-lookup
endpoint. Measurement killed the first - 83 of 87 failures returned *zero* hits,
so a bigger page could not have helped - and that measurement is the entire
argument for the endpoint. Iterating on the first idea would have produced a
larger page size, a slower client, and the same bug.

### Issue and pull request hygiene

**The tracker is GitHub Issues on this repository**, reached with the `gh` CLI
(`gh issue list`, `gh issue create`, `gh issue comment`). It is settled; do not
ask again.

- Claim an issue when you start it, so parallel work does not collide.
- Link the change to the issue: `Closes #N` to auto-close, `Refs #N` when it
  only partially addresses it. Note that `Closes` does not fire on a
  release-branch merge - see **Release branches**.
- Anything deferred, discovered, or out of scope becomes a tracker entry, not a
  code comment. Entries assume fresh eyes: what the work is, why it is needed,
  and acceptance criteria, enough for someone who was not there to pick it up.
- A large multi-part effort gets an epic with the work items linked from it.
- Keep the entry current as work moves, not only at open and close. A short
  comment recording a decision, a blocker, or a measurement is what makes the
  tracker reconstructable later - several issues in this repo were settled by a
  number somebody recorded in a comment.
- On multi-session work, leave a short status note before stopping - what
  landed, what is in progress, what is blocked - so state is reconstructable
  without replaying the git log.

### Commits

Conventional-commit subjects, scoped to the crate where it helps:

```
fix(catalog): resolve datasets by slug through the exact-lookup endpoint
test(catalog): record fixture provenance and capture the negative responses
build: enforce warnings mechanically, add a just runner, and gate rustls-only
docs: fold the newest shared standards into CLAUDE.md
```

- **One concern per commit.** A gate change and a model change do not travel
  together, even when the same task produced both.
- **Never commit against a failing gate.** `just check` is the bar, and
  `--no-verify` is not an answer.
- **Commit `Cargo.lock`.** It is what makes the MSRV leg of CI meaningful.
- **The body carries the reasoning the diff cannot**: what was measured, what
  was ruled out, which mutations were run. A subject line says what changed; the
  body is where "83 of 87 failures returned zero hits" lives.

### Writing

Prose in this repository - docs, comments, commit messages, issues, PR bodies -
is **plain ASCII**. Use `-` rather than an em- or en-dash, `->` rather than an
arrow glyph, `...` rather than an ellipsis character, and straight quotes. The
point is that a human editing the file can type every character in it without
hunting for a compose key.

`CLAUDE.md` and the `justfile` hold to this. The READMEs deliberately do not:
their emoji headings and box-drawing trees are presentation, and rewriting them
would cost more than it returns.

Keep sentences short and active, one main clause each, and use one word per
meaning throughout - `distribution` is not sometimes `resource`. Technical
prose is read by people in a hurry and by tools that do not infer.

## Reference: where the authoritative sources are

Prose documentation for these services is incomplete and in places stale. These
are the sources that settled questions during the 2026-07 review, with the
caveats that made them worth recording.

### data.gov Catalog API

| Source | Use it for | Caveat |
|---|---|---|
| `https://catalog.data.gov/openapi.json` | **The contract.** Endpoint paths, parameters, responses | Authoritative. Check here first |
| `https://resources.data.gov/catalog-api/` | Narrative guide, examples | Incomplete - omits `/api/dataset/{slug_or_id}` entirely |
| `https://api.data.gov/docs/developer-manual/` | Gateway architecture, API-key rate limits | DEMO_KEY is 30/hour and 50/day; a signed-up key is 1,000/hour |
| `https://open.gsa.gov/api/` | GSA's API catalogue | **Stale.** Lists only the retired CKAN v3 API, with no deprecation notice, and does not mention the current Catalog API at all |

Two hosts serve the same backend: `catalog.data.gov` (keyless, `/api`-prefixed
paths) and `api.gsa.gov/technology/datagov/v4` (documented, requires
`X-Api-Key`, no `/api` prefix). `/search` returns identical results from both.

### Model Context Protocol

| Source | Use it for |
|---|---|
| `https://modelcontextprotocol.io/specification/versioning` | **Which revision is current.** Check this before writing a version string |
| `.../specification/<revision>/basic/lifecycle` | initialize, version negotiation, notifications, shutdown |
| `.../specification/<revision>/server/tools` | Tool results, the closed `content` union, `structuredContent`, `isError` |

Revisions are dates and the details differ between them. `2025-11-25` was
current at the time of writing, and was two revisions ahead of what would have
been written from memory.

### Advisories

`cargo audit` consults the RustSec database only. **OSV.dev aggregates RustSec
and GHSA**, and the difference is not academic: seven `openssl` advisories, five
of them High, were GHSA-only and invisible to `cargo audit`. Run both.

### Reference implementations

`~/Projects/adelie-ai/mcp-core` is a hand-rolled MCP server core used by around
a dozen servers, and a useful cross-check on protocol questions - it
independently arrives at the same negotiate-or-fall-back design and the same
`listChanged` capability shape. It is **not usable as a dependency here**: it is
git-only and unpublished, and `cargo publish` rejects git dependencies. The
`mcp-core` crate on crates.io is an unrelated project by a different author.
