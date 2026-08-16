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
#
# The substring selects runs. A run's follow-up assertions come with it,
# whether or not their own names match, because they have nothing to read
# without it.

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
skipped=0

# Every `and_*` assertion reads the capture that the most recent `want` left
# behind, so the filter decides once, at the `want`, and the assertions after
# it inherit that decision. An assertion admitted by a filter that excluded
# its own run would otherwise read whatever capture happened to be on disk -
# some earlier case's, or none - and report a pass on another test's evidence.
last_want_name=""
last_want_ran=0

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
  : > "$tmp/probe-count"
  python3 "$here/stub-index-server.py" --count-file "$tmp/probe-count" "$@" \
    > "$tmp/stub.out" 2> "$tmp/stub.err" &
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
  last_want_name="$name"
  last_want_ran=0
  if [ -n "$filter" ] && [[ "$name" != *"$filter"* ]]; then
    return 0
  fi
  last_want_ran=1
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

# True when the run an `and_*` is about to inspect actually happened. When it
# did not, the assertion is refused and said to be refused, rather than run
# against a capture belonging to some other case.
inspects_the_last_run() {
  local name="$1"
  if [ "$last_want_ran" -eq 1 ]; then
    return 0
  fi
  if [ -z "$last_want_name" ]; then
    printf 'FAIL %s: nothing to inspect - no run came before it\n' "$name"
    failed=$((failed + 1))
    return 1
  fi
  skipped=$((skipped + 1))
  # Only say so when the filter named this assertion. Then the operator asked
  # for a check whose run was left behind, and silence would look like a pass.
  # Otherwise the filter is simply doing its job, and a line per case is noise.
  if [[ "$name" == *"$filter"* ]]; then
    printf 'skip %s: the run it inspects (%s) is outside the filter %s\n' \
      "$name" "$last_want_name" "$filter"
  fi
  return 1
}

# Assertions about the run `want` just made. Kept separate so one case can
# check both the exit status and what the operator was told.
and_output_has() {
  local name="$1" needle="$2"
  inspects_the_last_run "$name" || return 0
  if grep -qF -- "$needle" "$tmp/out"; then
    printf 'ok   %s\n' "$name"
    passed=$((passed + 1))
  else
    printf 'FAIL %s: output does not mention %s\n' "$name" "$needle"
    sed 's/^/       | /' "$tmp/out"
    failed=$((failed + 1))
  fi
}

and_probes_numbered() {
  local name="$1" expected="$2"
  inspects_the_last_run "$name" || return 0
  local actual
  actual="$(cat "$tmp/probe-count" 2>/dev/null)"
  actual="${actual:-0}"
  if [ "$actual" -eq "$expected" ]; then
    printf 'ok   %s\n' "$name"
    passed=$((passed + 1))
  else
    printf 'FAIL %s: expected %s probes, the index saw %s\n' \
      "$name" "$expected" "$actual"
    failed=$((failed + 1))
  fi
}

and_cargo_ran() {
  local name="$1"
  inspects_the_last_run "$name" || return 0
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
  inspects_the_last_run "$name" || return 0
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

# A 200 that is not an index file at all. A captive portal, a proxy sign-in
# page, or a CDN error page served with 200 all look like this.
cat > "$tmp/index-interception.txt" <<'BODY'
<html><head><title>Sign in</title></head>
<body><h1>Sign in to continue</h1></body></html>
BODY

# Keep the retries and the polling short. The defaults are tuned for a real
# release; a test must not spend minutes proving a guard fires.
#
# The per-probe timeout is capped for the same reason, and it is not
# redundant: the unreachable-index cases point curl at 127.0.0.1:1, which
# normally refuses at once, but a host that DROPs instead makes each of those
# fifteen probes sit out the timeout. At the 20s default that is five minutes
# added to `just check`; five seconds is already far longer than a loopback
# needs.
export CRATES_INDEX_ATTEMPTS=3
export CRATES_INDEX_RETRY_DELAY=0
export CRATES_INDEX_TIMEOUT=5
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

# A 200 whose body is not this crate's index file answers no question about
# the crate. Reading it as "absent" would be reading it as an instruction to
# publish, on the one path that cannot be taken back.
start_stub --mode ok --body "$tmp/index-interception.txt"
want reports_unknown_when_a_200_body_is_not_the_index_file \
  "$UNKNOWN" index_has "$stub_base" data-gov-ckan 0.5.0
and_output_has reports_unknown_when_a_200_body_is_not_the_index_file_and_says_so \
  "with a body that is not the index file"

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
# The needle has to be a phrase only publish-crate.sh can produce. The capture
# holds the whole subtree's stderr, and index-has.sh writes its own "the
# crates.io index could not be reached" into it on the way past - so that
# phrase is present whatever publish-crate.sh decides, and asserting on it
# tests nothing about the script this case is named for.
and_output_has publish_names_the_unknown_index_state_in_its_error \
  "::error::refusing to publish data-gov-ckan 0.5.0"

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
and_output_has wait_names_a_missing_publish_rather_than_an_unreachable_index \
  ": not yet visible. If crates.io is healthy, the publish may have failed silently"

# The poll loop is already the retry, so letting the probe retry inside it
# would multiply the wait ceiling by the probe's attempt count. The bound is
# stated in the error message this script prints, so it has to be real.
start_stub --mode server-error
CRATES_INDEX_ATTEMPTS="" \
  want wait_does_not_multiply_its_ceiling_by_the_probe_retries \
  1 wait_for_crate "$stub_base" data-gov-ckan 0.5.0
and_probes_numbered wait_spends_one_probe_per_poll 3

want wait_fails_when_the_index_stays_unreachable \
  1 wait_for_crate "$(unreachable_base)" data-gov-ckan 0.5.0
# The two cases above are the two halves of one property: the final message
# says which of "the version never appeared" and "the index never answered"
# happened, because they call for different responses. Each needle is the
# whole tail of that message, state and advice together, so collapsing either
# branch into the other fails one of them.
#
# "could not be reached" on its own cannot do that job. index-has.sh runs
# inside the poll loop and prints its own copy of that phrase on every poll,
# into the same captured stream, so the needle is satisfied no matter what
# wait-for-crate.sh concludes.
and_output_has wait_names_the_unreachable_index_rather_than_a_missing_publish \
  ": the index could not be reached. The publish itself may well have succeeded"

stop_stub
echo
echo "passed: ${passed}, failed: ${failed}, skipped: ${skipped}"
if [ -n "$filter" ] && [ "$((passed + failed))" -eq 0 ]; then
  echo "no run matched the filter '${filter}'; nothing was checked." >&2
  exit 1
fi
[ "$failed" -eq 0 ]
