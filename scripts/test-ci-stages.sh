#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
. "$root/scripts/ci-stages.sh"

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
release-refs"
pre_return_macos="$pre_return_common
architecture-budget
privacy-gate
release-integrity
docs-workflow
first-impression
value-contract
plugin-distribution"
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
for stage in tests value-proof drift-warning native-handoff; do
  if ci_stage_selected pre-return "$stage" linux; then
    fail "full-only stage selected for pre-return: $stage"
  fi
done
for stage in architecture-budget privacy-gate release-integrity docs-workflow first-impression value-contract plugin-distribution; do
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
  grep -F "    $stage)" "$root/scripts/ci-local.sh" >/dev/null ||
    fail "selected stage has no command dispatch: $stage"
done

printf 'ci-stage contract passed\n'
