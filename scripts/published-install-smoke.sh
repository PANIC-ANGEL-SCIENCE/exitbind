#!/bin/sh
set -eu

fail() {
  printf '%s\n' "exitbind published installer smoke: $*" >&2
  exit 1
}

target=${1:?usage: published-install-smoke.sh TARGET EXPECTED_BINARY [INSTALLER]}
expected=${2:?usage: published-install-smoke.sh TARGET EXPECTED_BINARY [INSTALLER]}
script_dir=$(CDPATH= cd "$(dirname "$0")" && pwd)
repo_root=$(CDPATH= cd "$script_dir/.." && pwd)
installer=${3:-$repo_root/install.sh}

case "$target" in
  x86_64-unknown-linux-gnu|aarch64-apple-darwin|x86_64-apple-darwin) ;;
  *) fail "unsupported release target $target" ;;
esac

case "$expected" in
  /*) ;;
  *) expected="$PWD/$expected" ;;
esac
case "$installer" in
  /*) ;;
  *) installer="$PWD/$installer" ;;
esac
test -f "$expected" || fail "expected same-workflow binary is missing: $expected"
test -x "$expected" || fail "expected same-workflow binary is not executable: $expected"
test -f "$installer" || fail "checked-in installer is missing: $installer"

host_os=$(uname -s)
host_arch=$(uname -m)
case "$target:$host_os:$host_arch" in
  x86_64-unknown-linux-gnu:Linux:x86_64) ;;
  aarch64-apple-darwin:Darwin:arm64) ;;
  x86_64-apple-darwin:Darwin:x86_64) ;;
  *) fail "host $host_os/$host_arch does not match target $target" ;;
esac

expected_version=$("$expected" version) || fail 'expected same-workflow binary did not report its version'
test -n "$expected_version" || fail 'expected same-workflow binary reported an empty version'
expected_tag="v$expected_version"
version=${EXITBIND_VERSION:-$expected_tag}
test "$version" = "$expected_tag" || fail "workflow release tag $version does not match expected binary $expected_tag"
repository=${EXITBIND_REPOSITORY:-envy404/exitbind}
test -n "$repository" || fail 'release repository is empty'
command -v curl >/dev/null 2>&1 || fail 'curl is required for real published-asset retrieval'

home=${HOME:?HOME is required}
root=$(mktemp -d)
cleanup() {
  if test -d "$root"; then
    find "$root" -depth -delete
  fi
}
trap cleanup EXIT HUP INT TERM

prefix="$root/prefix with spaces"
global="$home/.local/bin/exitbind"
global_state="$root/global-state"
if test -e "$global" || test -L "$global"; then
  test -f "$global" || fail "existing global install is not a regular file: $global"
  printf '%s\n' present > "$global_state"
  cp "$global" "$root/global-before"
else
  printf '%s\n' absent > "$global_state"
fi

path="$prefix${PATH:+:$PATH}"
if env \
  HOME="$home" \
  PATH="$path" \
  EXITBIND_INSTALL_PREFIX="$prefix" \
  EXITBIND_REPOSITORY="$repository" \
  EXITBIND_VERSION="$version" \
  sh "$installer" >"$root/install.log" 2>&1
then
  :
else
  status=$?
  cat "$root/install.log" >&2
  exit "$status"
fi

installed="$prefix/exitbind"
test -x "$installed" || fail 'published installer did not create an executable'
cmp "$expected" "$installed"
test "$("$installed" version)" = "$expected_version"
test "$(PATH="$path" command -v exitbind)" = "$installed"

case "$(cat "$global_state")" in
  present)
    test -f "$global"
    cmp "$root/global-before" "$global"
    ;;
  absent)
    if test -e "$global" || test -L "$global"; then
      fail "published installer changed the global install path: $global"
    fi
    ;;
  *) fail 'invalid global installation state' ;;
esac

"$script_dir/onboarding-smoke.sh" "$installed" "$repo_root/skills/exitbind/SKILL.md" >/dev/null
printf '%s\n' "published installer smoke passed target=$target version=$expected_version"
