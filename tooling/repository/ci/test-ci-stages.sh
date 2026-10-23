#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd -P)
cd "$root"
. "$root/tooling/repository/ci/ci-stages.sh"

fail() {
  printf 'ci-stage contract: %s\n' "$*" >&2
  exit 1
}

selected_stages() {
  mode=$1
  platform=$2
  for stage in $(ci_stage_order); do
    if ci_stage_selected "$mode" "$stage" "$platform"; then
      printf '%s\n' "$stage"
    fi
  done
}

expect() {
  mode=$1
  platform=$2
  expected=$3
  actual=$(selected_stages "$mode" "$platform")
  test "$actual" = "$expected" || {
    printf 'mode=%s platform=%s\nexpected:\n%s\nactual:\n%s\n' \
      "$mode" "$platform" "$expected" "$actual" >&2
    fail 'stage membership drifted'
  }
}

pre_return_common='history
workflows
workflow-regression
ci-stage-contract
quiet-command-regression'
pre_return_linux="$pre_return_common
fmt
path-rendering
product-surface
clippy
architecture-budget
privacy-gate
release-integrity
docs-workflow
first-impression
value-contract
plugin-distribution
affected-tests
release-refs"
pre_return_macos="$pre_return_common
architecture-budget
privacy-gate
release-integrity
docs-workflow
first-impression
value-contract
plugin-distribution
affected-tests"
full_linux='history
workflows
workflow-regression
ci-stage-contract
quiet-command-regression
fmt
path-rendering
product-surface
clippy
tests
value-proof
drift-warning
native-handoff
release-refs'
full_macos='history
workflows
workflow-regression
ci-stage-contract
quiet-command-regression
tests
value-proof'

expect pre-return linux "$pre_return_linux"
expect pre-return macos "$pre_return_macos"
expect full linux "$full_linux"
expect full macos "$full_macos"

for stage in history workflows workflow-regression ci-stage-contract quiet-command-regression; do
  ci_stage_selected unexpected "$stage" linux || fail "shared stage omitted: $stage"
done
# This stage runs the two inventory checks below, so it runs everywhere.
for mode in full pre-return; do
  for platform in linux macos; do
    ci_stage_selected "$mode" ci-stage-contract "$platform" ||
      fail "the inventory checks must run on every platform: $mode $platform"
  done
done
for stage in tests value-proof drift-warning native-handoff; do
  if ci_stage_selected pre-return "$stage" linux; then
    fail "full-only stage selected for pre-return: $stage"
  fi
done
for stage in architecture-budget privacy-gate release-integrity docs-workflow first-impression value-contract plugin-distribution affected-tests; do
  ci_stage_selected pre-return "$stage" linux || fail "pre-return invariant omitted: $stage"
  if ci_stage_selected full "$stage" linux; then
    fail "invariant target reruns in full mode: $stage"
  fi
done
if ci_stage_selected full release-refs macos; then
  fail 'release-refs platform applicability changed'
fi
if ci_stage_selected pre-return unknown linux; then
  fail 'unknown stage accepted'
fi
for stage in $(ci_stage_order); do
  grep -F "    $stage)" "$root/tooling/repository/ci/ci-local.sh" >/dev/null ||
    fail "selected stage has no command dispatch: $stage"
done
grep -F 'plugin-distribution) run plugin-distribution "$CARGO" test --locked --features repository-checks --test plugin_distribution ;;' \
  "$root/tooling/repository/ci/ci-local.sh" >/dev/null ||
  fail 'plugin distribution tests must execute behind the repository-checks feature'
grep -F 'run affected-tests sh ./tooling/repository/ci/test-affected.sh --base "$affected_base"' \
  "$root/tooling/repository/ci/ci-local.sh" >/dev/null ||
  fail 'pre-return must run the affected test targets with their declared features'
grep -F 'affected_base=${PRE_RETURN_BASE:-${VALUE_PROOF_BASE:-}}' \
  "$root/tooling/repository/ci/ci-local.sh" >/dev/null ||
  fail 'affected tests must default to PRE_RETURN_BASE, then VALUE_PROOF_BASE'
grep -F 'run focused "$CARGO" test --locked --features repository-checks "$@"' \
  "$root/tooling/repository/ci/test-focused.sh" >/dev/null ||
  fail 'focused tests must include repository-only contract targets'

proof_base_functions=$(sed -n \
  -e '/^proof_base_is_ref() {$/,/^}$/p' \
  -e '/^prepare_value_proof_base() {$/,/^}$/p' \
  "$root/tooling/repository/ci/ci-local.sh")
case $proof_base_functions in
  *'proof_base_is_ref() {'*'prepare_value_proof_base() {'*) ;;
  *) fail 'ci-local.sh proof-base preparation functions not found' ;;
esac
proof_work=$(mktemp -d "${TMPDIR:-/tmp}/ci-proof-base.XXXXXX")
trap 'rm -rf "$proof_work"' EXIT HUP INT TERM
git init -q --bare "$proof_work/remote.git"
git init -q "$proof_work/source"
git -C "$proof_work/source" config user.name 'CI proof-base fixture'
git -C "$proof_work/source" config user.email 'ci-proof-base@example.invalid'
git -C "$proof_work/source" commit -q --allow-empty -m fork
fork=$(git -C "$proof_work/source" rev-parse HEAD)
git -C "$proof_work/source" commit -q --allow-empty -m main-moved
git -C "$proof_work/source" update-ref refs/remotes/pub/main HEAD
git -C "$proof_work/source" checkout -q -b candidate "$fork"
git -C "$proof_work/source" commit -q --allow-empty -m candidate
git -C "$proof_work/source" remote add origin "$proof_work/remote.git"
git -C "$proof_work/source" push -q origin candidate
mkdir "$proof_work/run"
# A fresh shell keeps set -e in force even when the caller tests the status.
prepare_proof_base() {
  (
    cd "$proof_work/source"
    VALUE_PROOF_BASE=$1 run_dir=$proof_work/run platform=fixture \
      PROOF_BASE_FUNCTIONS=$proof_base_functions \
      sh -eu -c 'eval "$PROOF_BASE_FUNCTIONS"; prepare_value_proof_base'
  )
}
resolved=$(prepare_proof_base pub/main 2>&1) || fail "a ref proof base failed: $resolved"
case $resolved in
  *"proof-base=$fork") ;;
  *) fail "a ref proof base must resolve to its merge-base $fork: $resolved" ;;
esac
resolved=$(prepare_proof_base "$fork" 2>&1) || fail "a SHA proof base failed: $resolved"
case $resolved in
  *"proof-base=$fork") ;;
  *) fail "a SHA proof base must be used unchanged: $resolved" ;;
esac
if resolved=$(prepare_proof_base no-such-ref 2>&1); then
  fail "an unknown ref proof base must fail: $resolved"
fi
rm -rf "$proof_work"
trap - EXIT HUP INT TERM

"$root/tooling/repository/ci/test-ci-selected-history.sh"

# Inventory checks every platform runs: every gate above in the gate
# inventory (an expired provisional date is reported here and refused at
# release), and every historical reader in docs/legacy-compatibility.md.
for check in check_gate_inventory.py check_legacy_inventory.py; do
  python3 "$root/tooling/repository/checks/$check" --self-test || fail "$check self-test failed"
  python3 "$root/tooling/repository/checks/$check" || fail "$check failed"
done
printf 'ci-stage contract passed\n'
