#!/bin/sh
set -eu

# Private GitHub checkouts start shallow, then fetch only the selected commit DAGs.
# Public and unknown repositories retain actions/checkout's full-ref behavior.
test "${GITHUB_REPOSITORY_PRIVATE:-}" = true || exit 0

fail() {
  printf 'ci-selected-history: %s\n' "$*" >&2
  exit 1
}

valid_sha() {
  case "$1" in
    *[!0123456789abcdef]*|'') return 1 ;;
  esac
  test "${#1}" -eq 40 &&
    test "$1" != 0000000000000000000000000000000000000000
}

head_sha=${GITHUB_SHA:-}
candidate_sha=${CANDIDATE_SHA:-}
base_sha=${VALUE_PROOF_BASE:-}

valid_sha "$head_sha" || fail 'GITHUB_SHA must be an explicit 40-character commit SHA'
valid_sha "$candidate_sha" || fail 'CANDIDATE_SHA must be an explicit 40-character commit SHA'
if [ -n "$base_sha" ] && [ "$base_sha" != 0000000000000000000000000000000000000000 ]; then
  valid_sha "$base_sha" || fail 'VALUE_PROOF_BASE must be a commit SHA or all zeroes'
else
  base_sha=
fi

repo=$(git rev-parse --show-toplevel 2>/dev/null) || fail 'a Git worktree is required'
cd "$repo"
actual_head=$(git rev-parse --verify --end-of-options 'HEAD^{commit}') || fail 'HEAD is not a commit'
test "$actual_head" = "$head_sha" || fail 'checkout HEAD does not match GITHUB_SHA'

fetch_commit() {
  sha=$1
  label=$2
  if [ "$(git rev-parse --is-shallow-repository)" = true ]; then
    git fetch --no-tags --unshallow origin "$sha" || fail "could not fetch complete $label history"
  else
    git fetch --no-tags --no-write-fetch-head origin "$sha" || fail "could not fetch $label commit"
  fi
  test "$(git rev-parse --is-shallow-repository)" = false ||
    fail "repository remains shallow after fetching $label history"
  fetched=$(git rev-parse --verify --end-of-options "$sha^{commit}") || fail "$label commit is unavailable"
  test "$fetched" = "$sha" || fail "$label resolved to a different commit"
}

fetch_commit "$head_sha" 'checkout HEAD'
fetch_commit "$candidate_sha" 'candidate'
if [ -n "$base_sha" ]; then
  fetch_commit "$base_sha" 'proof base'
fi

test "$(git rev-parse --is-shallow-repository)" = false || fail 'complete selected history is unavailable'
actual_head=$(git rev-parse --verify --end-of-options 'HEAD^{commit}') || fail 'HEAD is not a commit after history fetch'
test "$actual_head" = "$head_sha" || fail 'checkout HEAD changed during selected-history fetch'
printf 'ci-selected-history: selected commit histories are available\n'
