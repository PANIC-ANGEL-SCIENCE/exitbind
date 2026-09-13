#!/bin/sh
set -eu

fail() {
  printf '%s\n' "exitbind installer smoke: $*" >&2
  exit 1
}

dist=${1:?usage: installer-smoke.sh DIST TARGET [INSTALLER]}
target=${2:?usage: installer-smoke.sh DIST TARGET [INSTALLER]}
script_dir=$(CDPATH= cd "$(dirname "$0")" && pwd)
repo_root=$(CDPATH= cd "$script_dir/.." && pwd)
installer=${3:-$repo_root/install.sh}

case "$target" in
  x86_64-unknown-linux-gnu|aarch64-apple-darwin|x86_64-apple-darwin) ;;
  *) fail "unsupported release target $target" ;;
esac
stem="exitbind-$target"
archive="$stem.tar.gz"
checksum="$archive.sha256"
test -f "$dist/$stem" || fail "release executable is missing: $dist/$stem"
test -f "$dist/$archive" || fail "release archive is missing: $dist/$archive"
test -f "$dist/$checksum" || fail "release checksum is missing: $dist/$checksum"
test -x "$dist/$stem" || fail "release executable is not executable: $dist/$stem"

version=$("$dist/$stem" version) || fail 'packaged executable did not report its version'
test -n "$version" || fail 'packaged executable reported an empty version'
tag="v$version"
root=$(mktemp -d)
cleanup() {
  if test -d "$root"; then
    find "$root" -depth -delete
  fi
}
trap cleanup EXIT HUP INT TERM

mkdir -p "$root/curl" "$root/home" "$root/server" "$root/prefix with spaces"
cp "$dist/$archive" "$root/server/$archive"
cp "$dist/$checksum" "$root/server/$checksum"
calls="$root/curl-calls"
: > "$calls"
cat > "$root/curl/curl" <<'EOF'
#!/bin/sh
set -eu
test "$#" -eq 4 && test "$1" = -fsSL && test "$3" = -o || exit 2
printf '%s\n' "$2" >> "$SOULMATE_SMOKE_CALLS"
case "$2" in
  "$SOULMATE_SMOKE_BASE/$SOULMATE_SMOKE_ARCHIVE") source="$SOULMATE_SMOKE_ARCHIVE_SOURCE" ;;
  "$SOULMATE_SMOKE_BASE/$SOULMATE_SMOKE_CHECKSUM") source="$SOULMATE_SMOKE_CHECKSUM_SOURCE" ;;
  *) printf '%s\n' "unexpected installer URL: $2" >&2; exit 1 ;;
esac
cp "$source" "$4"
EOF
chmod 0755 "$root/curl/curl"

prefix="$root/prefix with spaces"
path="$root/curl:${PATH:-/usr/bin:/bin}"
repo=${EXITBIND_REPOSITORY:-envy404/exitbind}
if env \
  HOME="$root/home" \
  PATH="$path" \
  EXITBIND_INSTALL_PREFIX="$prefix" \
  EXITBIND_REPOSITORY="$repo" \
  EXITBIND_VERSION="$tag" \
  SOULMATE_SMOKE_BASE="https://github.com/$repo/releases/download/$tag" \
  SOULMATE_SMOKE_ARCHIVE="$archive" \
  SOULMATE_SMOKE_CHECKSUM="$checksum" \
  SOULMATE_SMOKE_ARCHIVE_SOURCE="$root/server/$archive" \
  SOULMATE_SMOKE_CHECKSUM_SOURCE="$root/server/$checksum" \
  SOULMATE_SMOKE_CALLS="$calls" \
  sh "$installer" >"$root/install.log" 2>&1
then
  :
else
  status=$?
  cat "$root/install.log" >&2
  exit "$status"
fi

test -x "$prefix/exitbind"
cmp "$dist/$stem" "$prefix/exitbind"
test "$("$prefix/exitbind" version)" = "$version"
test "$(sed -n '1p' "$calls")" = "https://github.com/$repo/releases/download/$tag/$archive"
test "$(sed -n '2p' "$calls")" = "https://github.com/$repo/releases/download/$tag/$checksum"
test "$(wc -l < "$calls" | tr -d ' ')" = 2
benchmark_line=$(grep -n -F "Next: \"$prefix/exitbind\" benchmark" "$root/install.log" | cut -d: -f1)
init_line=$(grep -n -F "Then: cd YOUR_PROJECT && \"$prefix/exitbind\" init --mode portable" "$root/install.log" | cut -d: -f1)
test -n "$benchmark_line"
test -n "$init_line"
test "$benchmark_line" -lt "$init_line"
"$script_dir/onboarding-smoke.sh" "$prefix/exitbind" "$repo_root/skills/exitbind/SKILL.md" >/dev/null
printf '%s\n' "installer smoke passed target=$target version=$version"
