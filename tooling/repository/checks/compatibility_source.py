#!/usr/bin/env python3
"""Read production `src/` the way `tests/inventory/rust_source.rs` does.

The compatibility-marker check runs without a cargo build, so this module is
the Python mirror of the one Rust scanner the inventory tests use: string and
char literals, comments, `#[cfg(..)]` gates, out-of-line `mod` items, `#[path]`
attributes and the transitive set of files only test code declares. A source
change there must change this file too; the release-integrity cross-check and
the check's own self-test pin the semantics.

`production(root)` returns every `src/` file that is not declared only by test
code (and not `tests.rs`/`*_tests.rs`), with items gated `#[cfg(test)]` or
`#[cfg(all(test, ..))]` cut by their extent and newlines kept.
"""

from __future__ import annotations

import re
import sys
from functools import lru_cache
from pathlib import Path
from typing import Dict, List, Optional, Tuple

sys.dont_write_bytecode = True

# A `mod` item: name, out-of-line, gate, `#[path]` value.
ModItem = Tuple[str, bool, Optional[str], Optional[str]]

# `test-reachable` (`cfg(any(test, ..))`) is the weaker gate of the two.
GATE_RANK = {"test-reachable": 0, "test-only": 1}


def identifier(text: str) -> bool:
    """Whether `text` is a field identifier the allowlist can admit."""
    return text.startswith(tuple("abcdefghijklmnopqrstuvwxyz")) and all(
        char.isascii() and char.isalnum() for char in text
    )


@lru_cache(maxsize=None)
def scan(text: str) -> Tuple[List[Tuple[int, int, str]], List[Tuple[int, int]], List[Tuple[int, int]]]:
    """String literals, char literals and comments, each sorted by start."""
    literals: List[Tuple[int, int, str]] = []
    chars: List[Tuple[int, int]] = []
    comments: List[Tuple[int, int]] = []
    n = len(text)
    i = 0

    def word_boundary(at: int) -> bool:
        return at > 0 and (text[at - 1].isalnum() or text[at - 1] == "_")

    while i < n:
        rest = text[i:]
        if rest.startswith("//"):
            end = text.find("\n", i)
            end = n if end < 0 else end
            comments.append((i, end))
            i = end
        elif rest.startswith("/*"):
            end = text.find("*/", i)
            end = n if end < 0 else end + 2
            comments.append((i, end))
            i = end
        elif text[i] == "r" and not word_boundary(i) and re.match(r'r#*"', rest):
            hashes = len(re.match(r'r(#*)"', rest).group(1))
            close = '"' + "#" * hashes
            body = i + 2 + hashes
            end = text.find(close, body)
            end = n if end < 0 else end
            literals.append((i, min(end + len(close), n), text[body:end]))
            i = end + len(close)
        elif text[i] == '"':
            j = i + 1
            while j < n and text[j] != '"':
                j += 2 if text[j] == "\\" else 1
            literals.append((i, min(j + 1, n), text[i + 1:j]))
            i = j + 1
        elif text[i] == "'":
            if i + 2 < n and text[i + 1] == "\\":
                j = i + 3
                while j < n and text[j] != "'":
                    j += 1
                chars.append((i, min(j + 1, n)))
                i = j + 1
            elif i + 2 < n and text[i + 2] == "'":
                chars.append((i, i + 3))
                i += 3
            else:
                i += 1
        else:
            i += 1
    return literals, chars, comments


@lru_cache(maxsize=None)
def blank(text: str) -> str:
    """`text` with comments and literal bodies replaced by spaces, same length."""
    literals, chars, comments = scan(text)
    out = list(text)

    def clear(start: int, end: int) -> None:
        for at in range(start, min(end, len(out))):
            if out[at] != "\n":
                out[at] = " "

    for start, end, _ in literals:
        clear(start + 1, max(end - 1, start + 1))
    for start, end in chars:
        clear(start + 1, max(end - 1, start + 1))
    for start, end in comments:
        clear(start, end)
    return "".join(out)


