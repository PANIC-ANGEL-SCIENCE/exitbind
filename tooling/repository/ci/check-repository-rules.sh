#!/bin/sh
# Exitbind stage-0 guard: no-build repository rules in seconds.
# Read-only: starts no cargo build, writes nothing in the checkout, prints paths
# and counts, never file values. ci-local.sh runs it before any build-dependent
# stage and passes the comparison base it prepared.
# Usage: check-repository-rules.sh [BASE] [PLATFORM] [MODE]
#   BASE      last published commit (default origin/main): changed-file
#             headroom warning and commit-identity range
#   PLATFORM  linux|macos (default: this host)
#   MODE      pre-return|full (default: pre-return)
# CANDIDATE_SHA, when set (a CI pull-request head), is checked for public
# history in addition to HEAD: a merge can retain published ancestry while the
# contributing head dropped it.
# Each check keeps the platform and mode applicability its former stage had;
# ci_repository_rule_check_selected in ci-stages.sh decides and SKIP says why.
# The build-dependent targets remain the authority for these rules:
#   cargo test --locked --features repository-checks --test architecture_budget
#   cargo test --locked --features repository-checks --test privacy_gate
#   cargo test --locked --features repository-checks --test release_integrity
set -u
base=${1:-origin/main}
platform=${2:-}
case $platform in
  '')
    case "$(uname -s)" in
      Darwin) platform=macos ;;
      *) platform=linux ;;
    esac
    ;;
  linux|macos) ;;
  *)
    printf 'repository-rules: unknown platform %s (linux or macos)\n' "$platform" >&2
    exit 2
    ;;
esac
mode=${3:-pre-return}
case $mode in
  pre-return|full) ;;
  *)
    printf 'repository-rules: unknown mode %s (pre-return or full)\n' "$mode" >&2
    exit 2
    ;;
esac
root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd -P)
. "$root/tooling/repository/ci/ci-stages.sh"
test -f tests/architecture_budget.rs && test -f scripts/check-path-rendering.sh &&
  test -f tooling/repository/checks/check_architecture_budget.py ||
  { echo 'repository-rules: run from an Exitbind checkout root' >&2; exit 2; }
case $base in
  0000000000000000000000000000000000000000)
    echo 'repository-rules: base is all zeros: a first push has no comparison base; pass the last published commit SHA or leave it unset' >&2
    exit 2
    ;;
esac
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
    return 0
  fi
  fails=$((fails + 1))
  printf 'FAIL %s (exit %s)\n' "$label" "$rc"
  # Reason lines first: buffered near-limit WARN lines flush after the FAIL
  # lines, so a plain tail could push the reason out of view.
  printf '%s\n' "$out" | grep -F 'FAIL' | head -n 10 | sed 's/^/  ! /'
  printf '%s\n' "$out" | grep -v -F 'FAIL' | tail -n 20 | sed 's/^/  | /'
}

run_check() {
  case $1 in
    architecture-budget) step architecture-budget python3 tooling/repository/checks/check_architecture_budget.py --base "$base" ;;
    path-rendering) step path-rendering ./scripts/check-path-rendering.sh ;;
    product-surface) step product-surface ./scripts/check-product-surface.sh ;;
    release-refs) step release-refs ./scripts/check-release-refs.sh ;;
    public-history) step public-history ./scripts/check-public-history.sh HEAD ${CANDIDATE_SHA:+"$CANDIDATE_SHA"} ;;
    commit-identity) step commit-identity python3 tooling/repository/checks/check_commit_identity.py --base "$base" ;;
  esac
}

for check in architecture-budget path-rendering product-surface release-refs public-history commit-identity; do
  if ci_repository_rule_check_selected "$check" "$platform" "$mode"; then
    run_check "$check"
  else
    printf 'SKIP %s (not selected for %s %s)\n' "$check" "$platform" "$mode"
  fi
done

# Untracked files are scanned by privacy_gate too; the guard states the count so
# an unexpected candidate is visible before the build-dependent gate sees it.
untracked=$(git ls-files --others --exclude-standard | wc -l | tr -d ' ')
test "$untracked" -eq 0 ||
  printf 'WARN %s untracked file(s) in checkout; privacy_gate scans them (git ls-files --others --exclude-standard)\n' "$untracked"

printf 'repository-rules: %s failing gate(s) @ %s base=%s platform=%s mode=%s\n' \
  "$fails" "$(git rev-parse --short HEAD)" "$base" "$platform" "$mode"
test "$fails" -eq 0
