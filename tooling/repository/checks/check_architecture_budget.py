#!/usr/bin/env python3
"""No-build scan of the architecture size budget.

The build-dependent authority is
``cargo test --locked --features repository-checks --test architecture_budget``;
this scan applies the same rule from the same two sources
(``tests/architecture_budget.rs`` ROOTS and
``tests/fixtures/architecture-budget.txt`` ceilings) so a breach fails before
any build starts. It reads paths and sizes only, never file values.

``--base`` names the last published commit used for the near-limit warning on
changed files. ``--self-test`` seeds every failure the rule defines.
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Dict, Iterable, List, Set

sys.dont_write_bytecode = True

ROOT = Path(__file__).resolve().parents[3]
TEST = "tests/architecture_budget.rs"
CEILINGS = "tests/fixtures/architecture-budget.txt"
LIMIT = 32 * 1024
WARN_PERCENT = 90
AUTHORITY = "cargo test --locked --features repository-checks --test architecture_budget"


def roots(source: Path) -> List[str]:
    match = re.search(
        r"const ROOTS: &\[&str\] = &\[([^\]]*)\]", source.read_text(encoding="utf-8")
    )
    if not match:
        raise ValueError(f"cannot read ROOTS from {source}")
    return re.findall(r'"([^"]+)"', match.group(1))


def ceilings(source: Path) -> Dict[str, int]:
    found: Dict[str, int] = {}
    for number, line in enumerate(source.read_text(encoding="utf-8").splitlines(), 1):
        if line.startswith("#") or not line.strip():
            continue
        parts = line.split(" ", 1)
        if len(parts) != 2 or not parts[0].isdigit() or not parts[1].strip():
            raise ValueError(f"{source} line {number} is not '<bytes> <path>'")
        found[parts[1].strip()] = int(parts[0])
    return found


def covered(root: Path, covered_roots: Iterable[str]) -> Set[str]:
    found: Set[str] = set()
    for covered_root in covered_roots:
        directory = root / covered_root
        if not directory.is_dir():
            continue
        for current, _, files in os.walk(directory):
            for name in files:
                found.add((Path(current) / name).relative_to(root).as_posix())
    return found


def failures(root: Path, covered_roots: List[str], limits: Dict[str, int]) -> List[str]:
    found = covered(root, covered_roots)
    bad: List[str] = []
    for path in sorted(found):
        size = os.path.getsize(root / path)
        ceiling = limits.get(path)
        if ceiling is not None and size > ceiling:
            bad.append(
                f"{path} grew to {size} bytes over its ceiling {ceiling}; "
                "extract the changed responsibility"
            )
        elif ceiling is not None and size < ceiling and size > LIMIT:
            bad.append(
                f"{path} fell to {size} bytes; lower its ceiling from {ceiling} "
                f"to {size} in {CEILINGS}"
            )
        elif ceiling is None and size > LIMIT:
            bad.append(f"{path} is {size} bytes, over {LIMIT}; divide it by responsibility")
    for path in sorted(limits):
        if path not in found:
            bad.append(f"{path} is listed but no longer exists")
        elif os.path.getsize(root / path) <= LIMIT:
            bad.append(f"{path} is under {LIMIT} bytes; remove its ceiling")
    return bad


def changed(root: Path, base: str, covered_roots: List[str]) -> Set[str]:
    scope = ["--"] + covered_roots
    commands = [
        ["git", "diff", "--name-only", base] + scope,
        ["git", "ls-files", "--others", "--exclude-standard"] + scope,
    ]
    names: Set[str] = set()
    for command in commands:
        result = subprocess.run(
            command,
            cwd=root,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
        )
        names.update(line for line in result.stdout.splitlines() if line)
    return names


def scan(root: Path, base: str | None) -> List[str]:
    found_roots = roots(root / TEST)
    limits = ceilings(root / CEILINGS)
    bad = failures(root, found_roots, limits)
    print(f"INFO budget roots: {', '.join(found_roots)}")
    if base:
        for path in sorted(changed(root, base, found_roots) & covered(root, found_roots)):
            if path in limits:
                continue
            size = os.path.getsize(root / path)
            if size * 100 >= WARN_PERCENT * LIMIT:
                print(
                    f"WARN near-limit changed file: {path} {size} "
                    f"({size * 100 / LIMIT:.1f}%, headroom {LIMIT - size})"
                )
    for line in bad:
        print(f"FAIL: {line}", file=sys.stderr)
    if bad:
        print(f"the build-dependent authority is: {AUTHORITY}", file=sys.stderr)
    return bad


def self_test() -> int:
    problems: List[str] = []

    def fixture(directory: str, name: str, text: str) -> Path:
        root = Path(directory) / name
        (root / "src").mkdir(parents=True)
        (root / "tests/fixtures/nested").mkdir(parents=True)
        (root / CEILINGS).write_text(text, encoding="utf-8")
        (root / "src/clean.rs").write_bytes(b"fn main() {}\n")
        (root / "tests/fixtures/nested/unlisted.bin").write_bytes(b"x" * 200)
        return root

    def expect(root: Path, case: str, *fragments: str) -> None:
        found = failures(root, ["src", "tests", "tests/fixtures"], ceilings(root / CEILINGS))
        for fragment in fragments:
            if not any(fragment in line for line in found):
                problems.append(f"{case}: no failure naming {fragment!r}; found {found}")

    with tempfile.TemporaryDirectory(prefix="architecture-budget-") as directory:
        text = "40000 tests/fixtures/nested/listed.bin\n"
        root = fixture(directory, "clean", text)
        (root / "tests/fixtures/nested/listed.bin").write_bytes(b"x" * 40000)
        if failures(root, ["src", "tests", "tests/fixtures"], ceilings(root / CEILINGS)):
            problems.append(
                "a fixture at its ceiling failed: "
                f"{failures(root, ['src', 'tests', 'tests/fixtures'], ceilings(root / CEILINGS))}"
            )

        (root / "tests/fixtures/nested/listed.bin").write_bytes(b"x" * 41000)
        expect(root, "listed file grew", "grew to 41000 bytes over its ceiling 40000")
        (root / "tests/fixtures/nested/listed.bin").write_bytes(b"x" * 39000)
        expect(root, "listed file fell", "fell to 39000 bytes; lower its ceiling from 40000")
        (root / "tests/fixtures/nested/listed.bin").write_bytes(b"x" * 1000)
        expect(root, "listed file under the limit", f"is under {LIMIT} bytes; remove its ceiling")

        (root / "src/over.rs").write_bytes(b"x" * (LIMIT + 1))
        expect(root, "source over the limit", f"over {LIMIT}; divide it by responsibility")
        os.remove(root / "src/over.rs")

        root = fixture(directory, "missing", "100 tests/fixtures/nested/gone.bin\n")
        expect(root, "listed file missing", "is listed but no longer exists")
    for problem in problems:
        print(f"FAIL: {problem}", file=sys.stderr)
    if not problems:
        print(
            "Architecture budget self-test passed: ceilings enforced; growth, shrink, "
            "removal and a 33 KiB source all refused"
        )
    return 1 if problems else 0


def main(argv: List[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", help="comparison commit for the near-limit warning")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args(argv)
    if args.self_test:
        return self_test()
    found_roots = roots(ROOT / TEST)
    if scan(ROOT, args.base):
        return 1
    print(
        f"Architecture budget passed: {len(covered(ROOT, found_roots))} files "
        f"under {', '.join(found_roots)}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
