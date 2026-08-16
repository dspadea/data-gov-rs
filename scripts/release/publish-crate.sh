#!/usr/bin/env bash
# Publish <name> <version> to crates.io, unless it is already there.
#
# This makes a re-run of the publish job resume rather than jam. A crates.io
# publish cannot be undone, only yanked, so a job that fails partway through
# would otherwise re-attempt the publishes that already succeeded and
# hard-fail on "crate version already exists", blocking every crate behind it.
#
# An index that cannot be answered for is not treated as "not published".
# That mistake publishes on a network blip and produces the very failure the
# skip exists to prevent, so this stops and asks for an operator instead.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [ "$#" -ne 2 ]; then
  echo "usage: publish-crate.sh <crate-name> <version>" >&2
  exit 1
fi

name="$1"
version="$2"

probe=0
"${here}/index-has.sh" "$name" "$version" || probe=$?

case "$probe" in
  0)
    echo "${name} ${version} is already on crates.io; skipping."
    exit 0
    ;;
  1)
    ;;
  *)
    echo "::error::refusing to publish ${name} ${version}: the crates.io index did not answer, so whether this version is already published is unknown (probe exited ${probe}; the reason is above). Publishing blind risks a duplicate publish, which cannot be undone. Check crates.io, then re-run this job." >&2
    exit 1
    ;;
esac

echo "Publishing ${name} ${version}..."
cargo publish -p "$name" --locked
