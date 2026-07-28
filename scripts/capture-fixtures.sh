#!/usr/bin/env bash
# Capture a fresh set of Catalog API responses into data-gov-catalog/tests/fixtures/.
#
# Fixtures are the project's record of what the API actually returns. Refresh
# them before changing any model struct, and prove the change against the new
# capture rather than against the code (see CLAUDE.md, "Fixtures").
#
# Every fixture this script writes is recorded in fixtures/MANIFEST.json with the
# endpoint that produced it, the HTTP status, and the capture date. A test
# asserts that manifest covers every file in the directory, so a fixture with no
# recorded provenance fails the build rather than sitting there unexplained.
#
# Usage: scripts/capture-fixtures.sh [base_url]
set -euo pipefail

BASE="${1:-https://catalog.data.gov}"
OUT="$(cd "$(dirname "$0")/.." && pwd)/data-gov-catalog/tests/fixtures"
mkdir -p "$OUT"

SKIPPED=0
LEDGER="$(mktemp)"
trap 'rm -f "$LEDGER"' EXIT

# get <path-with-query> <fixture-name> [expected-status]
#
# Captures the response body whatever the status, then checks the status against
# what the caller expected. Error fixtures matter as much as success ones: the
# 404 body is what proves dataset_by_slug can tell "no such dataset" from "the
# API is down", and a hand-written guess at that body would prove nothing.
get() {
  local path="$1" name="$2" want="${3:-200}"
  local url="$BASE$path" dest="$OUT/$name" body code
  body="$(mktemp)"

  code=$(curl -sS --max-time 60 -H 'Accept: application/json' \
           -o "$body" -w '%{http_code}' "$url" 2>/dev/null || echo "000")

  if [ "$code" = "$want" ] && python3 -m json.tool --indent 2 < "$body" > "$dest.new" 2>/dev/null; then
    mv "$dest.new" "$dest"
    printf '%s\t%s\t%s\n' "$name" "$path" "$code" >> "$LEDGER"
    echo "  ok    $name  <-  $path  [$code]"
  else
    # Write to a temp file first and only move on success, so a failed refresh
    # never truncates a good fixture. A skip is a finding, not an error: it may
    # mean the endpoint changed. Investigate before editing any model.
    rm -f "$dest.new"
    echo "  SKIP  $name  <-  $path  (wanted $want, got $code; existing fixture left untouched)"
    SKIPPED=$((SKIPPED + 1))
  fi
  rm -f "$body"
}

# Slug-addressed fixtures are pinned to specific long-lived datasets, because
# tests assert against their contents and a fixture whose subject changes every
# capture cannot be asserted on. Discover only the ids that are opaque.
PINNED_SLUG="crime-data-from-2020-to-present"
# A slug at the 90-character cap, where the title is truncated mid-word. Roughly
# 69% of slugs hit this cap, and it is the case full-text search could not
# recall, so it stays pinned as its own fixture.
PINNED_SLUG_TRUNCATED="advancing-the-automation-of-plant-nucleic-acid-extraction-for-rapid-diagnosis-of-plant-dis"
# Deliberately absent, to capture the 404 envelope.
ABSENT_SLUG="this-dataset-does-not-exist-abc123xyz"
# Matches nothing, to capture the empty-result envelope, which differs from the
# 404 envelope: /search omits total and search_after entirely.
NO_MATCH_QUERY="zzzzqqqxxnomatchwhatsoever12345"

HIT=$(curl -sS --fail --max-time 60 -H 'Accept: application/json' "$BASE/search?per_page=1")
HARVEST=$(printf '%s' "$HIT" | python3 -c 'import sys,json;print(json.load(sys.stdin)["results"][0]["harvest_record"].rstrip("/").split("/")[-1])')
LOCATION=$(curl -sS --fail --max-time 60 -H 'Accept: application/json' "$BASE/api/locations/search?q=california" \
  | python3 -c 'import sys,json;print(json.load(sys.stdin)["locations"][0]["id"])')

# /harvest_record/{id}/transformed 404s when the base record's source_transform
# is null, and that is the common case: an unfiltered /search?per_page=1 (what
# $HARVEST above comes from) lands on a record with no transform roughly 6 times
# out of 7, which is why this fixture used to be a permanent SKIP (#83). census
# and noaa are the two organizations, of the 18 sampled while investigating #83,
# that populate a transform on every record -- census is arbitrary between the
# two. $HARVEST itself is reused for the negative fixture below: it reliably
# 404s (org "ed"), confirmed during that same investigation.
HARVEST_WITH_TRANSFORM=$(curl -sS --fail --max-time 60 -H 'Accept: application/json' \
    "$BASE/search?per_page=1&org_slug=census" \
  | python3 -c 'import sys,json;print(json.load(sys.stdin)["results"][0]["harvest_record"].rstrip("/").split("/")[-1])')

