#!/bin/sh
# Run the timing-sensitive recovery test alone, with the caller's Cargo features.
set -eu
test "$#" -ge 3 || { echo 'usage: isolated-recovery.sh run|list CARGO FEATURES [CARGO_OPTIONS...]' >&2; exit 2; }
mode=$1 cargo=$2 features=$3
shift 3
case "$mode" in run|list) ;; *) echo 'isolated-recovery: expected run or list' >&2; exit 2 ;; esac
root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd -P)
test_name=session_goal::journal_form::tests::recovery_reads_a_complete_journal_past_the_former_16_mib_bound
set -- "$cargo" test --locked "$@"
test -z "$features" || set -- "$@" --features "$features"
set -- "$@" --bin exitbind "$test_name" -- --ignored --exact
printf '+'
printf ' %s' "$@"
printf '\n'
test "$mode" = run || exit 0
log=$(mktemp "${TMPDIR:-/tmp}/isolated-recovery.XXXXXX")
trap 'rm -f "$log"' 0
if "$@" > "$log" 2>&1; then status=0; else status=$?; fi
cat "$log"
test "$status" -eq 0 || exit "$status"
sh "$root/tooling/repository/ci/assert-test-ran.sh" "$test_name" < "$log" || {
  echo 'isolated-recovery: exact test did not run and pass' >&2
  exit 1
}
awk '/^test result: ok[.] 1 passed; 0 failed; 0 ignored; 0 measured; [0-9]+ filtered out; finished in [0-9.]+s$/ { passed = 1 } END { exit !passed }' "$log" || {
  echo 'isolated-recovery: missing successful single-test summary' >&2
  exit 1
}
printf 'isolated-recovery: exact test passed\n'