def bracket_end(code: str, at: int) -> int:
    """The byte after the bracket group opening at `at`."""
    opener = code[at]
    closer = {"[": "]", "(": ")", "{": "}"}[opener]
    depth = 0
    for offset in range(at, len(code)):
        if code[offset] == opener:
            depth += 1
        elif code[offset] == closer:
            depth -= 1
            if depth == 0:
                return offset + 1
    return len(code)


def attribute_end(code: str, at: int) -> int:
    """The `]` closing the attribute that opens at `at` (a `#` or a `[`)."""
    depth = 0
    for offset in range(at, len(code)):
        if code[offset] == "[":
            depth += 1
        elif code[offset] == "]":
            depth -= 1
            if depth == 0:
                return offset + 1
    return len(code)


def item_end(code: str, at: int) -> int:
    """The end of the item at `at`: after its first depth-0 `;` or brace."""
    depth = 0
    for offset in range(at, len(code)):
        byte = code[offset]
        if byte in "([{":
            depth += 1
        elif byte in ")]":
            depth -= 1
        elif byte == "}":
            depth -= 1
            if depth <= 0:
                return offset + 1
        elif byte == ";" and depth == 0:
            return offset + 1
    return len(code)


def gate(attribute: str) -> Optional[str]:
    """How a `#[cfg(..)]` attribute gates its item on `test`.

    `test-only` for `cfg(test)` and `cfg(all(test, ..))`; `test-reachable`
    for `cfg(any(test, ..))`; None otherwise (`cfg(not(test))`,
    `cfg(all(unix, any(.., test)))` and every other shape).
    """
    compact = "".join(char for char in attribute if not char.isspace())
    if not compact.startswith("#[cfg(") or not compact.endswith(")]"):
        return None
    inner = compact[len("#[cfg("):-2]
    if inner == "test":
        return "test-only"
    if inner.startswith("all("):
        only = True
        terms = inner[len("all("):]
    elif inner.startswith("any("):
        only = False
        terms = inner[len("any("):]
    else:
        return None
    if not terms.endswith(")"):
        return None
    terms = terms[:-1]
    depth = 0
    start = 0
    found = False
    for at, char in list(enumerate(terms)) + [(len(terms), ",")]:
        if char == "(":
            depth += 1
        elif char == ")":
            depth -= 1
        elif char == "," and depth == 0:
            found = found or terms[start:at] == "test"
            start = at + 1
    if not found:
        return None
    return "test-only" if only else "test-reachable"


def gated_items(text: str) -> List[Tuple[int, int, str]]:
    """Every item a `#[cfg(..)]` attribute gates on `test`, outermost only."""
    code = blank(text)
    found: List[Tuple[int, int, str]] = []
    for match in re.finditer(r"#\[cfg\(", code):
        at = match.start()
        if found and at < found[-1][1]:
            continue
        close = attribute_end(code, at)
        gated = gate(code[at:close])
        if gated is None:
            continue
        item = close
        while True:
            rest = code[item:]
            item += len(rest) - len(rest.lstrip())
            if code.startswith("#[", item):
                item = attribute_end(code, item)
            else:
                break
        found.append((at, item_end(code, item), gated))
    return found


def strip_test_items(text: str) -> str:
    """Remove test-only items by their own extent, keeping newlines."""
    out: List[str] = []
    cursor = 0
    for start, end, gated in gated_items(text):
        if gated != "test-only":
            continue
        out.append(text[cursor:start])
        out.append("\n" * text[start:end].count("\n"))
        cursor = end
    out.append(text[cursor:])
    return "".join(out)


