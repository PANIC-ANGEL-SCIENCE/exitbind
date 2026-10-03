#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
log=$(mktemp "${TMPDIR:-/tmp}/workflow-counterexample.XXXXXX")
trap 'rm -f "$log"' 0
if "$root/scripts/check-workflows.sh" "$root/tests/fixtures/workflows/runner-temp-job-env.yml" > "$log" 2>&1; then
  printf 'workflow validator accepted the unsupported job runner context\n' >&2
  exit 1
fi
grep -F 'context "runner" is not allowed here' "$log" >/dev/null || { cat "$log" >&2; exit 1; }
printf 'workflow unsupported-context regression passed\n'
