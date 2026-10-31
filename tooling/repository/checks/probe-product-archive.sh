#!/bin/sh
# Check a built Cargo archive for repository-only content, then execute its
# default product tests without access to this checkout's tooling.
set -eu
if test "$#" -ne 3; then
  printf '%s\n' 'usage: probe-product-archive.sh ARCHIVE.crate EXTRACT-DIR TARGET-DIR' >&2
  exit 2
fi
archive=$(CDPATH= cd -- "$(dirname -- "$1")" && pwd)/$(basename -- "$1")
extract=$2
target=$3
test -f "$archive" || { printf '%s\n' 'archive does not exist' >&2; exit 2; }
test ! -e "$extract" || { printf '%s\n' 'extract directory already exists' >&2; exit 2; }
mkdir -p "$extract" "$target"
python3 - "$archive" "$extract" <<'PY'
import sys, tarfile
from pathlib import PurePosixPath
archive = sys.argv[1]
with tarfile.open(archive, "r:gz") as package:
    members = package.getmembers()
    parsed = [(item, PurePosixPath(item.name)) for item in members]
    roots = {path.parts[0] for _, path in parsed if path.parts}
    if len(roots) != 1:
        raise SystemExit("archive must contain one crate root")
    forbidden = ("tooling", ".github", ".claude", ".codex", "evidence",
                 "docs/maintenance", "docs/holytail-", "docs/v0.19-")
    errors = []
    for item, path in parsed:
        name = str(path)
        relative = "/".join(path.parts[1:])
        if path.is_absolute() or ".." in path.parts or not path.parts:
            errors.append(f"archive contains unsafe path: {item.name}")
        if item.issym() or item.islnk():
            errors.append(f"archive contains link: {name}")
        if path.parts[1:2] and path.parts[1] in forbidden[:5]:
            errors.append(f"archive contains forbidden source: {name}")
        if relative == "docs/architecture.json":
            errors.append(f"archive contains forbidden path: {name}")
        if relative.startswith("scripts/") and relative != "scripts/demo-checked-work.sh":
            errors.append(f"archive contains repository script: {name}")
        if relative.startswith(forbidden[5:]):
            errors.append(f"archive contains removed internal record: {name}")
    if errors:
        raise SystemExit("\n".join(errors))
    package.extractall(sys.argv[2], filter="data")
PY
crate_root=$(find "$extract" -mindepth 1 -maxdepth 1 -type d -print -quit)
test -n "$crate_root" || { printf '%s\n' 'archive extraction did not produce crate root' >&2; exit 1; }
for required in Cargo.toml Cargo.lock src/main.rs schema/exitbind.schema.json \
  skills/exitbind/SKILL.md plugins/exitbind/plugin.json \
  plugins/exitbind/skills/exitbind/SKILL.md .agents/plugins/marketplace.json \
  .claude-plugin/marketplace.json docs/index.md \
  docs/evidence/participation-comparison.md tests/groups/project_user_surface.rs \
  scripts/demo-checked-work.sh; do
  test -f "$crate_root/$required" || { printf 'missing product path: %s\n' "$required" >&2; exit 1; }
done
for forbidden in tooling .github .claude .codex evidence docs/maintenance docs/holytail-replacement-checklist.md docs/holytail-workload-findings.md docs/v0.19-evidence-priorities-review.md docs/architecture.json; do
  test ! -e "$crate_root/$forbidden" || { printf 'forbidden extracted path: %s\n' "$forbidden" >&2; exit 1; }
done
for script in "$crate_root"/scripts/*; do
  test ! -e "$script" || test "${script##*/}" = demo-checked-work.sh || { printf 'repository script shipped: %s\n' "$script" >&2; exit 1; }
done
CARGO_TARGET_DIR=$target CARGO_BUILD_JOBS=1 sh "$(dirname -- "$0")/../ci/isolated-recovery.sh" run cargo '' \
  --no-default-features --manifest-path "$crate_root/Cargo.toml"
CARGO_TARGET_DIR=$target CARGO_BUILD_JOBS=1 cargo test --locked --no-default-features --no-fail-fast --manifest-path "$crate_root/Cargo.toml"
printf 'Extracted product archive default tests passed without repository tooling: %s\n' "$crate_root"
