#!/usr/bin/env python3
"""Validate the Cargo source-package inventory without creating an archive."""
from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
REQUIRED = {
    "Cargo.toml", "Cargo.lock", "src/main.rs", "schema/exitbind.schema.json", "compatibility/rename-matrix.json",
    "skills/exitbind/SKILL.md", "plugins/exitbind/skills/exitbind/SKILL.md", "plugin.json",
    ".agents/plugins/marketplace.json", ".claude-plugin/marketplace.json",
    "docs/index.md", "docs/evidence/participation-comparison.md",
    "tests/groups/project_user_surface.rs", "tests/setup.rs",
    "scripts/demo-checked-work.sh",
}
FORBIDDEN = (
    "tooling/", ".github/", ".claude/", ".codex/", "evidence/", "docs/maintenance/",
    "docs/holytail-", "docs/v0.19-",
)


def validate(paths: set[str]) -> list[str]:
    issues = [f"missing product source: {path}" for path in sorted(REQUIRED - paths)]
    issues.extend(f"forbidden repository/private source: {path}" for path in sorted(paths)
                  if any(path.startswith(prefix) for prefix in FORBIDDEN)
                  or path == "docs/architecture.json"
                  or (path.startswith("scripts/") and path != "scripts/demo-checked-work.sh")
                  or (path.startswith(".agents/") and path != ".agents/plugins/marketplace.json")
                  or (path.startswith(".claude-plugin/") and path != ".claude-plugin/marketplace.json"))
    return issues


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--inventory", type=Path, help="newline-delimited package inventory")
    args = parser.parse_args()
    if args.self_test:
        clean = {"Cargo.toml", "src/main.rs", "schema/exitbind.schema.json", "skills/exitbind/SKILL.md",
                 "tests/groups/project_user_surface.rs", "tests/setup.rs", "scripts/demo-checked-work.sh", "Cargo.lock", "compatibility/rename-matrix.json",
                 "plugins/exitbind/skills/exitbind/SKILL.md", "plugin.json", ".agents/plugins/marketplace.json",
                 ".claude-plugin/marketplace.json", "docs/index.md", "docs/evidence/participation-comparison.md"}
        if validate(clean):
            print("FAIL: positive product inventory was rejected", file=sys.stderr)
            return 1
        bad = clean | {"tooling/repository/private.py"}
        if not any("tooling/repository/private.py" in issue for issue in validate(bad)):
            print("FAIL: seeded forbidden repository file was accepted", file=sys.stderr)
            return 1
        print("Package inventory self-test passed: positive inventory accepted; seeded forbidden file rejected")
        return 0
    if args.inventory:
        try:
            paths = {line.strip() for line in args.inventory.read_text(encoding="utf-8").splitlines() if line.strip()}
        except (OSError, UnicodeError) as error:
            print(f"FAIL: cannot read inventory: {error}", file=sys.stderr)
            return 1
    else:
        result = subprocess.run(["cargo", "package", "--list", "--allow-dirty"], cwd=ROOT,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        if result.returncode:
            sys.stderr.write(result.stderr)
            return result.returncode
        paths = {line.strip() for line in result.stdout.splitlines() if line.strip()}
    issues = validate(paths)
    if issues:
        print("FAIL: Cargo source inventory is not product-only", file=sys.stderr)
        for issue in issues:
            print(f"  {issue}", file=sys.stderr)
        return 1
    print(f"Cargo source inventory passed: {len(paths)} paths; required product inputs present; no forbidden paths")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
