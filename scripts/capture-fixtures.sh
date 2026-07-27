#!/usr/bin/env bash
# Capture a fresh set of Catalog API responses into data-gov-catalog/tests/fixtures/.
#
# Fixtures are the project's record of what the API actually returns. Refresh
# them before changing any model struct, and prove the change against the new
# capture rather than against the code (see CLAUDE.md, "Fixtures").
#
# Usage: scripts/capture-fixtures.sh [base_url]
set -euo pipefail

BASE="${1:-https://catalog.data.gov}"
OUT="$(cd "$(dirname "$0")/.." && pwd)/data-gov-catalog/tests/fixtures"
mkdir -p "$OUT"

get() {  # get <path-with-query> <fixture-name>
  local url="$BASE$1" dest="$OUT/$2" tmp
  tmp="$(mktemp)"
  if curl -sS --fail --max-time 60 -H 'Accept: application/json' "$url" > "$tmp" 2>/dev/null \
     && python3 -m json.tool --indent 2 < "$tmp" > "$dest.new" 2>/dev/null; then
    mv "$dest.new" "$dest"
    echo "  ok    $2  <-  $1"
  else
    # Write to a temp file first and only move on success, so a failed refresh
    # never truncates a good fixture. A skip is a finding, not an error: it may
    # mean the endpoint changed. Investigate before editing any model.
    rm -f "$dest.new"
    echo "  SKIP  $2  <-  $1  (request failed; existing fixture left untouched)"
    SKIPPED=$((SKIPPED + 1))
  fi
  rm -f "$tmp"
}
SKIPPED=0

# Slug-addressed fixtures are pinned to a specific long-lived dataset, because
# tests assert against their contents and a fixture whose subject changes every
# capture cannot be asserted on. Discover only the ids that are opaque.
PINNED_SLUG="crime-data-from-2020-to-present"

HIT=$(curl -sS --fail --max-time 60 -H 'Accept: application/json' "$BASE/search?per_page=1")
HARVEST=$(printf '%s' "$HIT" | python3 -c 'import sys,json;print(json.load(sys.stdin)["results"][0]["harvest_record"].rstrip("/").split("/")[-1])')
LOCATION=$(curl -sS --fail --max-time 60 -H 'Accept: application/json' "$BASE/api/locations/search?q=california" \
  | python3 -c 'import sys,json;print(json.load(sys.stdin)["locations"][0]["id"])')

# If the pinned dataset ever disappears, say so loudly rather than silently
# capturing a fixture that no longer contains what the tests look for.
if ! curl -sS --fail --max-time 60 -H 'Accept: application/json' "$BASE/search?per_page=3&q=$PINNED_SLUG" \
     | python3 -c "import sys,json;sys.exit(0 if any(r.get('slug')=='$PINNED_SLUG' for r in json.load(sys.stdin).get('results') or []) else 1)"; then
  echo "WARNING: pinned dataset '$PINNED_SLUG' no longer resolves."
  echo "Pick a new long-lived dataset, update PINNED_SLUG here and the slug in"
  echo "data-gov-catalog/tests/client_tests.rs, then recapture."
fi

echo "capturing from $BASE (pinned=$PINNED_SLUG harvest=$HARVEST location=$LOCATION)"

get "/search?per_page=3"                          search.json
get "/search?per_page=3&org_slug=nasa"            search_filtered.json
get "/search?per_page=3&q=$PINNED_SLUG"           search_by_slug.json
get "/api/organizations"                          organizations.json
# size/min_count are echoed back in the response and asserted by client_tests.
get "/api/keywords?size=10&min_count=5"           keywords.json
get "/api/locations/search?q=california"          locations_search.json
get "/api/location/$LOCATION"                     location.json
get "/harvest_record/$HARVEST"                    harvest_record.json
get "/harvest_record/$HARVEST/raw"                harvest_record_raw.json
get "/harvest_record/$HARVEST/transformed"        harvest_record_transformed.json

if [ "$SKIPPED" -gt 0 ]; then
  echo
  echo "$SKIPPED endpoint(s) could not be refreshed. Do NOT treat that as licence to"
  echo "drop a field or a method: check whether the endpoint moved, whether the"
  echo "sampled records simply lack that data, and sample several publishers"
  echo "before concluding anything. See CLAUDE.md, 'The live API is the source of truth'."
fi

echo "done. Review the diff: fixtures are the source of truth for model shape."
