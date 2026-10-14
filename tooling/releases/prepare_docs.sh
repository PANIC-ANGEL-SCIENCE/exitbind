#!/bin/sh
# Render locally, then check release references. Review and commit separately.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
python3 "$root/tooling/releases/sync_docs.py" --write "$@"
cd "$root"
python3 tooling/releases/sync_docs.py
sh scripts/check-release-refs.sh
printf '%s\n' 'Documentation prepared locally. Review and commit through the existing release process.'
