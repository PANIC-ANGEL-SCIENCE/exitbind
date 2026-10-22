#!/bin/sh
set -eu

bin=${1:?usage: onboarding-smoke.sh BINARY EXPECTED_SKILL}
expected=${2:?usage: onboarding-smoke.sh BINARY EXPECTED_SKILL}
case "$bin" in
  /*) ;;
  *) bin="$(cd "$(dirname "$bin")" && pwd)/$(basename "$bin")" ;;
esac
test -x "$bin"
test -f "$expected"
root=$(mktemp -d)
cleanup() { find "$root" -depth -delete; }
trap cleanup EXIT HUP INT TERM
mkdir -p "$root/home" "$root/project" "$root/bindings"

name=$(basename "$bin")
test "$name" = exitbind || { echo 'onboarding smoke: expected the current exitbind executable' >&2; exit 1; }
config="$root/project/exitbind.json"
namespace=.exitbind
skill_name=exitbind
binding_env=EXITBIND_BINDINGS_DIR

activation="$root/activation.json"

invoke_path=/nonexistent
ancestor=$(CDPATH= cd "$root/project" && pwd -P)
while :; do
  if test -e "$ancestor/.git" || test -L "$ancestor/.git"; then
    if test "${EXITBIND_TEST_NO_GIT_UNAVAILABLE:-}" = 1; then
      git_bin=$(command -v git)
      git_bin="$(CDPATH= cd "$(dirname "$git_bin")" && pwd -P)/$(basename "$git_bin")"
      test -x "$git_bin"
      mkdir "$root/tools"
      ln -s "$git_bin" "$root/tools/git"
      env -i HOME="$root/home" PATH="$root/tools" "$git_bin" init --quiet "$root/project"
      git_root=$(env -i HOME="$root/home" PATH="$root/tools" "$git_bin" -C "$root/project" rev-parse --show-toplevel)
      test "$git_root" = "$(CDPATH= cd "$root/project" && pwd -P)"
      invoke_path="$root/tools"
      printf '%s\n' 'COVERAGE UNAVAILABLE: onboarding marker-free/no-Git cell: fixture inherits a Git marker; the owned real-Git-only core workflow runs instead' >&2
    fi
    break
  fi
  test "$ancestor" != / || break
  ancestor=$(dirname "$ancestor")
done

invoke() {
  env -i HOME="$root/home" "$binding_env=$root/bindings" \
    PATH="$invoke_path" "$bin" "$@"
}

# init prints its core path as full routes: the invoking executable and the
# created configuration, each quoted as one shell word when it is not plain.
shell_word() {
  case "$1" in
    ''|*[!A-Za-z0-9_./:=@%+,\<\>-]*) printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\\\\''/g")" ;;
    *) printf '%s' "$1" ;;
  esac
}
bin_real="$(CDPATH= cd "$(dirname "$bin")" && pwd -P)/$(basename "$bin")"
config_real="$(CDPATH= cd "$root/project" && pwd -P)/exitbind.json"
printed_route() {
  for exe in "$bin" "$bin_real"; do
    for cfg in "$config" "$config_real"; do
      line="  $(shell_word "$exe") $1 --config $(shell_word "$cfg")"
      printf '%s\n' "$next" | grep -qxF -- "$line" && return 0
    done
  done
  printf '%s\n' "onboarding smoke: init did not print the core path: $1" >&2
  printf '%s\n' "$next" >&2
  exit 1
}

next=$(invoke init --mode portable --root "$root/project")
printed_route "brief worker --task 'Describe the change you want to make'"
printed_route "work begin change --goal 'Describe the bounded change' --check-command <CHECK_COMMAND>"
printed_route check

test -f "$root/project/.agents/skills/$skill_name/SKILL.md"
test -f "$root/project/.claude/skills/$skill_name/SKILL.md"
cmp "$expected" "$root/project/.agents/skills/$skill_name/SKILL.md"
cmp "$expected" "$root/project/.claude/skills/$skill_name/SKILL.md"
cmp "$root/project/.agents/skills/$skill_name/SKILL.md" "$root/project/.claude/skills/$skill_name/SKILL.md"
invoke brief worker --task "First bounded handoff" --config "$config" >/dev/null
invoke work begin change --goal "First bounded handoff" \
  --check-command true --config "$config" >"$activation"
grep -q '"work": "smw_' "$activation"
grep -q '"action":' "$activation"
test "$(find "$root/project/$namespace/runs" -type f -name 'work-*.jsonl' | wc -l | tr -d ' ')" = 1
invoke check --config "$config" >/dev/null

printf '%s\n' "onboarding smoke passed"
