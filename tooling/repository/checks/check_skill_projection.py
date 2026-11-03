#!/usr/bin/env python3
"""Check or explicitly regenerate the committed standalone plugin skill projection."""
from __future__ import annotations
import argparse
import shutil
import stat
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
SOURCES = [
    Path("skills/exitbind/SKILL.md"),
    Path("skills/exitbind/references/operators.md"),
    Path("skills/exitbind/references/preservation.md"),
    Path("skills/exitbind/references/competence.md"),
    Path("skills/exitbind/references/roles/lead.md"),
    Path("skills/exitbind/references/roles/worker.md"),
    Path("skills/exitbind/references/roles/reviewer.md"),
    Path("skills/exitbind/resources/publication/README.md"),
    Path("skills/exitbind/resources/publication/pre-push"),
    Path("skills/exitbind/resources/publication/check-public-text"),
    Path("skills/exitbind/resources/publication/check-tracked-snapshot"),
    Path("skills/exitbind/resources/judgments/assurance.md"),
    Path("skills/exitbind/resources/judgments/context-lifecycle.md"),
    Path("skills/exitbind/resources/judgments/product.md"),
    Path("skills/exitbind/resources/judgments/quantitative.md"),
    Path("skills/exitbind/resources/judgments/workflow.md"),
    Path("skills/exitbind/resources/analysis/README.md"),
    Path("skills/exitbind/resources/analysis/rollout-metrics.py"),
    Path("skills/exitbind/resources/verification/README.md"),
    Path("skills/exitbind/resources/verification/bind-target.py"),
    Path("skills/exitbind/resources/workflows/continuity.md"),
    Path("skills/exitbind/resources/workflows/dispatch.md"),
    Path("skills/exitbind/resources/economy/instructions.md"),
]
EXECUTABLES = {
    Path("skills/exitbind/resources/publication/pre-push"),
    Path("skills/exitbind/resources/publication/check-public-text"),
    Path("skills/exitbind/resources/publication/check-tracked-snapshot"),
    Path("skills/exitbind/resources/analysis/rollout-metrics.py"),
    Path("skills/exitbind/resources/verification/bind-target.py"),
}
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
            mode = stat.S_IMODE(source.stat().st_mode) if relative in EXECUTABLES else None
            if mode is not None and mode & 0o100 == 0:
                print(f"FAIL: source executable lacks owner execute permission: {relative}", file=sys.stderr)
                return 1
            target_mode = stat.S_IMODE(target.stat().st_mode) if mode is not None and target.exists() else None
        except OSError as error:
            print(f"FAIL: {error}", file=sys.stderr)
            return 1
        if source_bytes != target_bytes or (mode is not None and mode != target_mode):
            drift.append((relative, target, source_bytes, mode))
    if args.write:
        for _, target, content, mode in drift:
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(content)
            if mode is not None:
                target.chmod(mode)
        print(f"Wrote {len(drift)} plugin skill projection file(s) from skills/exitbind")
        return 0
    if drift:
        print("FAIL: committed plugin skill projection differs from canonical skills/exitbind:", file=sys.stderr)
        for source, target, _, _ in drift:
            print(f"  {source} -> {target.relative_to(ROOT)}", file=sys.stderr)
        return 1
    print("Skill projection passed: plugin bytes match the canonical source")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
