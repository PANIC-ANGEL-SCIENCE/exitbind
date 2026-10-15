#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
helper=$root/scripts/ci-selected-history.sh
tmp_root=${TMPDIR:-/tmp}
work=$(mktemp -d "$tmp_root/ci-selected-history.XXXXXX")
cleanup() {
  rm -rf "$work"
}
trap cleanup EXIT HUP INT TERM

fail() {
  printf 'ci-selected-history regression: %s\n' "$*" >&2
  exit 1
}

git -C "$work" init --bare remote.git >/dev/null
git init "$work/source" >/dev/null
git -C "$work/source" config user.name 'CI history fixture'
git -C "$work/source" config user.email 'ci-history@example.invalid'
printf 'root\n' > "$work/source/history.txt"
git -C "$work/source" add history.txt
git -C "$work/source" commit -m root >/dev/null
checkpoint=$(git -C "$work/source" rev-parse HEAD)

git -C "$work/source" checkout -b candidate >/dev/null
printf 'candidate\n' >> "$work/source/history.txt"
git -C "$work/source" commit -am candidate >/dev/null
candidate=$(git -C "$work/source" rev-parse HEAD)

git -C "$work/source" checkout -b selected "$checkpoint" >/dev/null
printf 'selected\n' >> "$work/source/history.txt"
git -C "$work/source" commit -am selected >/dev/null
head=$(git -C "$work/source" rev-parse HEAD)

git -C "$work/source" checkout -b proof-base "$checkpoint" >/dev/null
printf 'proof base\n' > "$work/source/base.txt"
git -C "$work/source" add base.txt
git -C "$work/source" commit -m proof-base >/dev/null
base=$(git -C "$work/source" rev-parse HEAD)

git -C "$work/source" checkout -b archive "$checkpoint" >/dev/null
printf 'unrelated archive\n' > "$work/source/archive.txt"
git -C "$work/source" add archive.txt
git -C "$work/source" commit -m archive >/dev/null
archive=$(git -C "$work/source" rev-parse HEAD)
git -C "$work/source" tag archive-tag "$archive"
git -C "$work/source" remote add origin "$work/remote.git"
git -C "$work/source" push origin candidate selected proof-base archive --tags >/dev/null

git ls-remote --heads --tags "$work/remote.git" | sort > "$work/upstream.before"
git clone --depth=1 --branch selected --single-branch "file://$work/remote.git" "$work/selected" >/dev/null 2>&1

selected_refs() {
  git -C "$work/selected" for-each-ref --format='%(refname) %(objectname)' | sort
}

selected_refs > "$work/refs.before"
test "$(git -C "$work/selected" rev-parse --is-shallow-repository)" = true ||
  fail 'selected fixture clone did not begin shallow'

run_private() {
  (
    cd "$work/selected"
    GITHUB_REPOSITORY_PRIVATE=true GITHUB_SHA="$head" CANDIDATE_SHA="$candidate" \
      VALUE_PROOF_BASE="$base" "$helper"
  )
}

run_private
test "$(git -C "$work/selected" rev-parse --is-shallow-repository)" = false ||
  fail 'selected history remained shallow'
git -C "$work/selected" merge-base --is-ancestor "$checkpoint" HEAD ||
  fail 'selected HEAD does not include the complete checkpoint ancestry'
git -C "$work/selected" cat-file -e "$candidate^{commit}" || fail 'separate candidate was not fetched'
git -C "$work/selected" cat-file -e "$base^{commit}" || fail 'separate proof base was not fetched'
if git -C "$work/selected" cat-file -e "$archive^{commit}" 2>/dev/null; then
  fail 'unrelated archive commit was fetched'
fi
selected_refs > "$work/refs.after"
cmp -s "$work/refs.before" "$work/refs.after" || fail 'helper changed local refs'
git ls-remote --heads --tags "$work/remote.git" | sort > "$work/upstream.after"
cmp -s "$work/upstream.before" "$work/upstream.after" || fail 'helper changed upstream refs'

git clone --depth=1 --branch selected --single-branch "file://$work/remote.git" "$work/non-private" >/dev/null 2>&1
for private_value in false '' unknown; do
  (
    cd "$work/non-private"
    GITHUB_REPOSITORY_PRIVATE="$private_value" GITHUB_SHA=bad CANDIDATE_SHA=missing \
      VALUE_PROOF_BASE=missing "$helper"
  )
  test "$(git -C "$work/non-private" rev-parse --is-shallow-repository)" = true ||
    fail 'non-private or unknown gate unexpectedly fetched history'
