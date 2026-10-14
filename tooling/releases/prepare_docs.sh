#!/bin/sh
# Render documentation during release preparation; never commit or publish.
set -eu
cd "$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)"
case "${1:-}" in
  '')
    test "$#" -eq 0 || { echo 'usage: prepare_docs.sh [--published]' >&2; exit 1; }
    python3 tooling/releases/sync_docs.py --write
    ;;
  --published)
    test "$#" -eq 1 || { echo 'usage: prepare_docs.sh [--published]' >&2; exit 1; }
    test -z "$(git status --porcelain)" || { echo 'Published documentation checkout must be clean' >&2; exit 1; }
    python3 tooling/releases/sync_docs.py --write --from-github
    ;;
  *) echo 'usage: prepare_docs.sh [--published]' >&2; exit 1 ;;
esac
python3 tooling/releases/sync_docs.py
sh scripts/check-release-refs.sh
printf '%s\n' 'Documentation prepared. Review and commit the changes through the existing release review process.'
