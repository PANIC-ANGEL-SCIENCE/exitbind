#!/bin/sh
# Scripted local workflow, not an agent evaluation or a user study.
set -eu
umask 077

binary=${EXITBIND_BIN:-${SOULMATE_BIN:-soulmate}}
case "$binary" in
  /*) ;;
  */*) binary="$(pwd)/$binary" ;;
  *) binary=$(command -v "$binary") ;;
esac
project=$(mktemp -d "${TMPDIR:-/tmp}/soulmate checked.XXXXXX")
trap 'rm -rf "$project"' EXIT HUP INT TERM
cd "$project"

case "$(basename "$binary")" in
  exitbind) config=exitbind.json; state=.exitbind ;;
  soulmate) config=soulmate.json; state=.soulmate ;;
  *) printf 'Demo requires an exitbind or soulmate binary: %s\n' "$binary" >&2; exit 1 ;;
esac

ledger="$state/runs/change.jsonl"
check_command='test "$(cat message.txt)" = ready'

sm() { "$binary" "$@" --config "$config"; }
artifact() { printf '%s\n' "$2" > "$state/artifacts/$1.md"; }
submit() {
  sm run submit "$1" "$ledger" --outcome "$2" \
    --artifact "$state/artifacts/$3.md" --artifact-root state
}

"$binary" init --mode portable >/dev/null
sm brief worker --task 'Make message.txt contain ready.' >/dev/null
sm run start change --goal 'Make message.txt contain ready.' \
  --check-command "$check_command" --ledger "$ledger" >/dev/null
sm check >/dev/null

artifact scope 'Accept only when the configured message check passes.'
submit lead scoped scope >/dev/null
printf 'unfinished\n' > message.txt
artifact worker-1 'Scripted worker claims completion; message is still unfinished.'
# Capture the identity at submission, before the host runs the check.
target=$(sm run submit worker "$ledger" --outcome completed \
  --artifact "$state/artifacts/worker-1.md" --artifact-root state --event-id)
check_exit=0
sh -c "$check_command" || check_exit=$?
test "$check_exit" -ne 0
sm run record-check "$ledger" --target "$target" \
  --check-command "$check_command" --exit-code "$check_exit" >/dev/null
artifact review-1 'Scripted reviewer approves, despite the failed check report.'
submit reviewer approved review-1 >/dev/null
artifact acceptance-1 'Scripted lead attempts acceptance.'
if refused=$(submit lead accepted acceptance-1 2>&1); then
  printf 'Demo failed: a failed check was accepted.\n' >&2
  exit 1
fi
case "$refused" in
  *'acceptance refused: configured check evidence is check_failed'*) ;;
  *) printf '%s\n' "$refused" >&2; exit 1 ;;
esac
printf 'Attempt 1: host check failed; acceptance refused.\n'

artifact rework 'Repair message.txt in a fresh attempt; keep earlier reports.'
submit lead rework rework >/dev/null
# Another CLI process reconstructs the assignment using only the local records.
resumed=$(sm run next "$ledger" --text)
case "$resumed" in
  *worker-1.md*) ;;
  *) printf 'Demo failed: prior worker artifact is missing from resume.\n' >&2; exit 1 ;;
esac
printf '%s\n' "$resumed"
test "$(cat "$state/artifacts/worker-1.md")" = \
  'Scripted worker claims completion; message is still unfinished.'

printf 'ready\n' > message.txt
artifact worker-2 'Scripted worker repaired message.txt; check it again.'
target=$(sm run submit worker "$ledger" --outcome completed \
  --artifact "$state/artifacts/worker-2.md" --artifact-root state --event-id)
check_exit=0
sh -c "$check_command" || check_exit=$?
test "$check_exit" -eq 0
sm run record-check "$ledger" --target "$target" \
  --check-command "$check_command" --exit-code "$check_exit" >/dev/null
artifact review-2 'Scripted reviewer approves the repaired attempt.'
submit reviewer approved review-2 >/dev/null
artifact acceptance-2 'Scripted lead accepts the reviewed, checked attempt.'
submit lead accepted acceptance-2 >/dev/null
status=$(sm run status "$ledger" --json)
case "$status" in
  *'"status": "accepted"'*) ;;
  *) printf 'Demo failed: final acceptance missing.\n' >&2; exit 1 ;;
esac
sm run inspect "$ledger" >/dev/null
printf 'Attempt 2: host check passed; separately reviewed and accepted.\n'
printf 'Earlier worker artifact retained; new process reconstructed the assignment.\n'
printf 'Scripted fixture complete. No model calls or user-success measurements.\n'
