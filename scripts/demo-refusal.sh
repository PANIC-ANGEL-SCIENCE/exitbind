#!/bin/sh
set -eu

bin="${EXITBIND_BIN:-${SOULMATE_BIN:-soulmate}}"
demo_dir="$(mktemp -d)"
trap 'rm -rf "$demo_dir"' EXIT HUP INT TERM
case "$(basename "$bin")" in
  exitbind) config="$demo_dir/exitbind.json"; ledger=.exitbind/runs/demo.jsonl ;;
  soulmate) config="$demo_dir/soulmate.json"; ledger=.soulmate/runs/demo.jsonl ;;
  *) printf 'Demo requires an exitbind or soulmate binary: %s\n' "$bin" >&2; exit 1 ;;
esac

"$bin" init --mode portable --root "$demo_dir" >/dev/null
"$bin" run start change --goal "Change one line" --ledger "$ledger" --config "$config" >/dev/null
printf 'before\n' >"$demo_dir/result.txt"
"$bin" run submit lead "$ledger" --outcome scoped --artifact result.txt \
  --artifact-root product --config "$config" >/dev/null
printf 'after\n' >"$demo_dir/result.txt"

if output=$("$bin" run next "$ledger" --config "$config" 2>&1); then
  echo "demo failed: changed artifact was accepted" >&2
  exit 1
fi

case "$output" in
  *"artifact drift detected: result.txt"*) printf '%s\n' "$output" ;;
  *) printf '%s\n' "$output" >&2; exit 1 ;;
esac
