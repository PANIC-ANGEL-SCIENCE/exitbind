#!/bin/sh
set -eu

fail() {
  printf '%s\n' "exitbind installer smoke: $*" >&2
  exit 1
}

dist=${1:?usage: installer-smoke.sh DIST TARGET [INSTALLER]}
target=${2:?usage: installer-smoke.sh DIST TARGET [INSTALLER]}
script_dir=$(CDPATH= cd "$(dirname "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/../../.." && pwd -P)
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
repo=${EXITBIND_REPOSITORY:-PANIC-ANGEL-SCIENCE/exitbind}
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
test ! -e "$prefix/soulmate"
home_dir="$root/home"
test ! -e "$home_dir/.agents/skills/soulmate"
test ! -e "$home_dir/.claude/skills/soulmate"
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

# Exercise the real host tar with archive ownership that differs from the
# caller. The synthetic archive retains the package's exact executable bytes.
python3 - "$root/server/$archive" "$root/server/$checksum" "$dist/$stem" "$stem" <<'PY'
import hashlib
import os
import pathlib
import sys
import tarfile

archive, checksum, binary, member = map(pathlib.Path, sys.argv[1:])
owner = os.getuid() + 1 if os.getuid() else 1001
group = os.getgid() + 1 if os.getgid() else 1001
data = binary.read_bytes()
info = tarfile.TarInfo(member.as_posix())
info.size = len(data)
info.mode = 0o755
info.uid = owner
info.gid = group
info.uname = "release-builder"
info.gname = "release-builder"
with tarfile.open(archive, "w:gz") as output:
    output.addfile(info, __import__("io").BytesIO(data))
digest = hashlib.sha256(archive.read_bytes()).hexdigest()
checksum.write_text(f"{digest}  {archive.name}\n", encoding="ascii")
PY

tar_options=
if tar --version 2>/dev/null | grep -q 'GNU tar'; then
  tar_options=--same-owner
fi
env \
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
  TAR_OPTIONS="$tar_options" \
  sh "$installer" >"$root/owner-install.log" 2>&1 || {
    cat "$root/owner-install.log" >&2
    fail 'actual archive extraction failed with non-caller ownership metadata'
  }
cmp "$dist/$stem" "$prefix/exitbind"
python3 - "$prefix/exitbind" <<'PY'
import os
import pathlib
import sys

assert pathlib.Path(sys.argv[1]).stat().st_uid == os.getuid(), "installer retained archive UID"
PY

# Injected tar failure is intentionally a mocked failure case. It checks that
# the installer does not move an existing binary before extraction succeeds.
mkdir "$root/failing-tar"
cat > "$root/failing-tar/tar" <<'EOF'
#!/bin/sh
exit 73
EOF
chmod 0755 "$root/failing-tar/tar"
if env \
  HOME="$root/home" \
  PATH="$root/failing-tar:$path" \
  EXITBIND_INSTALL_PREFIX="$prefix" \
  EXITBIND_REPOSITORY="$repo" \
  EXITBIND_VERSION="$tag" \
  SOULMATE_SMOKE_BASE="https://github.com/$repo/releases/download/$tag" \
  SOULMATE_SMOKE_ARCHIVE="$archive" \
  SOULMATE_SMOKE_CHECKSUM="$checksum" \
  SOULMATE_SMOKE_ARCHIVE_SOURCE="$root/server/$archive" \
  SOULMATE_SMOKE_CHECKSUM_SOURCE="$root/server/$checksum" \
  SOULMATE_SMOKE_CALLS="$calls" \
  sh "$installer" >"$root/failing-tar.log" 2>&1
then
  fail 'mocked tar failure unexpectedly installed the package'
else
  status=$?
  test "$status" -eq 73 || fail "mocked tar failure returned unexpected status $status"
fi
cmp "$dist/$stem" "$prefix/exitbind"
printf '%s\n' "installer smoke passed target=$target version=$version (actual owner extraction; mocked tar failure preserved install)"