done

expect_private_failure() {
  label=$1
  shift
  if (cd "$work/selected" && GITHUB_REPOSITORY_PRIVATE=true "$@" "$helper") \
      > "$work/failure.out" 2>&1; then
    fail "$label unexpectedly succeeded"
  fi
}

expect_private_failure 'exact HEAD mismatch' env GITHUB_SHA="$candidate" CANDIDATE_SHA="$candidate" VALUE_PROOF_BASE="$base"
expect_private_failure 'malformed candidate SHA' env GITHUB_SHA="$head" CANDIDATE_SHA=not-a-sha VALUE_PROOF_BASE="$base"
expect_private_failure 'missing candidate SHA' env GITHUB_SHA="$head" VALUE_PROOF_BASE="$base"
expect_private_failure 'malformed proof-base SHA' env GITHUB_SHA="$head" CANDIDATE_SHA="$candidate" VALUE_PROOF_BASE=not-a-sha
expect_private_failure 'unavailable candidate' env GITHUB_SHA="$head" CANDIDATE_SHA=1111111111111111111111111111111111111111 VALUE_PROOF_BASE="$base"
expect_private_failure 'unavailable proof base' env GITHUB_SHA="$head" CANDIDATE_SHA="$candidate" VALUE_PROOF_BASE=2222222222222222222222222222222222222222

# An absent or all-zero base is valid for events without a declared base; HEAD's
# full ancestry still supplies ci-local's existing HEAD^ fallback.
git clone --depth=1 --branch selected --single-branch "file://$work/remote.git" "$work/no-base" >/dev/null 2>&1
(
  cd "$work/no-base"
  unset VALUE_PROOF_BASE
  GITHUB_REPOSITORY_PRIVATE=true GITHUB_SHA="$head" CANDIDATE_SHA="$candidate" "$helper"
)
test "$(git -C "$work/no-base" rev-parse --is-shallow-repository)" = false ||
  fail 'omitted proof base did not retain complete HEAD history'
git -C "$work/no-base" cat-file -e "$checkpoint^{commit}" || fail 'omitted-base flow lacks HEAD ancestry'

git clone --depth=1 --branch selected --single-branch "file://$work/remote.git" "$work/zero-base" >/dev/null 2>&1
(
  cd "$work/zero-base"
  GITHUB_REPOSITORY_PRIVATE=true GITHUB_SHA="$head" CANDIDATE_SHA="$candidate" \
    VALUE_PROOF_BASE=0000000000000000000000000000000000000000 "$helper"
)
test "$(git -C "$work/zero-base" rev-parse --is-shallow-repository)" = false ||
  fail 'all-zero proof base did not retain complete HEAD history'
git -C "$work/zero-base" cat-file -e "$checkpoint^{commit}" || fail 'all-zero-base flow lacks HEAD ancestry'

# A shallow origin cannot supply the selected ancestry; the helper must report failure.
git clone --depth=1 --branch selected --single-branch "file://$work/remote.git" "$work/shallow-origin" >/dev/null 2>&1
git clone --depth=1 --branch selected --single-branch "file://$work/shallow-origin" "$work/shallow-selected" >/dev/null 2>&1
git -C "$work/shallow-selected" remote set-url origin "file://$work/shallow-origin"
if (
  cd "$work/shallow-selected"
  GITHUB_REPOSITORY_PRIVATE=true GITHUB_SHA="$head" CANDIDATE_SHA="$candidate" \
    VALUE_PROOF_BASE="$base" "$helper"
) > "$work/shallow.out" 2>&1; then
  fail 'helper accepted history from a shallow origin'
fi
test "$(git -C "$work/shallow-selected" rev-parse --is-shallow-repository)" = true ||
  fail 'shallow-origin failure did not retain the unresolved shallow state'

workflow=$root/.github/workflows/ci.yml
conditional_count=$(grep -F -c 'fetch-depth: ${{ github.event.repository.private == true && 1 || 0 }}' "$workflow")
test "$conditional_count" -eq 3 || fail 'public/unknown depth-zero checkout behavior drifted from scope, readme and Linux test jobs'
grep -F 'Complete selected Git history' "$workflow" >/dev/null || fail 'private selected-history helper is not wired into CI'

printf 'ci-selected-history regression passed\n'
