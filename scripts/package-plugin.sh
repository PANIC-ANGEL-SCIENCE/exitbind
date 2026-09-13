#!/bin/sh
set -eu

fail() {
  printf '%s\n' "exitbind plugin: $*" >&2
  exit 1
}

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/.." && pwd)
package="$repo_root/plugins/exitbind"
output=${1:-"$repo_root/dist/exitbind-0.17.0-plugin.tar.gz"}

case "$output" in
  /*) ;;
  *) output="$PWD/$output" ;;
esac
output_dir=$(dirname -- "$output")
mkdir -p "$output_dir"
test ! -e "$output" || fail "archive output already exists: $output"
case "$output" in
  "$package"/*) fail "archive output must be outside the plugin package" ;;
esac

python3 "$script_dir/check-plugin-distribution.py" --repo-root "$repo_root" >/dev/null \
  || fail "plugin distribution check failed"
command -v tar >/dev/null 2>&1 || fail "tar is required"
command -v gzip >/dev/null 2>&1 || fail "gzip is required"

stage=$(mktemp -d "${TMPDIR:-/tmp}/exitbind-plugin.XXXXXX")
cleanup() {
  if test -d "$stage"; then
    find "$stage" -depth -delete
  fi
}
trap cleanup EXIT HUP INT TERM

tar_file="$stage/exitbind.tar"
gzip_file="$stage/exitbind.tar.gz"
tar --sort=name --mtime='UTC 1970-01-01 00:00:00' --owner=0 --group=0 --numeric-owner \
  -cf "$tar_file" -C "$repo_root/plugins" exitbind \
  || fail "could not create deterministic tar archive"
gzip -n -9 < "$tar_file" > "$gzip_file" || fail "could not gzip archive"
test -s "$gzip_file" || fail "archive is empty"
mv "$gzip_file" "$output"
printf '%s\n' "packaged plugin=exitbind version=0.17.0 archive=$output"
