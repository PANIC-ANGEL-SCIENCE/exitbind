#!/bin/sh
set -eu

fail() {
  printf '%s\n' "exitbind plugin: $*" >&2
  exit 1
}

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/.." && pwd)
package="$repo_root/plugins/exitbind"
output=${1:-"$repo_root/dist/exitbind-0.24.0-rc.3-plugin.tar.gz"}

case "$output" in
  /*) ;;
  *) output="$PWD/$output" ;;
esac
output_dir=$(dirname -- "$output")
mkdir -p "$output_dir"
test ! -e "$output" || fail "archive output already exists: $output"
test ! -L "$output" || fail "archive output already exists: $output"
case "$output" in
  "$package"/*) fail "archive output must be outside the plugin package" ;;
esac

python3 "$script_dir/check-plugin-distribution.py" --repo-root "$repo_root" >/dev/null \
  || fail "plugin distribution check failed"

stage=$(mktemp -d "$output_dir/.exitbind-plugin.XXXXXX")
cleanup() {
  if test -d "$stage"; then
    find "$stage" -depth -delete
  fi
}
trap cleanup EXIT HUP INT TERM

gzip_file="$stage/exitbind.tar.gz"
python3 "$script_dir/package-plugin.py" "$package" "$gzip_file" \
  || fail "could not create deterministic tar archive"
test -s "$gzip_file" || fail "archive is empty"
mv -n "$gzip_file" "$output"
test ! -e "$gzip_file" || fail "archive output appeared during packaging"
printf '%s\n' "packaged plugin=exitbind version=0.24.0-rc.3 archive=$output"
