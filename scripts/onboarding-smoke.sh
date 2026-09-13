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
if test "$name" = exitbind; then
  config="$root/project/exitbind.json"
  namespace=.exitbind
  skill_name=exitbind
  binding_env=EXITBIND_BINDINGS_DIR
else
  config="$root/project/soulmate.json"
  namespace=.soulmate
  skill_name=soulmate
  binding_env=SOULMATE_BINDINGS_DIR
fi
core_words="$skill_name brief"

activation="$root/activation.json"

invoke() {
  env -i HOME="$root/home" "$binding_env=$root/bindings" \
    PATH=/nonexistent "$bin" "$@"
}

next=$(invoke init --mode portable --root "$root/project")
case "$next" in
  *"$core_words"*"$skill_name work begin"*"$skill_name check"*) ;;
  *) echo "onboarding smoke: init did not print the core path" >&2; exit 1 ;;
esac

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
if test "$name" != exitbind; then
  invoke benchmark >/dev/null
fi

printf '%s\n' "onboarding smoke passed"
