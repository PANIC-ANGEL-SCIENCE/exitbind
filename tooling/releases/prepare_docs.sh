#!/bin/sh
# Called on trusted main after assets are uploaded. Produce a reviewable patch.
set -eu
patch=${1:?usage: prepare_docs.sh PATCH_PATH}
test "${GITHUB_REPOSITORY:-}" = PANIC-ANGEL-SCIENCE/exitbind
test -z "$(git status --porcelain)" || { echo 'Documentation checkout must be clean' >&2; exit 1; }
python3 tooling/releases/sync_docs.py --write --from-github
git diff --binary > "$patch"
python3 tooling/releases/sync_docs.py
sh scripts/check-release-refs.sh
printf 'Documentation patch prepared from %s: %s\n' "$(git rev-parse HEAD)" "$patch"
printf '%s\n' 'Remote publication requires the normal authorized review path; no branch or protection was changed.'
