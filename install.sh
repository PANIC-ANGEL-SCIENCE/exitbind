#!/bin/sh
set -eu
surface=exitbind
legacy_bridge=0
if test -z "${EXITBIND_REPOSITORY:-}" &&
   test -z "${EXITBIND_VERSION:-}" &&
   test -z "${EXITBIND_INSTALL_PREFIX:-}" &&
   { test -n "${SOULMATE_REPOSITORY:-}" ||
     test -n "${SOULMATE_VERSION:-}" ||
     test -n "${SOULMATE_INSTALL_PREFIX:-}"; }; then
  surface=soulmate
  if test "${SOULMATE_REPOSITORY:-envy404/soulmate}" = "envy404/soulmate"; then
    legacy_bridge=1
  fi
fi
repo="${EXITBIND_REPOSITORY:-envy404/exitbind}"
if test -z "${EXITBIND_REPOSITORY:-}" && test -n "${SOULMATE_REPOSITORY:-}"; then
  repo="$SOULMATE_REPOSITORY"
fi
if test "$legacy_bridge" = 1; then
  repo=envy404/exitbind
fi
version="${EXITBIND_VERSION:-v0.19.1}"
if test -z "${EXITBIND_VERSION:-}" && test -n "${SOULMATE_VERSION:-}"; then
  version="$SOULMATE_VERSION"
fi
os=$(uname -s | tr '[:upper:]' '[:lower:]')
arch=$(uname -m)
case "$os:$arch" in
  linux:x86_64|linux:amd64) target="x86_64-unknown-linux-gnu" ;;
  darwin:arm64) target="aarch64-apple-darwin" ;;
  darwin:x86_64) target="x86_64-apple-darwin" ;;
  *) echo "exitbind: unsupported platform $os/$arch" >&2; exit 1 ;;
esac
if command -v sha256sum >/dev/null 2>&1; then
  checksum=sha256sum
elif command -v shasum >/dev/null 2>&1; then
  checksum=shasum
else
  echo "exitbind: no SHA-256 checksum utility found" >&2
  exit 1
fi
asset_surface=$surface
if test "$legacy_bridge" = 1; then
  asset_surface=exitbind
  if printf '%s\n' "$version" | grep -Eq \
    '^v0\.(0|1|2|3|4|5|6|7|8|9|10|11|12|13|14|15|16)\.(0|[1-9][0-9]*)(-[0-9A-Za-z]+([.-][0-9A-Za-z]+)*)?([+][0-9A-Za-z]+([.-][0-9A-Za-z]+)*)?$'; then
    asset_surface=soulmate
    legacy_bridge=0
  fi
fi
base="https://github.com/$repo/releases/download/$version"
tmp=$(mktemp -d)
cleanup() { find "$tmp" -depth -delete; }
trap cleanup EXIT HUP INT TERM
archive="${asset_surface}-${target}.tar.gz"
curl -fsSL "$base/$archive" -o "$tmp/$archive"
curl -fsSL "$base/$archive.sha256" -o "$tmp/$archive.sha256"
case "$checksum" in
  sha256sum) (cd "$tmp" && sha256sum -c "$archive.sha256") ;;
  shasum) (cd "$tmp" && shasum -a 256 -c "$archive.sha256") ;;
esac
prefix="${EXITBIND_INSTALL_PREFIX:-${HOME:?HOME is required}/.local/bin}"
if test -z "${EXITBIND_INSTALL_PREFIX:-}" && test -n "${SOULMATE_INSTALL_PREFIX:-}"; then
  prefix="$SOULMATE_INSTALL_PREFIX"
fi
case "$prefix" in ""|/) echo "exitbind: unsafe install prefix" >&2; exit 1 ;; esac
mkdir -p "$prefix"
tar -xzf "$tmp/$archive" -C "$tmp"
# Exitbind 0.18.0 and 0.19.0 updaters re-read their own executable name after
# this script returns; on Linux a binary replaced in place reads as
# "NAME (deleted)" and those updaters then truncate the new install. Move a
# replaced regular binary aside under a directory that keeps its file name, so
# a running updater still sees its own name. Earlier aside copies are removed.
for previous in "$prefix"/.exitbind-previous-* "$prefix"/.soulmate-previous-*; do
  if test -d "$previous" && ! test -L "$previous"; then
    rm -rf "$previous"
  fi
done
replace_binary() {
  staged=$1
  destination=$2
  name=${destination##*/}
  aside=
  if test -f "$destination" && ! test -L "$destination"; then
    aside="$prefix/.$name-previous-$$"
    mkdir -m 0700 "$aside"
    mv -f "$destination" "$aside/$name"
  fi
  if ! mv -f "$staged" "$destination"; then
    if test -n "$aside"; then
      mv -f "$aside/$name" "$destination"
      rmdir "$aside"
    fi
    return 1
  fi
}
stage="$prefix/.$surface-install-$$"
install -m 0755 "$tmp/$asset_surface-${target}" "$stage"
replace_binary "$stage" "$prefix/$surface"
if test "$legacy_bridge" = 1; then
  compatibility_stage="$prefix/.soulmate-compat-install-$$"
  install -m 0755 "$tmp/exitbind-${target}" "$compatibility_stage"
  replace_binary "$compatibility_stage" "$prefix/exitbind"
fi
echo "Installed $surface $version to $prefix/$surface"
case ":${PATH:-}:" in
  *":$prefix:"*) ;;
  *) echo "Add $prefix to PATH to invoke $surface by name." ;;
esac
echo "Next: \"$prefix/$surface\" benchmark"
echo "Then: cd YOUR_PROJECT && \"$prefix/$surface\" init --mode portable"
