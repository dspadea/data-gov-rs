#!/usr/bin/env bash
# Capture a fresh set of CKAN Action API responses into
# data-gov-ckan/tests/fixtures/.
#
# Unlike the Catalog API, CKAN is run by many independent portals, so this
# script captures from SEVERAL live public CKAN instances rather than one base
# URL. Each portal is pinned deliberately, not interchangeably:
#
#   open.canada.ca   Canada's federal open-data portal. Government-run, not
#                     publicly editable, so its content is stable and safe to
#                     commit. Provides the general happy-path shape AND a
#                     resource over 2 GiB (the live acceptance case for #62 --
#                     Resource.size as i32 fails to parse it).
#   data.gov.ie       Ireland's national open-data portal. A block of its
#                     organizations were created with an explicit slug id
#                     instead of CKAN's default make_uuid() -- the live
#                     acceptance case for #63 (entity ids are not always
#                     UUIDs).
#   data.qld.gov.au   Queensland's open-data portal. Emits resource.size as a
#                     human-formatted string ("523 KiB"), not a number -- the
#                     live acceptance case for the string-size half of #62.
#
# demo.ckan.org (CKAN's own public sandbox) was deliberately NOT used: it
# accepts anonymous edits, and at the time of writing its canonical
# "annakarenina" demo dataset had been vandalized with spam content. A fixture
# source that anyone can rewrite is not a fixture source.
#
# Refresh these before changing any CKAN model, and prove the change against
# the new capture rather than against the code (see CLAUDE.md, "Changing a
# model requires fresh fixtures first").
#
# Usage: scripts/capture-ckan-fixtures.sh
set -euo pipefail

OUT="$(cd "$(dirname "$0")/.." && pwd)/data-gov-ckan/tests/fixtures"
mkdir -p "$OUT"

UA="data-gov-rs-ckan-fixtures/1.0 (+https://github.com/dspadea/data-gov-rs)"

SKIPPED=0
LEDGER="$(mktemp)"
trap 'rm -f "$LEDGER"' EXIT

# get <site-root> <path-with-query> <fixture-name> [expected-status]
#
# Captures the response body whatever the status, then checks the status
# against what the caller expected. Error fixtures matter as much as success
# ones: they are what proves the client can tell "this dataset does not exist"
# from "the request was malformed" from "the portal is down".
get() {
  local base="$1" path="$2" name="$3" want="${4:-200}"
  local url="$base$path" dest="$OUT/$name" body code
  body="$(mktemp)"

  code=$(curl -sS --max-time 60 -H 'Accept: application/json' -H "User-Agent: $UA" \
           -o "$body" -w '%{http_code}' "$url" 2>/dev/null || echo "000")

  if [ "$code" = "$want" ] && python3 -m json.tool --indent 2 < "$body" > "$dest.new" 2>/dev/null; then
    mv "$dest.new" "$dest"
    printf '%s\t%s\t%s\t%s\n' "$name" "$path" "$code" "$base" >> "$LEDGER"
    echo "  ok    $name  <-  $base$path  [$code]"
  else
    # Write to a temp file first and only move on success, so a failed
    # refresh never truncates a good fixture. A skip is a finding, not an
    # error: it may mean the portal or endpoint changed. Investigate before
    # editing any model.
    rm -f "$dest.new"
    echo "  SKIP  $name  <-  $base$path  (wanted $want, got $code; existing fixture left untouched)"
    SKIPPED=$((SKIPPED + 1))
  fi
  rm -f "$body"
}

CANADA="https://open.canada.ca"
IE="https://data.gov.ie"
QLD="https://www.data.qld.gov.au"

# Pinned to a permanent statutory dataset (Treasury Board of Canada
# Secretariat's proactive disclosure of grants and contributions), which
# happens to carry a CSV extract over 2 GiB -- the live #62 acceptance case.
CANADA_PKG="432527ab-7aac-45b5-81d6-7597107a7013"
# Pinned to Ireland's Central Statistics Office, a stable organization with a
# non-UUID id ("central-statistics-office") -- the live #63 acceptance case.
IE_ORG="central-statistics-office"
QLD_PKG="coastal-data-system-near-real-time-wave-data"
ABSENT_ID="this-dataset-does-not-exist-abc123xyz"

echo "capturing CKAN fixtures from several live public portals"

