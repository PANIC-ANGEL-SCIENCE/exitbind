#!/bin/sh
# Pinned, checksum-verified task-local validator. Network acquisition is opt-in.
set -eu
fail() { printf 'workflow validation: %s\n' "$*" >&2; exit 1; }
version=1.7.12
case "$(uname -s):$(uname -m)" in
  Linux:x86_64) asset=linux_amd64; checksum=8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8 ;;
  Darwin:arm64) asset=darwin_arm64; checksum=aba9ced2dee8d27fecca3dc7feb1a7f9a52caefa1eb46f3271ea66b6e0e6953f ;;
  Darwin:x86_64) asset=darwin_amd64; checksum=5b44c3bc2255115c9b69e30efc0fecdf498fdb63c5d58e17084fd5f16324c644 ;;
  *) fail 'unsupported native validator host' ;;
esac
cache=${EXITBIND_WORKFLOW_CACHE:-${TMPDIR:-/tmp}/exitbind-actionlint-$version}
archive="$cache/actionlint_${version}_${asset}.tar.gz"
if [ ! -f "$archive" ]; then
  test "${EXITBIND_FETCH_VALIDATOR:-0}" = 1 ||
    fail 'verified archive unavailable offline; provision EXITBIND_WORKFLOW_CACHE or opt in with EXITBIND_FETCH_VALIDATOR=1'
  (umask 077; mkdir -p "$cache")
  download=$(mktemp "$cache/download.XXXXXX")
  trap 'rm -f "$download"' 0 HUP INT TERM
  curl -fLsS --proto '=https' --tlsv1.2 "https://github.com/rhysd/actionlint/releases/download/v$version/actionlint_${version}_${asset}.tar.gz" -o "$download"
  archive=$download
fi
if command -v sha256sum >/dev/null 2>&1; then
  actual=$(sha256sum "$archive" | awk '{print $1}')
else
  actual=$(shasum -a 256 "$archive" | awk '{print $1}')
fi
test "$actual" = "$checksum" || fail 'validator archive checksum mismatch'
if [ "${download:-}" ]; then
  mv "$download" "$cache/actionlint_${version}_${asset}.tar.gz"
  archive="$cache/actionlint_${version}_${asset}.tar.gz"
  trap - 0 HUP INT TERM
fi
runner=$(umask 077; mktemp -d "$cache/run.XXXXXX")
trap 'rm -rf "$runner"' 0
trap 'exit 130' INT
trap 'exit 143' TERM
trap 'exit 129' HUP
tar -xzf "$archive" -C "$runner" actionlint
# Auxiliary shell/Python analyzers are independent gates and are not ambient inputs.
"$runner/actionlint" -shellcheck= -pyflakes= "$@"
