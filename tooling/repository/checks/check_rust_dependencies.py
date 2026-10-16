#!/usr/bin/env python3
"""Enforce the selected Rust source dependency boundaries."""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
import tempfile
from pathlib import Path


PRESENTATION_MODULE = r"(?:presentation|presentation_events|run_presentation)"
PRESENTATION_REFERENCE = re.compile(
    rf"\bcrate\s*::\s*{PRESENTATION_MODULE}\b"
    rf"|\buse\s+crate\s*::\s*\{{[^}}]*\b{PRESENTATION_MODULE}\b",
    re.DOTALL,
)
REPOSITORY_REFERENCE = re.compile(r"(?:tooling\s*::\s*repository|tooling[/\\:]+repository)", re.IGNORECASE)


def rust_parts(text: str) -> tuple[str, list[str]]:
    """Return Rust code with comments/literals masked and decoded literals."""
    code: list[str] = []
    literals: list[str] = []
    index = 0
    while index < len(text):
        if text.startswith("//", index):
            end = text.find("\n", index)
            if end < 0:
                code.append(" " * (len(text) - index))
                break
            code.append(" " * (end - index) + "\n")
            index = end + 1
            continue
        if text.startswith("/*", index):
            start = index
            depth = 1
            index += 2
            while index < len(text) and depth:
                if text.startswith("/*", index):
                    depth += 1
                    index += 2
                elif text.startswith("*/", index):
                    depth -= 1
                    index += 2
                else:
                    index += 1
            code.append(" " * (index - start))
            continue

        raw_start = re.match(r"(?:b)?r(#+)?\"", text[index:])
        if raw_start:
            opening = raw_start.group(0)
            hashes = raw_start.group(1) or ""
            content_start = index + len(opening)
            terminator = '"' + hashes
            end = text.find(terminator, content_start)
            if end < 0:
                end = len(text)
                literal_end = end
            else:
                literal_end = end + len(terminator)
            literals.append(text[content_start:end])
            code.append(" " * (literal_end - index))
            index = literal_end
            continue

        string_prefix = 1 if text[index] == '"' else (
            2 if text.startswith('b"', index) else 0
        )
        if string_prefix:
            start = index
            quote = index + string_prefix - 1
            cursor = quote + 1
            value: list[str] = []
            while cursor < len(text):
                character = text[cursor]
                if character == "\\" and cursor + 1 < len(text):
                    escapes = {"n": "\n", "r": "\r", "t": "\t", "0": "\0"}
                    value.append(escapes.get(text[cursor + 1], text[cursor + 1]))
                    cursor += 2
                elif character == '"':
                    cursor += 1
                    break
                else:
                    value.append(character)
                    cursor += 1
            literals.append("".join(value))
            code.append(" " * (cursor - start))
            index = cursor
            continue

        # Character literals cannot contain Rust module paths. Mask a closed
        # literal while leaving lifetime apostrophes in code.
        if text[index] == "'":
            cursor = index + 1
            if cursor < len(text) and text[cursor] == "\\":
                cursor += 2
            else:
                cursor += 1
            if cursor < len(text) and text[cursor] == "'":
                code.append(" " * (cursor + 1 - index))
                index = cursor + 1
                continue

        code.append(text[index])
        index += 1
    return "".join(code), literals


def owner(path: Path) -> str | None:
    parts = path.as_posix().split("/")
    if len(parts) >= 3 and parts[:2] == ["src", "run"]:
        return "execution"
    if len(parts) >= 3 and parts[:2] == ["src", "presentation"]:
        return "presentation"
    return None


def violations(root: Path) -> list[str]:
    source = root / "src"
    if not source.is_dir():
        return ["src directory is missing"]

    found: list[str] = []
    for path in sorted(source.rglob("*.rs")):
        relative = path.relative_to(root)
        try:
            text = path.read_text(encoding="utf-8")
        except (OSError, UnicodeError) as error:
            found.append(f"{relative}: cannot read Rust source: {error}")
            continue

        code, literals = rust_parts(text)
        source_owner = owner(relative)
        if source_owner == "execution" and PRESENTATION_REFERENCE.search(code):
            found.append(f"{relative}: execution source depends on presentation")
        if REPOSITORY_REFERENCE.search(code) or any(REPOSITORY_REFERENCE.search(value) for value in literals):
            found.append(f"{relative}: product source depends on repository administration")

    return found


def check(root: Path) -> int:
    errors = violations(root)
    if errors:
        for error in errors:
            print(f"FAIL: {error}", file=sys.stderr)
        return 1
    print("Rust source dependency boundaries passed")
    return 0


def self_test(script: Path) -> int:
    with tempfile.TemporaryDirectory(prefix="exitbind-import-check-") as temporary:
        root = Path(temporary)
        forbidden = root / "src/run/gate/value.rs"
        forbidden.parent.mkdir(parents=True)
        forbidden.write_text(
            "use crate::presentation::render;\n", encoding="utf-8"
        )
        bypass = root / "src/run/gate/bypass.rs"
        bypass.write_text(
            "use crate::{run as execution, presentation as renderer};\n",
            encoding="utf-8",
        )
        comment_only = root / "src/run/comment_only.rs"
        comment_only.write_text(
            "// use crate::presentation::fake;\n/* crate::{presentation} */\n",
            encoding="utf-8",
        )
        admin_reference = root / "src/project/admin.rs"
        admin_reference.parent.mkdir(parents=True)
        admin_reference.write_text(
            'const ADMIN: &str = include_str!("../../tooling/repository/admin.rs");\n',
            encoding="utf-8",
        )
        result = subprocess.run(
            [sys.executable, str(script), str(root)],
            capture_output=True,
            text=True,
            check=False,
        )
        expected = (
            "execution source depends on presentation",
            "product source depends on repository administration",
        )
        if (
            result.returncode == 0
            or any(item not in result.stderr for item in expected)
            or "src/run/gate/bypass.rs: execution source depends on presentation" not in result.stderr
            or "src/run/comment_only.rs:" in result.stderr
        ):
            print("FAIL: seeded forbidden source dependency was not rejected", file=sys.stderr)
            print(result.stdout, result.stderr, file=sys.stderr)
            return 1

    print("Seeded forbidden execution-to-presentation and product-to-repository edges rejected")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", nargs="?", type=Path, default=Path(__file__).resolve().parents[3])
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        return self_test(Path(__file__).resolve())
    return check(args.root.resolve())


if __name__ == "__main__":
    raise SystemExit(main())
