#!/usr/bin/env python3
"""Check that docs/legacy-compatibility.md inventories every historical reader.

The mechanism is enumerated from source, not from one spelling: every
non-test Rust file under src/ whose code names the old product (any case) or
compares a recorded format version with a number from 1 to 7 must be named in
the "Reader inventory" table. A form marked retired must not be read again.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path
from typing import Dict, List, Set, Tuple

sys.dont_write_bytecode = True

DOC = "docs/legacy-compatibility.md"
HEADING = "## Reader inventory"
COLUMNS = ["Reader", "Old form", "Last write", "Live file or fixture", "Pinning test", "Decision"]

TEST_MODULE = re.compile(r"^\s*#\[cfg\(test\)\]\s*\n\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+\s*\{", re.M)
FULL_LINE_COMMENT = re.compile(r"^\s*//.*$", re.M)
LEGACY_NAME = re.compile(r"soulmate", re.I)
# A version operand: an identifier containing "version", or a JSON "version"
# field read by index or get, optionally followed by accessor calls.
VERSION = (
    r"(?:\w*version\w*|\[\s*\"version\"\s*\]|get\(\s*\"version\"\s*\))"
    r"(?:\s*\.\s*\w+(?:::\w+)*\([^()]*(?:\([^()]*\))?[^()]*\))*"
)
OLD = r"(?:Some\(\s*)?\b[1-7]\b(?!\s*\.\.)"
VERSION_GATES = [
    re.compile(VERSION + r"\s*(?:<=|>=|==|!=|<|>)\s*" + OLD),
    re.compile(r"(?:Some\(\s*)?\b[1-7]\)?\s*(?:<=|>=|==|!=|<|>)\s*" + VERSION),
    re.compile(r"matches!\(\s*[^,]*version[^,]*,[^)]*?\b[1-7]\s*\.\."),
    re.compile(r"\b[1-7]\s*\.\.=?\s*\d+\s*\)\s*\.contains\(\s*&?[^)]*version"),
]
SRC_PATH = re.compile(r"`(src/[^`]+\.rs)`")
TOKEN = re.compile(r"`([^`]+)`")


def code_of(text: str) -> str:
    """Production code: drop a trailing test module and full-line comments."""
    match = TEST_MODULE.search(text)
    if match:
        text = text[: match.start()]
    return FULL_LINE_COMMENT.sub("", text)


def is_test_file(path: str) -> bool:
    name = path.rsplit("/", 1)[-1]
    return name == "tests.rs" or name.endswith("_tests.rs") or "/tests/" in path


def detect(sources: Dict[str, str]) -> Set[str]:
    found = set()
    for path, text in sources.items():
        if is_test_file(path):
            continue
        code = code_of(text)
        if LEGACY_NAME.search(code) or any(gate.search(code) for gate in VERSION_GATES):
            found.add(path)
    return found


def parse_rows(doc: str) -> Tuple[List[List[str]], List[str]]:
    errors: List[str] = []
    if HEADING not in doc:
        return [], [f"{DOC} has no '{HEADING}' section"]
    section = doc.split(HEADING, 1)[1]
    section = re.split(r"^## ", section, maxsplit=1, flags=re.M)[0]
    rows = []
    header_seen = False
    for line in section.splitlines():
        if not line.startswith("|"):
            continue
        cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
        if not header_seen:
            if cells != COLUMNS:
                errors.append(f"inventory columns must be {COLUMNS}, found {cells}")
            header_seen = True
            continue
        if set("".join(cells)) <= set("- "):
            continue
        if len(cells) != len(COLUMNS):
            errors.append(f"inventory row has {len(cells)} cells: {line}")
            continue
        rows.append(cells)
    if not rows:
        errors.append("inventory table has no rows")
    return rows, errors


def check(sources: Dict[str, str], doc: str) -> List[str]:
    rows, errors = parse_rows(doc)
    detected = detect(sources)
    listed: Set[str] = set()
    for reader, form, _last, _live, pinning, decision in rows:
        paths = SRC_PATH.findall(reader)
        if not paths:
            errors.append(f"inventory row names no source file: {reader}")
        if not pinning:
            errors.append(f"inventory row has no pinning test: {reader}")
        retired = decision == "retired"
        if not retired and not decision.startswith("keep"):
            errors.append(f"decision must be 'retired' or start with 'keep': {decision}")
        for path in paths:
            listed.add(path)
            if path not in sources:
                errors.append(f"inventory names a missing source file: {path}")
            elif not retired and path not in detected:
                errors.append(f"inventory keeps a file that reads no historical form: {path}")
        if retired:
            tokens = TOKEN.findall(form)
            if not tokens:
                errors.append(f"retired row names no form token: {form}")
            for token in tokens:
                for path, text in sorted(sources.items()):
                    if not is_test_file(path) and token in code_of(text):
                        errors.append(f"retired form {token} is read again in {path}")
    for path in sorted(detected - listed):
        errors.append(f"historical reader missing from the inventory: {path}")
    return errors


def load(root: Path) -> Tuple[Dict[str, str], str]:
    sources = {
        path.relative_to(root).as_posix(): path.read_text(encoding="utf-8")
        for path in sorted((root / "src").rglob("*.rs"))
    }
    return sources, (root / DOC).read_text(encoding="utf-8")


def self_test() -> int:
    table = "\n".join(
        [
            HEADING,
            "",
            "| " + " | ".join(COLUMNS) + " |",
            "| --- | --- | --- | --- | --- | --- |",
            "| `src/a.rs` | name | never | live | `tests/a.rs` | keep: live |",
            "| `src/b.rs` | gate | append | live | `tests/b.rs` | keep: live |",
            "| `src/c.rs` | `OLD_PREFIX:` | never | none | unit | retired |",
            "",
            "## Next",
        ]
    )
    detected_spellings = {
        "src/legacy_case.rs": 'let config = "SoulMate.JSON";\n',
        "src/get_gate.rs": 'if ev.get("version").and_then(Value::as_u64) == Some(4) {}\n',
        "src/range_gate.rs": "let old = (1..=7).contains(&record_version);\n",
        "src/matches_gate.rs": 'let v = matches!(state["version"].as_u64(), Some(2..=4));\n',
        "src/reversed_gate.rs": "if 5 <= format_version {}\n",
    }
    ignored = {
        "src/current.rs": 'json!({"version": 8}); if version == 8 {}\n',
        "src/comment.rs": "// soulmate history lives in the docs\nfn f() {}\n",
        "src/test_only.rs": 'fn f() {}\n#[cfg(test)]\nmod tests {\n    const P: &str = "soulmate";\n}\n',
        "src/run/tests.rs": 'const P: &str = "soulmate";\n',
    }
    failures = []
    found = detect({**detected_spellings, **ignored})
    for path in detected_spellings:
        if path not in found:
            failures.append(f"detector missed {path}")
    for path in ignored:
        if path in found:
            failures.append(f"detector flagged {path}")
    good = {
        "src/a.rs": 'const P: &str = ".soulmate";\n',
        "src/b.rs": "if version < 6 {}\n",
        "src/c.rs": "fn retired() {}\n",
    }
    if check(good, table):
        failures.append(f"a complete inventory failed: {check(good, table)}")
    cases = {
        "unlisted reader": ({**good, "src/d.rs": "if version >= 3 {}\n"}, "missing from the inventory"),
        "retired form read again": ({**good, "src/e.rs": 'strip("OLD_PREFIX:")\n'}, "read again"),
        "stale keep row": ({**good, "src/b.rs": "fn f() {}\n"}, "reads no historical form"),
        "missing file": ({k: v for k, v in good.items() if k != "src/a.rs"}, "missing source file"),
    }
    for name, (sources, expected) in cases.items():
        if not any(expected in error for error in check(sources, table)):
            failures.append(f"self-test case did not fail: {name}")
    for failure in failures:
        print(f"FAIL: {failure}", file=sys.stderr)
    if not failures:
        print("Legacy inventory self-test passed: mixed spellings found; unlisted, stale and revived readers rejected")
    return 1 if failures else 0


def main(argv: List[str]) -> int:
    if argv == ["--self-test"]:
        return self_test()
    if argv:
        print("usage: check_legacy_inventory.py [--self-test]", file=sys.stderr)
        return 2
    root = Path(__file__).resolve().parents[3]
    sources, doc = load(root)
    errors = check(sources, doc)
    for error in errors:
        print(f"FAIL: {error}", file=sys.stderr)
    if not errors:
        print(f"Legacy reader inventory passed: {len(detect(sources))} source files classified")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
