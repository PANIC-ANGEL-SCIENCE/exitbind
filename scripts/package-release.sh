#!/bin/sh
set -eu

fail() {
  printf '%s\n' "exitbind: $*" >&2
  exit 1
}

target=${1:?usage: package-release.sh TARGET BINARY [DIST]}
binary=${2:?usage: package-release.sh TARGET BINARY [DIST]}
dist=${3:-dist}

case "$target" in
  x86_64-unknown-linux-gnu|aarch64-apple-darwin|x86_64-apple-darwin) ;;
  *) fail "unsupported release target $target" ;;
esac
test -f "$binary" || fail "release binary is missing: $binary"
test -x "$binary" || fail "release binary is not executable: $binary"

stem="exitbind-$target"
archive="$stem.tar.gz"
checksum="$archive.sha256"
mkdir -p "$dist"
for output in "$dist/$stem" "$dist/$archive" "$dist/$checksum"; do
  test ! -e "$output" || fail "release output already exists: $output"
done

if command -v sha256sum >/dev/null 2>&1; then
  checksum_tool=sha256sum
elif command -v shasum >/dev/null 2>&1; then
  checksum_tool=shasum
else
  fail 'no SHA-256 checksum utility found'
fi

version=$("$binary" version) || fail 'release binary did not report its version'
test -n "$version" || fail 'release binary reported an empty version'
stage="$dist/.exitbind-package-$$"
test ! -e "$stage" || fail "temporary package path already exists: $stage"
cleanup() {
  if test -d "$stage"; then
    find "$stage" -depth -delete
  fi
}
trap cleanup EXIT HUP INT TERM

mkdir "$stage"
install -m 0755 "$binary" "$stage/$stem"
test -x "$stage/$stem"
tar -czf "$stage/$archive" -C "$stage" "$stem"
case "$checksum_tool" in
  sha256sum) (cd "$stage" && sha256sum "$archive" > "$checksum") ;;
  shasum) (cd "$stage" && shasum -a 256 "$archive" > "$checksum") ;;
esac
test -s "$stage/$archive"
test -s "$stage/$checksum"

mv "$stage/$stem" "$dist/$stem"
mv "$stage/$archive" "$dist/$archive"
mv "$stage/$checksum" "$dist/$checksum"
digest=$(awk '{print $1}' "$dist/$checksum")
test -n "$digest"
printf '%s\n' "packaged target=$target version=$version archive=$archive sha256=$digest"
