#!/usr/bin/env bash
# Ask the crates.io sparse index whether <name> <version> is published.
#
# The answer is three-valued, not two, and the third value is the point of
# this script. A publish to crates.io cannot be undone, only yanked, and
# yanking does not free the version number - so "I could not tell" must never
# collapse into "not published", which is an instruction to publish.
#
#   exit 0  published   - the index answered and lists this version
#   exit 1  absent      - the index answered and does not list this version,
#                         or has never heard of the crate (404)
#   exit 2  unknown     - the index could not be reached, or answered with
#                         something other than 200 or 404, and kept doing so
#                         across every retry. The caller must stop.
#
# The status comes from curl's -w '%{http_code}', not from curl's exit code
# alone: a 500, a reset connection and a genuine 404 are one single non-zero
# exit to curl, and telling them apart is the whole job here.
#
# Environment:
#   CRATES_INDEX_BASE_URL     index root (default https://index.crates.io)
#   CRATES_INDEX_ATTEMPTS     probes before declaring unknown (default 5)
#   CRATES_INDEX_RETRY_DELAY  seconds between probes (default 3)
#   CRATES_INDEX_TIMEOUT      seconds allowed per probe (default 20)

set -uo pipefail

readonly EXIT_PUBLISHED=0
readonly EXIT_ABSENT=1
readonly EXIT_UNKNOWN=2

if [ "$#" -ne 2 ]; then
  echo "usage: index-has.sh <crate-name> <version>" >&2
  exit "$EXIT_UNKNOWN"
fi

name="$1"
version="$2"
base="${CRATES_INDEX_BASE_URL:-https://index.crates.io}"
attempts="${CRATES_INDEX_ATTEMPTS:-5}"
delay="${CRATES_INDEX_RETRY_DELAY:-3}"
timeout_seconds="${CRATES_INDEX_TIMEOUT:-20}"

# The sparse index shards crate files by name length; see the cargo book,
# "Registry index" - Index files.
len=${#name}
if [ "$len" -ge 4 ]; then
  path="${name:0:2}/${name:2:2}/${name}"
elif [ "$len" -eq 3 ]; then
  path="3/${name:0:1}/${name}"
elif [ "$len" -eq 2 ]; then
  path="2/${name}"
else
  path="1/${name}"
fi
url="${base}/${path}"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
body="${work}/body"
curl_error="${work}/curl-error"

# Matched with grep -F so the version is a literal. Read as a regex, the dots
# in 0.5.0 are wildcards and the released 0X5X0 of some other crate would
# answer for it.
needle="\"vers\":\"${version}\""

reason="no probe was attempted"
for attempt in $(seq 1 "$attempts"); do
  curl_status=0
  http_code="$(
    curl --silent --show-error --max-time "$timeout_seconds" \
      --output "$body" --write-out '%{http_code}' "$url" 2>"$curl_error"
  )" || curl_status=$?

  if [ "$curl_status" -ne 0 ]; then
    reason="curl exited ${curl_status}: $(tr '\n' ' ' <"$curl_error")"
  else
    case "$http_code" in
      200)
        if grep -qF -- "$needle" "$body"; then
          exit "$EXIT_PUBLISHED"
        fi
        exit "$EXIT_ABSENT"
        ;;
      404)
        exit "$EXIT_ABSENT"
        ;;
      *)
        reason="the index answered HTTP ${http_code}"
        ;;
    esac
  fi

  if [ "$attempt" -lt "$attempts" ]; then
    echo "index probe ${attempt}/${attempts} for ${name} ${version} failed (${reason}); retrying in ${delay}s..." >&2
    sleep "$delay"
  fi
done

echo "::error::the crates.io index could not be reached for ${name} ${version}: ${reason} (${attempts} attempts against ${url}). Whether this version is already published is unknown, and a publish cannot be undone, so nothing is published from here. Re-run once the index answers." >&2
exit "$EXIT_UNKNOWN"
