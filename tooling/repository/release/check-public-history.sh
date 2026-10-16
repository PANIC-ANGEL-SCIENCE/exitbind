#!/bin/sh
set -eu

# Organization import baseline; contributor checks do not authorize replacing it.
checkpoint=cde4c003ad885b99ec8e405c8bc80e89935bb050

fail() {
  printf '%s\n' "public history check failed: $*" >&2
  exit 1
}

# Inspect stored parentage, not local replacement or graft views.
export GIT_NO_REPLACE_OBJECTS=1
export GIT_GRAFT_FILE=/dev/null
test "$(git rev-parse --is-inside-work-tree)" = true ||
  fail 'a Git worktree is required'
test "$(git rev-parse --is-shallow-repository)" = false ||
  fail 'complete Git history is required; fetch the full history'
git cat-file -e "$checkpoint^{commit}" ||
  fail "published checkpoint $checkpoint is unavailable"
# A CI merge can retain main even when its contributing PR head dropped history.
# Additional candidates supplement the mandatory native HEAD check.
for candidate in HEAD "$@"; do
  test -n "$candidate" || fail 'candidate revision is missing'
  commit=$(git rev-parse --verify --end-of-options "$candidate^{commit}") ||
    fail "candidate $candidate is unavailable"
  git merge-base --is-ancestor "$checkpoint" "$commit" ||
    fail "$candidate does not retain published checkpoint $checkpoint"
done
