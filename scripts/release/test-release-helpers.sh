#!/usr/bin/env bash
# Specification tests for the crates.io release helpers.
#
# These run offline against a stub index (scripts/release/stub-index-server.py),
# because the behaviour under test is what the helpers do when the real index
# answers badly - 404, 500, a rate-limit, a refused connection - and none of
# that can be provoked on demand from the live service.
#
# The helpers sit on the irreversible path: a crates.io publish cannot be
# undone, only yanked, and yanking does not free the version number. So the
# case that matters most here is the guard, not the happy path - an index
# that cannot be reached must stop the publish, never wave it through.
#
# Usage: test-release-helpers.sh [name-substring]

set -uo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
tmp="$(mktemp -d)"
filter="${1:-}"

# Exit codes the helpers use to tell the three outcomes apart.
readonly PUBLISHED=0
readonly NOT_PUBLISHED=1
readonly UNKNOWN=2

stub_pid=""
stub_base=""
passed=0
failed=0

stop_stub() {
  if [ -n "$stub_pid" ]; then
    kill "$stub_pid" 2>/dev/null
    wait "$stub_pid" 2>/dev/null
    stub_pid=""
  fi
}

cleanup() {
  stop_stub
  rm -rf "$tmp"
}
trap cleanup EXIT

# Start the stub index and point $stub_base at it. The stub binds port 0 and
# reports the port it got, so parallel runs cannot collide on a fixed one.
start_stub() {
  stop_stub
  : > "$tmp/stub.out"
  python3 "$here/stub-index-server.py" "$@" > "$tmp/stub.out" 2> "$tmp/stub.err" &
  stub_pid=$!
  local port=""
  for _ in $(seq 1 100); do
    port="$(awk '/^PORT /{print $2; exit}' "$tmp/stub.out")"
    [ -n "$port" ] && break
    sleep 0.05
  done
  if [ -z "$port" ]; then
    echo "stub index failed to start:" >&2
    cat "$tmp/stub.err" >&2
    exit 1
  fi
  stub_base="http://127.0.0.1:${port}"
}

# Nothing listens on port 1, so this stands in for a DNS failure, a dropped
# connection, or crates.io being unreachable.
unreachable_base() {
  echo "http://127.0.0.1:1"
}

want() {
  local name="$1" expected="$2"
  shift 2
  if [ -n "$filter" ] && [[ "$name" != *"$filter"* ]]; then
    return 0
  fi
  : > "$tmp/cargo.log"
  local actual=0
  "$@" > "$tmp/out" 2>&1 || actual=$?
  if [ "$actual" -eq "$expected" ]; then
    printf 'ok   %s\n' "$name"
    passed=$((passed + 1))
  else
    printf 'FAIL %s: expected exit %s, got %s\n' "$name" "$expected" "$actual"
    sed 's/^/       | /' "$tmp/out"
    failed=$((failed + 1))
  fi
}

# Assertions about the run `want` just made. Kept separate so one case can
# check both the exit status and what the operator was told.
and_output_has() {
  local name="$1" needle="$2"
  if [ -n "$filter" ] && [[ "$name" != *"$filter"* ]]; then
    return 0
  fi
  if grep -qF -- "$needle" "$tmp/out"; then
    printf 'ok   %s\n' "$name"
    passed=$((passed + 1))
  else
    printf 'FAIL %s: output does not mention %s\n' "$name" "$needle"
    sed 's/^/       | /' "$tmp/out"
    failed=$((failed + 1))
  fi
}

and_cargo_ran() {
  local name="$1"
  if [ -n "$filter" ] && [[ "$name" != *"$filter"* ]]; then
    return 0
  fi
  if [ -s "$tmp/cargo.log" ]; then
    printf 'ok   %s\n' "$name"
    passed=$((passed + 1))
  else
    printf 'FAIL %s: cargo was never called\n' "$name"
    failed=$((failed + 1))
  fi
}

and_cargo_did_not_run() {
  local name="$1"
  if [ -n "$filter" ] && [[ "$name" != *"$filter"* ]]; then
    return 0
  fi
  if [ -s "$tmp/cargo.log" ]; then
    printf 'FAIL %s: cargo ran anyway: %s\n' "$name" "$(cat "$tmp/cargo.log")"
    failed=$((failed + 1))
  else
    printf 'ok   %s\n' "$name"
    passed=$((passed + 1))
  fi
}

# A stand-in for cargo, so a test can tell whether a publish was attempted
# without publishing anything. Real `cargo publish` is never called here.
mkdir -p "$tmp/bin"
cat > "$tmp/bin/cargo" <<'CARGO'
#!/usr/bin/env bash
echo "$@" >> "$CARGO_LOG"
CARGO
chmod +x "$tmp/bin/cargo"
export CARGO_LOG="$tmp/cargo.log"
export PATH="$tmp/bin:$PATH"