def mod_items(text: str) -> List[ModItem]:
    """Every `mod` item of `text`, on blanked code; `#[path]` from raw text."""
    code = blank(text)
    items: List[ModItem] = []
    attributes: List[Tuple[int, str]] = []
    offset = 0
    for index, line in enumerate(code.split("\n")):
        start = offset
        offset += len(line) + 1
        trimmed = line.strip()
        if trimmed.startswith("#["):
            at = start + (len(line) - len(line.lstrip()))
            original = text[at:attribute_end(code, at)]
            attributes.append((index, original.replace(" ", "")))
            continue
        item = trimmed
        for prefix in ("pub(crate) ", "pub(super) ", "pub "):
            if item.startswith(prefix):
                item = item[len(prefix):]
                break
        if item.startswith("mod "):
            rest = item[len("mod "):]
            name = "".join(
                char for char in rest
                if char.isalnum() or char == "_"
            )
            after = rest[len(name):].lstrip()
            if name and (after.startswith(";") or after.startswith("{")):
                gated: Optional[str] = None
                for _, attribute in attributes:
                    own = gate(attribute)
                    if own is not None and (
                        gated is None or GATE_RANK[own] > GATE_RANK[gated]
                    ):
                        gated = own
                path: Optional[str] = None
                for _, attribute in attributes:
                    if attribute.startswith('#[path="'):
                        path = attribute[len('#[path="'):].split('"', 1)[0]
                        break
                items.append((name, after.startswith(";"), gated, path))
        if trimmed:
            attributes.clear()
    return items


def declared_file(root: Path, file: str, item: ModItem) -> Optional[str]:
    """The file an out-of-line `mod` item of `file` declares, if it exists."""
    _, _, _, path = item
    parent = file.rsplit("/", 1)[0] if "/" in file else ""
    if path is not None:
        return (parent + "/" + path) if parent else path
    stem = file[:-3] if file.endswith(".rs") else file
    parts = stem.rsplit("/", 1)
    own_dir = parts[0] if len(parts) == 2 and parts[1] in ("mod", "main", "lib") else stem
    for candidate in (own_dir + "/" + item[0] + ".rs", own_dir + "/" + item[0] + "/mod.rs"):
        if (root / candidate).is_file():
            return candidate
    return None


def test_files(root: Path) -> Dict[str, str]:
    """`src/` files that only test code declares, with their gate.

    Starts from every `#[cfg(test)]` out-of-line `mod` item and follows the
    files those declare in turn, exactly like `rust_source.rs::test_files`.
    Files under `tests/` are not read here: a `src/` file declared from a test
    file would be the only case the two diverge.
    """
    files: Dict[str, str] = {}
    for path in sorted((root / "src").rglob("*.rs")):
        relative = path.relative_to(root).as_posix()
        files[relative] = path.read_text(encoding="utf-8")
    known = set(files)
    items = {file: mod_items(text) for file, text in files.items()}
    test: Dict[str, str] = {}
    while True:
        added = False
        for file, declared in items.items():
            for item in declared:
                if not item[1]:
                    continue
                parent = test.get(file)
                own = item[2]
                if parent is None:
                    gated = own
                elif own is None:
                    gated = parent
                else:
                    gated = parent if GATE_RANK[parent] >= GATE_RANK[own] else own
                if gated is None:
                    continue
                child = declared_file(root, file, item)
                if child is None or child not in known:
                    continue
                if child not in test or GATE_RANK[test[child]] < GATE_RANK[gated]:
                    test[child] = gated
                    added = True
        if not added:
            return test


def production(root: Path) -> Dict[str, str]:
    """Production `src/` files: test-only files and items removed."""
    test = test_files(root)
    sources: Dict[str, str] = {}
    for path in sorted((root / "src").rglob("*.rs")):
        relative = path.relative_to(root).as_posix()
        if test.get(relative) == "test-only":
            continue
        if relative.endswith("/tests.rs") or relative.endswith("_tests.rs"):
            continue
        sources[relative] = strip_test_items(path.read_text(encoding="utf-8"))
    return sources
