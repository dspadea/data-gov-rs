# Development tasks for data-gov-rs.
#
# CI calls these same recipes, so a green `just check` locally means a green
# pipeline. Keep recipes short: when one grows past a few lines, move the body
# into `scripts/` and call the script from here.
#
# Three of the recipes exist because of a specific trap:
#
#   test          `--workspace` is required. Narrowing to `--lib` silently skips
#                 both binary crates and every `tests/` target - 37 of 181 tests
#                 ran that way, and nothing looked wrong.
#   check-rustls  `--all-features` enables native-tls and rustls at once, which
#                 no consumer selects, and never compiles the rustls-only build
#                 that consumers do select.
#   audit         `cargo audit` reads the RustSec database only. GHSA-only
#                 advisories are invisible to it, so OSV runs too.

# List the available recipes.
default:
    @just --list

# The full gate. Run this before you push.
check: fmt-check check-ascii check-home-paths check-deps lint build test check-rustls examples docs

# Format every file in place.
fmt:
    cargo fmt --all

# Fail if any file is not rustfmt-clean.
fmt-check:
    cargo fmt --all -- --check

# Fail if the working documents contain characters nobody can type by hand.
# The READMEs are exempt: their emoji and box-drawing are presentation.
check-ascii:
    #!/usr/bin/env bash
    set -euo pipefail
    status=0
    for f in AGENTS.md CLAUDE.md justfile; do
      if LC_ALL=C grep -nP '[^\x00-\x7F]' "$f"; then
        echo "^ $f: non-ASCII above. Use - for dashes, -> for arrows, ... for ellipsis." >&2
        status=1
      fi
    done
    exit $status

# Fail if the tracked tree contains a real developer's absolute home path
# (#74): /home/<name>, /Users/<name>, or C:\Users\<name>. A committed path
# like that works on nobody's machine but the one it was captured from.
#
# The lookbehind excludes a path segment inside a URL - "arcgis.com/home/..."
# is not a home directory - and the three-character floor on the name
# excludes short synthetic placeholders such as the "me" in a Windows
# path-traversal test fixture, without hardcoding that value by name.
# Bracket-class syntax right after the literal prefix keeps this recipe from
# matching its own pattern definition.
check-home-paths:
    #!/usr/bin/env bash
    set -euo pipefail
    pattern='(?<![A-Za-z0-9])(/home/|/Users/|C:\\+Users\\+)[A-Za-z0-9._-]{3,}'
    if git grep -nIP "$pattern" -- .; then
      echo "^ absolute home path(s) above. Use \$HOME, a workspace-relative path, or a placeholder like /home/<user>." >&2
      exit 1
    fi
    exit 0

# Fail if a workspace crate declares a dependency nothing imports (#76).
# Installs cargo-machete on demand so a fresh clone with just `just` and Rust
# still runs `just check` end to end.
check-deps:
    #!/usr/bin/env bash
    set -euo pipefail
    if ! command -v cargo-machete >/dev/null 2>&1; then
      echo "cargo-machete not found; installing..." >&2
      cargo install cargo-machete --locked
    fi
    cargo machete

# Clippy over every target and feature, warnings fatal.
lint:
    cargo clippy --all-targets --all-features --workspace -- -D warnings

# Compile the workspace.
build:
    cargo build --all-features --workspace

# Compile the examples. They call the live API, so they are built, never run.
examples:
    cargo build --examples --all-features --workspace

# Unit, integration, and doc tests across the whole workspace.
test:
    cargo test --all-features --workspace

# Unit tests only. Fast, no network.
test-unit:
    cargo test --lib --all-features --workspace

# The ignored tests that call live data.gov. Needs network; never part of check.
test-live:
    cargo test --all-features --workspace -- --ignored

# Compile the rustls-only configuration.
check-rustls:
    cargo check -p data-gov-catalog --no-default-features --features rustls-tls
    cargo check -p data-gov-ckan --no-default-features --features rustls-tls

# Rustdoc must build clean.
docs:
    cargo doc --all-features --no-deps --document-private-items

# Scan dependencies for advisories, RustSec and OSV both. Needs cargo-audit and
# osv-scanner on PATH. CI runs the OSV half through the upstream action instead.
audit:
    cargo audit
    osv-scanner scan source --lockfile Cargo.lock

# Check for dependency releases beyond the current semver range.
outdated:
    cargo outdated --root-deps-only

# Re-capture the Catalog API fixtures from live data.gov.
fixtures:
    ./scripts/capture-fixtures.sh