# --- open.canada.ca: general shape, error envelopes, oversized resource ----
get "$CANADA" "/data/en/api/3/action/package_search?fq=id:$CANADA_PKG&rows=3" package_search.json
# Also the #62 acceptance fixture: one of its resources is a CSV over 2 GiB.
get "$CANADA" "/data/en/api/3/action/package_show?id=$CANADA_PKG"             package_show.json
get "$CANADA" "/data/en/api/3/action/organization_list?sort=name&limit=5"     organization_list.json
get "$CANADA" "/data/en/api/3/action/package_show?id=$ABSENT_ID"              package_show_not_found.json 404
get "$CANADA" "/data/en/api/3/action/package_show"                            package_show_validation_error.json 409

# --- data.gov.ie: non-UUID entity ids (#63) ---------------------------------
get "$IE" "/api/3/action/organization_list?all_fields=true&sort=name&limit=10" organization_list_slug_ids.json
get "$IE" "/api/3/action/package_search?fq=organization:$IE_ORG&rows=3"        package_search_non_uuid_org_id.json

# --- data.qld.gov.au: resource.size as a formatted string (#62) ------------
get "$QLD" "/api/3/action/package_show?id=$QLD_PKG"                            resource_size_as_string.json

# Sanity checks on the pins above. These are findings, not build failures: a
# pin drifting means "pick a new one and update this script", not "something
# broke in the client".
python3 - "$OUT" <<'PY'
import json, sys, re

out = sys.argv[1]
UUID_RE = re.compile(r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$")


def load(name):
    try:
        with open(f"{out}/{name}") as fh:
            return json.load(fh)
    except FileNotFoundError:
        return None


orgs = load("organization_list_slug_ids.json")
if orgs is not None:
    ids = [o.get("id") for o in orgs.get("result", [])]
    if not any(not UUID_RE.match(str(i)) for i in ids):
        print("WARNING: every captured data.gov.ie organization id is now UUID-shaped.")
        print("The non-UUID pin (#63) has drifted; find another slug-id org and update this script.")

org_pkg = load("package_search_non_uuid_org_id.json")
if org_pkg is not None:
    results = org_pkg.get("result", {}).get("results", [])
    org_ids = [(p.get("organization") or {}).get("id") for p in results]
    if not org_ids or all(UUID_RE.match(str(i)) for i in org_ids):
        print("WARNING: package_search_non_uuid_org_id.json no longer nests a non-UUID organization id.")
        print("Find a new non-UUID organization with packages and update this script's pin.")

canada = load("package_show.json")
if canada is not None:
    sizes = [r.get("size") for r in canada.get("result", {}).get("resources", [])]
    if not any(isinstance(s, int) and s > 2_147_483_647 for s in sizes):
        print("WARNING: the pinned open.canada.ca dataset no longer has a resource over i32::MAX bytes.")
        print("Find a new oversized resource and update this script's CANADA_PKG pin.")

qld = load("resource_size_as_string.json")
if qld is not None:
    sizes = [r.get("size") for r in qld.get("result", {}).get("resources", [])]
    if not any(isinstance(s, str) for s in sizes):
        print("WARNING: the pinned data.qld.gov.au dataset no longer sends size as a string.")
        print("Find a new resource with a string size and update this script's QLD_PKG pin.")
PY

# Record provenance for everything captured this run, merging over any entry
# that was already there so a partial run does not erase what it did not
# touch.
python3 - "$OUT/MANIFEST.json" "$LEDGER" <<'PY'
import json, os, sys
from datetime import date

manifest_path, ledger_path = sys.argv[1], sys.argv[2]

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
        name, path, status, base = line.split("\t")
        fixtures[name] = {
            "endpoint": path,
            "status": int(status),
            "source": base,
            "captured": today,
        }

manifest["fixtures"] = dict(sorted(fixtures.items()))
manifest.setdefault("unverified", {})
manifest["note"] = (
    "Provenance for every file in this directory. Written by "
    "scripts/capture-ckan-fixtures.sh; asserted by fixture_parity_tests.rs. "
    "A fixture with no entry here has unverified provenance and fails the "
    "test. Unlike data-gov-catalog, these fixtures come from several "
    "independent public CKAN portals -- see `source` on each entry, and the "
    "comment block at the top of the capture script for why each was chosen."
)

with open(manifest_path, "w") as fh:
    json.dump(manifest, fh, indent=2, sort_keys=False)
    fh.write("\n")

print(f"manifest: {len(fixtures)} fixtures recorded in {os.path.basename(manifest_path)}")
PY

if [ "$SKIPPED" -gt 0 ]; then
  echo
  echo "$SKIPPED endpoint(s) could not be refreshed. Do NOT treat that as licence to"
  echo "drop a field or a method: check whether the portal or endpoint moved, and"
  echo "look for a replacement portal before concluding anything. See CLAUDE.md,"
  echo "'The live API is the source of truth'."
fi

echo "done. Review the diff: fixtures are the source of truth for model shape."
