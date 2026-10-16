#!/usr/bin/env python3
"""Check or explicitly regenerate the committed standalone plugin skill projection."""
from __future__ import annotations
import argparse
import shutil
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
SOURCES = [
    Path("skills/exitbind/SKILL.md"),
    Path("skills/exitbind/references/operators.md"),
    Path("skills/exitbind/references/preservation.md"),
]
DEST = Path("plugins/exitbind/skills")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    modes = parser.add_mutually_exclusive_group()
    modes.add_argument("--check", action="store_true")
    modes.add_argument("--write", action="store_true", help="explicitly copy canonical skill bytes into the plugin")
    args = parser.parse_args()
    drift = []
    for relative in SOURCES:
        source = ROOT / relative
        target = ROOT / DEST / relative.relative_to("skills")
        try:
            source_bytes = source.read_bytes()
            target_bytes = target.read_bytes() if target.exists() else None
        except OSError as error:
            print(f"FAIL: {error}", file=sys.stderr)
            return 1
        if source_bytes != target_bytes:
            drift.append((relative, target, source_bytes))
    if args.write:
        for _, target, content in drift:
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(content)
        print(f"Wrote {len(drift)} plugin skill projection file(s) from skills/exitbind")
        return 0
    if drift:
        print("FAIL: committed plugin skill projection differs from canonical skills/exitbind:", file=sys.stderr)
        for source, target, _ in drift:
            print(f"  {source} -> {target.relative_to(ROOT)}", file=sys.stderr)
        return 1
    print("Skill projection passed: plugin bytes match the canonical source")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
