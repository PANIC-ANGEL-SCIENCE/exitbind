#!/bin/sh
# Run the Cargo test targets a change affects, each with its declared features,
# and the binary's unit tests when src/ or the manifest changed.
# Usage: test-affected.sh --base BASE [--list] [--skip-passed 'TARGET=FEATURES ...']
# Changed files are the diff against BASE plus untracked files; selection and
# features come from tooling/repository/checks/test-target-map.json. The CI
# caller may omit exact pairs it has already run successfully in this run.
set -eu
fail() { printf 'test-affected: %s\n' "$*" >&2; exit 1; }
usage="usage: test-affected.sh --base BASE [--list] [--skip-passed 'TARGET=FEATURES ...']"
base=
list_only=false
passed_targets=
while [ "$#" -gt 0 ]; do
  case "$1" in
    --base) test "$#" -ge 2 && test -n "$2" || fail "$usage"; base=$2; shift 2 ;;
    --list) list_only=true; shift ;;
    --skip-passed) test "$#" -ge 2 || fail "$usage"; passed_targets=$2; shift 2 ;;
    *) fail "$usage" ;;
  esac
done
test -n "$base" || fail "$usage"
root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd -P)
cd "$root"
git rev-parse --verify -q "$base^{commit}" >/dev/null || fail "base is not a commit: $base"
. "$root/tooling/repository/ci/verification-context.sh"
cargo=${CARGO:-cargo}

selection=$(python3 -B tooling/repository/checks/test_target_sources.py affected --base "$base") ||
  fail 'target selection failed; fix the test-target map first'
tab=$(printf '\t')
if [ -z "$selection" ]; then
  printf 'test-affected: base=%s selects no Cargo test target\n' "$base"
  exit 0
fi
count=$(printf '%s\n' "$selection" | wc -l | tr -d ' ')
printf 'test-affected: base=%s selects %s target(s)\n' "$base" "$count"

# Build `env -u VAR ...` once so the printed command is the executed command.
unset_args=
for variable in $host_session_variables; do
  unset_args="$unset_args -u $variable"
done
if [ "$list_only" = true ]; then
  log_dir=
else
  # Per-target logs live only as long as a target fails; a passing run leaves
  # nothing in TMPDIR, and a failing one names the logs it keeps.
  log_dir=$(mktemp -d "${TMPDIR:-/tmp}/test-affected.XXXXXX")
fi
failed=
executed=0
omitted=0
while IFS="$tab" read -r target features; do
  case " $passed_targets " in
    *" $target=$features "*)
      printf '  %s: omitted (caller reports an earlier pass with matching features)\n' "$target"
      omitted=$((omitted + 1))
      continue
      ;;
  esac
  executed=$((executed + 1))
  if [ "${target#bin:}" != "$target" ]; then
    set -- test --locked --bin "${target#bin:}"
  elif [ -n "$features" ]; then
    set -- test --locked --features "$features" --test "$target"
  else
    set -- test --locked --test "$target"
  fi
  # shellcheck disable=SC2086
  printf '+ env%s %s %s\n' "$unset_args" "$cargo" "$*"
  test "$list_only" = true && continue
  log="$log_dir/$(printf '%s' "$target" | tr ':' '-').log"
  # shellcheck disable=SC2086
  if env $unset_args "$cargo" "$@" > "$log" 2>&1; then status=0; else status=$?; fi
  passed=$(sed -n 's/^test result: [A-Za-z]*\. \([0-9][0-9]*\) passed;.*/\1/p' "$log" | awk '{n += $1} END {print n + 0}')
  failed_count=$(sed -n 's/^test result: [A-Za-z]*\. [0-9]* passed; \([0-9][0-9]*\) failed;.*/\1/p' "$log" | awk '{n += $1} END {print n + 0}')
  if [ "$status" -ne 0 ] || [ "$passed" -eq 0 ]; then
    printf '  %s: exit=%s passed=%s failed=%s log=%s\n' "$target" "$status" "$passed" "$failed_count" "$log"
  else
    printf '  %s: exit=%s passed=%s failed=%s\n' "$target" "$status" "$passed" "$failed_count"
    rm -f "$log"
  fi
  if [ "$status" -ne 0 ]; then
    sed -n '/^failures:$/,/^test result:/p' "$log" | tail -n 60 | sed 's/^/  | /'
    test "$failed_count" -gt 0 || tail -n 25 "$log" | sed 's/^/  | /'
    failed="$failed $target"
  elif [ "$passed" -eq 0 ]; then
    printf '  | %s ran no test; a target must execute at least one test\n' "$target"
    failed="$failed $target"
  fi
done <<SELECTION
$selection
SELECTION
test "$list_only" = true && exit 0
test -z "$failed" || fail "failing target(s):$failed; logs kept in $log_dir"
rmdir "$log_dir"
printf 'test-affected: %s target(s) selected; %s executed; %s omitted after caller-reported passes\n' "$count" "$executed" "$omitted"
