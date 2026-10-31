#!/bin/sh
# Show the failed steps and a bounded excerpt of their logs.
set -eu

test "$#" -eq 1 || { echo 'usage: ci-failure.sh RUN_ID' >&2; exit 2; }
case "$1" in ''|*[!0-9]*) echo 'RUN_ID must be numeric' >&2; exit 2 ;; esac
command -v gh >/dev/null 2>&1 || { echo 'gh is required' >&2; exit 1; }
run=$1
repo=PANIC-ANGEL-SCIENCE/exitbind
jobs=$(gh run view "$run" -R "$repo" --json jobs \
  --jq '.jobs[] | select(.conclusion == "failure") | .databaseId')
test -n "$jobs" || { echo "No failed jobs in run $run"; exit 0; }
tmp=$(mktemp -d "${EXITBIND_CI_LOG_TMPDIR:-${TMPDIR:-/tmp}}/exitbind-ci-failure.XXXXXX")
trap 'find "$tmp" -depth -delete' EXIT
printf 'run=%s repository=%s\n' "$run" "$repo"
gh run view "$run" -R "$repo" --json jobs \
  --jq '.jobs[] | select(.conclusion == "failure") | "job=\(.databaseId) name=\(.name) failed_steps=\([.steps[] | select(.conclusion == "failure") | .name] | join(", "))"'
for job in $jobs; do
  # gh can return an empty successful log when its own TMPDIR is a mounted disk.
  TMPDIR=/tmp gh run view "$run" -R "$repo" --job "$job" --log-failed > "$tmp/$job.log"
  test -s "$tmp/$job.log" || { echo "failed-step log is unavailable for job $job" >&2; exit 1; }
  printf 'job=%s relevant_log_lines:\n' "$job"
  awk '
    /./ {
      line=tolower($0)
      if (line ~ /error:|failures:|panicked at|\.\.\. failed|test result: failed|assertion.*failed|err.*value|text file busy|directory not empty/) {
        count++
        if (count <= 200) print substr($0, 1, 500)
      }
    }
    END {
      if (count > 200) printf "... %d additional matching lines omitted\n", count-200
      if (count == 0) print "No matching failure lines; inspect the job summary."
    }
  ' "$tmp/$job.log"
done