# Two lines in the shape crates.io actually returns, with the dependency
# arrays trimmed. The version under test, 0.5.0, is deliberately absent.
cat > "$tmp/index-body.txt" <<'BODY'
{"name":"data-gov-ckan","vers":"0.3.1","deps":[],"cksum":"0000000000000000000000000000000000000000000000000000000000000000","features":{},"yanked":false}
{"name":"data-gov-ckan","vers":"0.4.0","deps":[],"cksum":"1111111111111111111111111111111111111111111111111111111111111111","features":{},"yanked":false}
BODY

# The same shape, carrying a version that matches 0.5.0 only if the dots are
# read as regex wildcards.
cat > "$tmp/index-regex-trap.txt" <<'BODY'
{"name":"data-gov-ckan","vers":"0X5X0","deps":[],"cksum":"2222222222222222222222222222222222222222222222222222222222222222","features":{},"yanked":false}
BODY

# Keep the retries and the polling short. The defaults are tuned for a real
# release; a test must not spend minutes proving a guard fires.
export CRATES_INDEX_ATTEMPTS=3
export CRATES_INDEX_RETRY_DELAY=0
export CRATES_WAIT_ATTEMPTS=3
export CRATES_WAIT_DELAY=0

index_has() {
  CRATES_INDEX_BASE_URL="$1" "$here/index-has.sh" "$2" "$3"
}

publish_crate() {
  CRATES_INDEX_BASE_URL="$1" "$here/publish-crate.sh" "$2" "$3"
}

wait_for_crate() {
  CRATES_INDEX_BASE_URL="$1" "$here/wait-for-crate.sh" "$2" "$3"
}

echo "== index-has.sh =="

start_stub --mode ok --body "$tmp/index-body.txt"
want reports_published_when_the_index_lists_the_version \
  "$PUBLISHED" index_has "$stub_base" data-gov-ckan 0.4.0

want reports_not_published_when_the_index_omits_the_version \
  "$NOT_PUBLISHED" index_has "$stub_base" data-gov-ckan 0.5.0

start_stub --mode missing
want reports_not_published_when_the_index_returns_404 \
  "$NOT_PUBLISHED" index_has "$stub_base" data-gov-ckan 0.5.0

want reports_unknown_when_the_index_is_unreachable \
  "$UNKNOWN" index_has "$(unreachable_base)" data-gov-ckan 0.5.0
and_output_has reports_unknown_when_the_index_is_unreachable_and_says_so \
  "could not be reached"

start_stub --mode server-error
want reports_unknown_when_the_index_returns_a_server_error \
  "$UNKNOWN" index_has "$stub_base" data-gov-ckan 0.5.0

start_stub --mode status --status 429
want reports_unknown_when_the_index_rate_limits_the_probe \
  "$UNKNOWN" index_has "$stub_base" data-gov-ckan 0.5.0

start_stub --mode flaky --fail-first 2 --body "$tmp/index-body.txt"
want retries_a_transient_server_error_before_answering \
  "$PUBLISHED" index_has "$stub_base" data-gov-ckan 0.4.0

start_stub --mode ok --body "$tmp/index-regex-trap.txt"
want matches_the_version_literally_rather_than_as_a_regex \
  "$NOT_PUBLISHED" index_has "$stub_base" data-gov-ckan 0.5.0

echo "== publish-crate.sh =="

start_stub --mode ok --body "$tmp/index-body.txt"
want publish_skips_the_crate_when_the_index_lists_the_version \
  0 publish_crate "$stub_base" data-gov-ckan 0.4.0
and_cargo_did_not_run publish_skips_the_crate_without_calling_cargo

start_stub --mode missing
want publish_runs_cargo_when_the_index_does_not_list_the_version \
  0 publish_crate "$stub_base" data-gov-ckan 0.5.0
and_cargo_ran publish_runs_cargo_when_the_index_does_not_list_the_version_call

want publish_refuses_when_the_index_state_is_unknown \
  1 publish_crate "$(unreachable_base)" data-gov-ckan 0.5.0
and_cargo_did_not_run publish_refuses_when_the_index_state_is_unknown_no_cargo
and_output_has publish_names_the_unknown_index_state_in_its_error \
  "could not be reached"

start_stub --mode server-error
want publish_refuses_when_the_index_returns_a_server_error \
  1 publish_crate "$stub_base" data-gov-ckan 0.5.0
and_cargo_did_not_run publish_refuses_on_a_server_error_without_calling_cargo

echo "== wait-for-crate.sh =="

start_stub --mode late --fail-first 1 --body "$tmp/index-body.txt"
want wait_returns_once_the_version_appears \
  0 wait_for_crate "$stub_base" data-gov-ckan 0.4.0

start_stub --mode missing
want wait_fails_when_the_version_never_appears \
  1 wait_for_crate "$stub_base" data-gov-ckan 0.5.0

want wait_fails_when_the_index_stays_unreachable \
  1 wait_for_crate "$(unreachable_base)" data-gov-ckan 0.5.0
and_output_has wait_names_the_unreachable_index_rather_than_a_missing_publish \
  "could not be reached"

stop_stub
echo
echo "passed: ${passed}, failed: ${failed}"
[ "$failed" -eq 0 ]