# If a pinned dataset ever disappears, say so loudly rather than silently
# capturing a fixture that no longer contains what the tests look for.
for pinned in "$PINNED_SLUG" "$PINNED_SLUG_TRUNCATED"; do
  if ! curl -sS --fail --max-time 60 -H 'Accept: application/json' "$BASE/api/dataset/$pinned" \
       | python3 -c "import sys,json;sys.exit(0 if any(r.get('slug')=='$pinned' for r in json.load(sys.stdin).get('results') or []) else 1)"; then
    echo "WARNING: pinned dataset '$pinned' no longer resolves."
    echo "Pick a new long-lived dataset, update the pin here and the slug in"
    echo "data-gov-catalog/tests/client_tests.rs, then recapture."
  fi
done

echo "capturing from $BASE (harvest=$HARVEST harvest_with_transform=$HARVEST_WITH_TRANSFORM location=$LOCATION)"

# Success responses.
get "/search?per_page=3"                              search.json
get "/search?per_page=3&org_slug=nasa"                search_filtered.json
get "/search?per_page=3&q=$PINNED_SLUG"               search_by_slug.json
get "/api/dataset/$PINNED_SLUG"                       dataset_by_slug.json
get "/api/dataset/$PINNED_SLUG_TRUNCATED"             dataset_by_slug_truncated.json
get "/api/organizations"                              organizations.json
# size/min_count are echoed back in the response and asserted by client_tests.
get "/api/keywords?size=10&min_count=5"               keywords.json
get "/api/locations/search?q=california"              locations_search.json
get "/api/location/$LOCATION"                         location.json
get "/harvest_record/$HARVEST"                        harvest_record.json
get "/harvest_record/$HARVEST/raw"                    harvest_record_raw.json
get "/harvest_record/$HARVEST_WITH_TRANSFORM/transformed" harvest_record_transformed.json

# Negative responses. These are the shapes the client must handle without
# treating a normal "no" as a failure.
get "/api/dataset/$ABSENT_SLUG"                       dataset_not_found.json      404
get "/search?per_page=3&q=$NO_MATCH_QUERY"            search_no_matches.json      200
# The common case for this endpoint (#83): most harvest records have no
# populated transform, and $HARVEST (an unfiltered search hit) reliably lands
# on one of them.
get "/harvest_record/$HARVEST/transformed"            harvest_record_transformed_not_found.json 404

# Record provenance for everything captured this run, merging over any entry that
# was already there so a partial run does not erase what it did not touch.
python3 - "$OUT/MANIFEST.json" "$BASE" "$LEDGER" <<'PY'
import json, os, sys
from datetime import date

manifest_path, base, ledger_path = sys.argv[1], sys.argv[2], sys.argv[3]

try:
    with open(manifest_path) as fh:
        manifest = json.load(fh)
except (OSError, ValueError):
    manifest = {}

fixtures = manifest.get("fixtures", {})
today = date.today().isoformat()

with open(ledger_path) as fh:
    for line in fh:
        line = line.rstrip("\n")
        if not line:
            continue
        name, path, status = line.split("\t")
        fixtures[name] = {
            "endpoint": path,
            "status": int(status),
            "source": base,
            "captured": today,
        }

manifest["fixtures"] = dict(sorted(fixtures.items()))
manifest["note"] = (
    "Provenance for every file in this directory. Written by "
    "scripts/capture-fixtures.sh; asserted by fixture_parity_tests.rs. "
    "A fixture with no entry here has unverified provenance and fails the test."
)

with open(manifest_path, "w") as fh:
    json.dump(manifest, fh, indent=2, sort_keys=False)
    fh.write("\n")

print(f"manifest: {len(fixtures)} fixtures recorded in {os.path.basename(manifest_path)}")
PY

if [ "$SKIPPED" -gt 0 ]; then
  echo
  echo "$SKIPPED endpoint(s) could not be refreshed. Do NOT treat that as licence to"
  echo "drop a field or a method: check whether the endpoint moved, whether the"
  echo "sampled records simply lack that data, and sample several publishers"
  echo "before concluding anything. See CLAUDE.md, 'The live API is the source of truth'."
fi

echo "done. Review the diff: fixtures are the source of truth for model shape."
