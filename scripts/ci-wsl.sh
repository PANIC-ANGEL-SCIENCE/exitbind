#!/bin/sh
set -eu

checkout=${1:?checkout path is required}
candidate="$checkout/wsl-release/exitbind"
test -n "${WSL_DISTRO_NAME:-}"
test "$(uname -s)" = Linux
test -f "$candidate"

export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq git tmux >/dev/null

cd "$checkout"
install_root=$(mktemp -d)
cleanup() {
  find "$install_root" -depth -delete
}
trap cleanup EXIT HUP INT TERM

mkdir -p "$install_root/bin"
install -m 0755 "$candidate" "$install_root/bin/exitbind"
export PATH="$install_root/bin:$PATH"

test "$(exitbind version)" = "0.19.0"
tmux -V >/dev/null
"$checkout/scripts/onboarding-smoke.sh" "$install_root/bin/exitbind" "$checkout/skills/exitbind/SKILL.md" >/dev/null
EXITBIND_BIN="$install_root/bin/exitbind" "$checkout/scripts/run-value-proof-suite.sh" >/dev/null

printf '%s\n' "WSL installed-path smoke passed"
