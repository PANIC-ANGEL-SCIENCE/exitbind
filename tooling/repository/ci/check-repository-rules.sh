#!/bin/sh
# Exitbind no-build repository-rule guard: stage 0 of ci-local.sh (seconds).
# Read-only: starts no cargo build, writes nothing in the checkout, prints paths
# and counts, never file values. Use it before a pin or push; ci-local.sh runs
# it before any build-dependent stage.
# Usage: check-repository-rules.sh [BASE]   (run from the checkout root)
#   BASE defaults to origin/main and names the last published commit used for
#   the changed-file headroom warning and the commit-identity range.
# The build-dependent targets remain the authority for these rules:
#   cargo test --locked --features repository-checks --test architecture_budget
#   cargo test --locked --features repository-checks --test privacy_gate
#   cargo test --locked --features repository-checks --test release_integrity
set -u
base=${1:-origin/main}
test -f tests/architecture_budget.rs && test -f scripts/check-path-rendering.sh &&
  test -f tooling/repository/checks/check_architecture_budget.py ||
  { echo 'repository-rules: run from an Exitbind checkout root' >&2; exit 2; }
git rev-parse --verify -q "$base^{commit}" >/dev/null ||
  { echo "repository-rules: base $base is unavailable; pass the last published commit SHA" >&2; exit 2; }

fails=0
step() {
  label=$1
  shift
  out=$("$@" 2>&1)
  rc=$?
  if [ "$rc" -eq 0 ]; then
    printf 'PASS %s\n' "$label"
  else
    fails=$((fails + 1))
    printf 'FAIL %s (exit %s)\n' "$label" "$rc"
    printf '%s\n' "$out" | tail -n 25 | sed 's/^/  | /'
  fi
}

step architecture-budget python3 tooling/repository/checks/check_architecture_budget.py --base "$base"
step path-rendering ./scripts/check-path-rendering.sh
step product-surface ./scripts/check-product-surface.sh
step release-refs ./scripts/check-release-refs.sh
step public-history ./scripts/check-public-history.sh HEAD
step commit-identity python3 tooling/repository/checks/check_commit_identity.py --base "$base"

# Untracked files are scanned by privacy_gate too; the guard states the count so
# an unexpected candidate is visible before the build-dependent gate sees it.
untracked=$(git ls-files --others --exclude-standard | wc -l | tr -d ' ')
test "$untracked" -eq 0 ||
  printf 'WARN %s untracked file(s) in checkout; privacy_gate scans them (git ls-files --others --exclude-standard)\n' "$untracked"

printf 'repository-rules: %s failing gate(s) @ %s base=%s\n' "$fails" "$(git rev-parse --short HEAD)" "$base"
test "$fails" -eq 0
