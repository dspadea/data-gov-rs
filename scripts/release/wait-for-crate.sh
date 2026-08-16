#!/usr/bin/env bash
# Block until <name> <version> is resolvable from the crates.io index.
#
# Polls rather than sleeping a fixed amount: crates.io serializes index
# updates and the real delay varies with load, so a fixed sleep is either too
# short under load or wastes minutes when the index is already current.
#
# An index that cannot be reached is a reason to keep waiting, not to stop -
# waiting costs nothing and cannot be undone the way a publish can. The
# ceiling still applies, and the error at the end names which of the two
# happened, because "the version never appeared" and "the index never
# answered" call for different responses from whoever reads it.
#
# Environment:
#   CRATES_WAIT_ATTEMPTS  polls before giving up (default 40)
#   CRATES_WAIT_DELAY     seconds between polls (default 15)

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [ "$#" -ne 2 ]; then
  echo "usage: wait-for-crate.sh <crate-name> <version>" >&2
  exit 1
fi

name="$1"
version="$2"
max_attempts="${CRATES_WAIT_ATTEMPTS:-40}"
delay="${CRATES_WAIT_DELAY:-15}"

echo "Waiting for ${name} ${version} to reach the crates.io index..."

state="not yet visible"
advice="If crates.io is healthy, the publish may have failed silently - check the publish step above before re-running."
for attempt in $(seq 1 "$max_attempts"); do
  probe=0
  "${here}/index-has.sh" "$name" "$version" || probe=$?
  case "$probe" in
    0)
      echo "${name} ${version} is visible in the crates.io index."
      exit 0
      ;;
    1)
      state="not yet visible"
      advice="If crates.io is healthy, the publish may have failed silently - check the publish step above before re-running."
      ;;
    *)
      state="the index could not be reached"
      advice="The publish itself may well have succeeded; confirm on crates.io before re-running."
      ;;
  esac
  echo "Attempt ${attempt}/${max_attempts}: ${state}, waiting ${delay}s..."
  sleep "$delay"
done

echo "::error::${name} ${version} did not appear in the crates.io index within $((max_attempts * delay))s: ${state}. ${advice}" >&2
exit 1
