#!/usr/bin/env python3
"""Check the compatibility-marker table against the production source tree.

`tooling/repository/checks/compatibility-markers.json` names every in-band
protocol marker and record field the product writes into, or reads from, a
persisted record for format compatibility, with one writer, one reader, the
change that introduced it and the condition under which it may be retired.

How a marker or record field in `src/` is recognised, so that a new one cannot
land without a row:

- `protocol`: any string literal matching ``^[a-z][A-Za-z0-9]*Protocol$``
  (the `*Protocol` keys the run ledger and the Goal journal branch on).
- `FIELD` extension: the value of a ``const FIELD: &str`` or
  ``const *_FIELD: &str`` declaration (the product's own way of naming a
  record extension).
- record field: every field of a ``#[serde(deny_unknown_fields)]`` struct.
- ledger field: every field the run-ledger allowlist
  (`src/run/state/unknown_fields.rs::reject_unknown`) admits per action,
  read with the same condition/segment structure the source uses.

The record-field rows name the fields those mechanisms declare that the
previous published release's snapshot
(`tests/fixtures/release-record-fields.txt`) does not hold: the one-release
ratchet. A row stays listed, with its writer, reader and retirement condition,
once the snapshot starts holding it. They are the field inventory
`tests/release_integrity/record_fields.rs` requires in the CHANGELOG's
mixed-version paragraph, intersected there with the snapshot; the table now
carries the writer/reader/retirement line once. Protocol markers are not
release-scoped: every one in `src/` needs a row. A row's writer must carry its
marker (a record field under its camelCase or snake_case spelling) and a
protocol row's writer and reader must hold the key literal.

Test-only authoring is not production: `tests.rs`/`*_tests.rs` files and
items gated `#[cfg(test)]` or `#[cfg(all(test, ..))]` are cut by their extent.
`--self-test` seeds a new `*Protocol` key, a `deny_unknown_fields` field, a
`FIELD` constant and a run-ledger allowlist field, and a row that is absent
from `src/`, names a missing file or carries no retirement condition.

No cargo build: this runs in the product-surface static stage.
"""

from __future__ import annotations

import json
import re
import sys
from functools import lru_cache
from pathlib import Path
from typing import Callable, Dict, List, Optional, Sequence, Set, Tuple

sys.dont_write_bytecode = True

TABLE = "tooling/repository/checks/compatibility-markers.json"
SNAPSHOT = "tests/fixtures/release-record-fields.txt"
LEDGER_ALLOWLIST = "src/run/state/unknown_fields.rs"
KINDS = {"protocol", "record-field", "record-kind", "env", "text"}
ROW_FIELDS = ["id", "marker", "kind", "writers", "readers", "introduced", "retire"]
NOT_A_CONDITION = {"never", "none", "n/a", "-"}

PROTOCOL = re.compile(r"^[a-z][A-Za-z0-9]*Protocol$")


def identifier(text: str) -> bool:
    """Whether `text` is a field identifier the allowlist can admit."""
    return text.startswith(tuple("abcdefghijklmnopqrstuvwxyz")) and all(
        char.isascii() and char.isalnum() for char in text
    )


# --- Rust source scanning -------------------------------------------------

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


def production(root: Path) -> Dict[str, str]:
    """Production `src/` files: test-only files and items removed."""
    sources: Dict[str, str] = {}
    for path in sorted((root / "src").rglob("*.rs")):
        relative = path.relative_to(root).as_posix()
        if relative.endswith("/tests.rs") or relative.endswith("_tests.rs"):
            continue
        sources[relative] = strip_test_items(path.read_text(encoding="utf-8"))
    return sources


# --- recognition ----------------------------------------------------------

def protocol_by_file(sources: Dict[str, str]) -> Dict[str, Set[str]]:
    """Every `*Protocol` string literal in production source, by file."""
    found: Dict[str, Set[str]] = {}
    for path, text in sources.items():
        for _, _, value in scan(text)[0]:
            if PROTOCOL.match(value):
                found.setdefault(value, set()).add(path)
    return found


@lru_cache(maxsize=None)
def string_constant(text: str, name: str) -> List[str]:
    """The string values of `const NAME: &str = ...` / `&[&str] = &[...]`."""
    code = blank(text)
    literals = scan(text)[0]
    at = code.find("const " + name + ":")
    if at < 0:
        return []
    end = code.find(";", at)
    if end < 0 or "&str" not in code[at:end]:
        return []
    return [value for start, stop, value in literals if start > at and stop <= end]


def matching_brace(code: str) -> int:
    """The offset of the `}` closing the `{` that `code` starts with."""
    depth = 0
    for offset, byte in enumerate(code):
        if byte == "{":
            depth += 1
        elif byte == "}":
            depth -= 1
            if depth == 0:
                return offset
    return len(code)


@lru_cache(maxsize=None)
def struct_fields(text: str) -> List[str]:
    """Fields of every `#[serde(deny_unknown_fields)]` struct, serde names."""
    code = blank(text)
    fields: List[str] = []
    for match in re.finditer("deny_unknown_fields", code):
        at = match.start()
        attribute_start = code.rfind("#[", 0, at)
        if attribute_start < 0:
            continue
        renamed = "camelCase" in text[attribute_start:at]
        rest = code[at:]
        open_at = rest.find("{")
        if open_at < 0 or "struct " not in rest[:open_at] or ";" in rest[:open_at]:
            continue
        body_start = at + open_at + 1
        body_end = at + open_at + matching_brace(rest[open_at:])
        rename: Optional[str] = None
        for line in text[body_start:body_end].splitlines():
            line = line.strip()
            if line.startswith("#[serde("):
                rename = None
                if 'rename = "' in line:
                    rename = line.split('rename = "', 1)[1].split('"', 1)[0]
                continue
            for prefix in ("pub(crate) ", "pub(super) "):
                if line.startswith(prefix):
                    line = line[len(prefix):]
            if line.startswith("pub "):
                line = line[len("pub "):]
            if ":" not in line:
                continue
            name = line.split(":", 1)[0]
            if not name or not all(
                char.isascii() and (char.islower() or char.isdigit() or char == "_")
                for char in name
            ):
                continue
            if rename is not None:
                fields.append(rename)
                rename = None
            elif renamed:
                fields.append(camel_case(name))
            else:
                fields.append(name)
    return fields


def snake_case(name: str) -> str:
    out: List[str] = []
    for char in name:
        if char.isupper():
            out.append("_")
            out.append(char.lower())
        else:
            out.append(char)
    return "".join(out)


def names_field(text: str, marker: str) -> bool:
    """Whether `text` names the record field under any spelling it writes."""
    field = marker.rsplit(".", 1)[-1]
    for candidate in {marker, field, snake_case(field)}:
        if candidate and re.search(r"\b" + re.escape(candidate) + r"\b", text):
            return True
    return False


def camel_case(name: str) -> str:
    out: List[str] = []
    upper = False
    for char in name:
        if char == "_":
            upper = True
        elif upper:
            out.append(char.upper())
            upper = False
        else:
            out.append(char)
    return "".join(out)


def record_fields_by_file(sources: Dict[str, str]) -> Dict[str, Set[str]]:
    """Fields declared by the record-field mechanisms, by file."""
    found: Dict[str, Set[str]] = {}
    for path, text in sources.items():
        code = blank(text)
        for match in re.finditer(r"const\s+([A-Za-z0-9_]+)\s*:", code):
            name = match.group(1)
            if name == "FIELD" or name.endswith("_FIELD"):
                for value in string_constant(text, name):
                    found.setdefault(value, set()).add(path)
        for field in struct_fields(text):
            found.setdefault(field, set()).add(path)
    return found


def function_span(code: str, name: str) -> Optional[Tuple[int, int]]:
    for match in re.finditer(r"\bfn\s+" + re.escape(name) + r"\s*\(", code):
        at = match.start()
        depth = 0
        for offset in range(at, len(code)):
            byte = code[offset]
            if byte in "([":
                depth += 1
            elif byte in ")]":
                depth -= 1
            elif byte == "{" and depth == 0:
                return at, bracket_end(code, offset)
            elif byte == ";" and depth == 0:
                break
    return None


def module_file(sources: Dict[str, str], module: str) -> Optional[str]:
    matches = [
        path
        for path in sources
        if path.endswith("/" + module + ".rs") or path.endswith("/" + module + "/mod.rs")
    ]
    return matches[0] if len(matches) == 1 else None


def segment_fields(
    sources: Dict[str, str], text: str, start: int, end: int
) -> List[str]:
    """Fields one run-ledger allowlist segment admits."""
    code = blank(text)
    literals = [
        item
        for item in scan(text)[0]
        if item[0] >= start and item[1] <= end and identifier(item[2])
    ]
    values = [item[2] for item in literals]
    segment = code[start:end]
    for match in re.finditer(
        r"([A-Za-z0-9_]+)::([A-Z][A-Z0-9_]*)", segment
    ):
        module = module_file(sources, match.group(1))
        if module:
            values.extend(string_constant(sources[module], match.group(2)))
    if "append_allowed_fields(" in segment:
        for module, module_text in sources.items():
            module_code = blank(module_text)
            inner = function_span(module_code, "append_allowed_fields")
            if inner is None:
                continue
            inner_start, inner_stop = inner
            values.extend(
                item[2]
                for item in scan(module_text)[0]
                if item[0] >= inner_start and item[1] <= inner_stop
            )
            for constant in re.findall(
                r"[A-Z][A-Z0-9_]*", module_code[inner_start:inner_stop]
            ):
                values.extend(string_constant(module_text, constant))
    return values


def ledger_fields(sources: Dict[str, str]) -> Dict[str, Set[str]]:
    """`action.field` for the fields the run-ledger allowlist admits."""
    text = sources.get(LEDGER_ALLOWLIST)
    if text is None:
        return {}
    code = blank(text)
    span = function_span(code, "reject_unknown")
    if span is None:
        return {}
    lo, hi = span
    literals = [item for item in scan(text)[0] if lo <= item[0] and item[1] <= hi]
    conditions: List[Tuple[int, int, List[str]]] = []
    for match in re.finditer(r"action ==", code[lo:hi]):
        position = lo + match.start()
        literal = next((item for item in literals if item[0] > position), None)
        if literal:
            conditions.append((position, literal[1], [literal[2]]))
    for match in re.finditer(r"matches!\(", code[lo:hi]):
        position = lo + match.start()
        rest = code[lo + match.end():hi]
        stripped = rest.lstrip()
        if not stripped.startswith("action,"):
            continue
        close = lo + match.end() + (len(rest) - len(stripped)) + stripped.find(")")
        names = [
            item[2] for item in literals if position < item[0] and item[1] <= close
        ]
        conditions.append((position, close, names))
    for match in re.finditer(r"\} else \{", code[lo:hi]):
        position = lo + match.start()
        conditions.append((position, lo + match.end(), ["other"]))
    conditions.sort(key=lambda condition: condition[0])
    found: Dict[str, Set[str]] = {}
    for index, (_, after, actions) in enumerate(conditions):
        end = conditions[index + 1][0] if index + 1 < len(conditions) else hi
        for value in segment_fields(sources, text, after, end):
            for action in actions:
                found.setdefault(action + "." + value, set()).add(LEDGER_ALLOWLIST)
    return found


def admitted(
    records: Dict[str, Set[str]], ledger: Dict[str, Set[str]]
) -> Set[str]:
    return set(records) | set(ledger)


def snapshot_fields(text: str) -> Set[str]:
    return {
        line.strip()
        for line in text.splitlines()
        if line.strip() and not line.startswith("#") and not line.startswith("release ")
    }


# --- the check ------------------------------------------------------------

def check(
    sources: Dict[str, str],
    table: dict,
    snapshot: Set[str],
    read: Callable[[str], Optional[str]],
) -> List[str]:
    errors: List[str] = []
    rows = table.get("markers")
    if table.get("schemaVersion") != 1 or not isinstance(rows, list):
        return ["compatibility table needs schemaVersion 1 and a markers list"]
    protocols = protocol_by_file(sources)
    records = record_fields_by_file(sources)
    ledger = ledger_fields(sources)
    added = admitted(records, ledger) - snapshot

    seen_ids: Set[str] = set()
    seen_markers: Dict[str, int] = {}
    protocol_rows: Set[str] = set()
    for number, row in enumerate(rows, 1):
        if not isinstance(row, dict):
            errors.append(f"row {number} is not an object")
            continue
        missing = [field for field in ROW_FIELDS if field not in row]
        if missing:
            errors.append(f"row {number} is missing {missing}")
            continue
        marker = row["marker"]
        kind = row["kind"]
        if not isinstance(row["id"], str) or not row["id"].strip():
            errors.append(f"row {number} has no id")
        elif row["id"] in seen_ids:
            errors.append(f"row {number} repeats id {row['id']}")
        seen_ids.add(row["id"] if isinstance(row["id"], str) else "")
        if not isinstance(marker, str) or not marker.strip():
            errors.append(f"row {number} has no marker")
            continue
        if marker in seen_markers:
            errors.append(f"row {number} repeats marker {marker}")
        seen_markers[marker] = number
        if kind not in KINDS:
            errors.append(f"row {number} has kind {kind!r}; use one of {sorted(KINDS)}")
        if not isinstance(row["introduced"], str) or not row["introduced"].strip():
            errors.append(f"row {number} ({marker}) names no introducing change")
        retire = row["retire"]
        if (
            not isinstance(retire, str)
            or not retire.strip()
            or retire.strip().lower() in NOT_A_CONDITION
        ):
            errors.append(f"row {number} ({marker}) has no retirement condition")
        writers = row["writers"] if isinstance(row["writers"], list) else []
        readers = row["readers"] if isinstance(row["readers"], list) else []
        if not writers or not readers:
            errors.append(f"row {number} ({marker}) needs a writer and a reader")
        for path in list(writers) + list(readers):
            if not isinstance(path, str) or not path:
                errors.append(f"row {number} ({marker}) names an empty file")
            elif read(path) is None:
                errors.append(f"row {number} ({marker}) names a missing file: {path}")
        present = [
            path for path in list(writers) + list(readers) if isinstance(path, str)
        ]
        if kind == "protocol":
            protocol_rows.add(marker)
            if marker not in protocols:
                errors.append(f"row {number} lists {marker}, which no src/ file reads")
            if not any(path in protocols.get(marker, set()) for path in writers):
                errors.append(f"row {number} ({marker}) names no writer that carries it")
            if not any(path in protocols.get(marker, set()) for path in readers):
                errors.append(f"row {number} ({marker}) names no reader that carries it")
        elif kind == "record-field":
            if marker not in set(records) | set(ledger):
                errors.append(f"row {number} lists {marker}, which no src/ record declares")
            if not any(
                isinstance(path, str)
                and names_field(read(path) or "", marker)
                for path in writers
            ):
                errors.append(f"row {number} ({marker}) names no writer that carries it")
            carriers = records.get(marker, set()) | ledger.get(marker, set())
            if not any(path in carriers for path in readers):
                errors.append(f"row {number} ({marker}) names no reader that carries it")
        elif kind == "record-kind":
            if not any(path in sources and marker in sources[path] for path in present):
                errors.append(f"row {number} ({marker}) names no src/ file that carries it")
        else:
            for path in present:
                text = read(path)
                if text is not None and marker not in text:
                    errors.append(f"row {number} ({marker}) is absent from {path}")

    for marker in sorted(set(protocols) - protocol_rows):
        errors.append(
            f"src/ carries the protocol marker {marker} with no table row; "
            f"files: {', '.join(sorted(protocols[marker]))}"
        )
    for name in sorted(added):
        bare = name.rsplit(".", 1)[-1]
        if not any(row.get("marker") in (name, bare) for row in rows if isinstance(row, dict)):
            errors.append(
                f"src/ declares the record field {name} with no table row; "
                "it is new since the previous release's snapshot"
            )
    return errors


def load(root: Path) -> Tuple[Dict[str, str], dict, Set[str], Callable[[str], Optional[str]]]:
    sources = production(root)
    table = json.loads((root / TABLE).read_text(encoding="utf-8"))
    snapshot = snapshot_fields((root / SNAPSHOT).read_text(encoding="utf-8"))

    def read(path: str) -> Optional[str]:
        if path in sources:
            return sources[path]
        target = root / path
        return target.read_text(encoding="utf-8") if target.is_file() else None

    return sources, table, snapshot, read


def main(argv: Sequence[str]) -> int:
    if argv == ["--self-test"]:
        return self_test()
    if argv:
        print("usage: check_compatibility_markers.py [--self-test]", file=sys.stderr)
        return 2
    root = Path(__file__).resolve().parents[3]
    sources, table, snapshot, read = load(root)
    errors = check(sources, table, snapshot, read)
    for error in errors:
        print("FAIL: " + error, file=sys.stderr)
    if not errors:
        protocols = sum(1 for row in table["markers"] if row.get("kind") == "protocol")
        print(
            "Compatibility-marker inventory passed: "
            f"{protocols} protocol markers and {len(table['markers'])} rows checked"
        )
    return 1 if errors else 0


# --- self-test ------------------------------------------------------------

BASE_SOURCE = """\
pub(crate) const FIELD: &str = "seededExtension";
pub(crate) const PROTOCOL_LIKE: &str = "thingProtocol";
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    run_id: String,
}
"""

BASE_LEDGER = """\
fn reject_unknown(object: &Map<String, Value>, action: &str, version: u64, line: usize) -> Result<(), String> {
    let allowed: &[&str] = if action == "start" {
        let mut allowed = vec!["version", "kind"];
        if version >= 8 {
            allowed.push("submittedReason");
        }
        allowed
    } else {
        vec!["version"]
    };
    reject_unknown_fields(object, &allowed, line)
}
"""

BASE_SNAPSHOT = {
    "start.version",
    "start.kind",
    "start.submittedReason",
    "other.version",
    "runId",
    "seededExtension",
}


def base_sources() -> Dict[str, str]:
    return {"src/a.rs": BASE_SOURCE, LEDGER_ALLOWLIST: BASE_LEDGER}


def base_table(**overrides: object) -> dict:
    row = {
        "id": "thing-protocol",
        "marker": "thingProtocol",
        "kind": "protocol",
        "writers": ["src/a.rs"],
        "readers": ["src/a.rs"],
        "introduced": "fixture",
        "retire": "when the fixture ends",
    }
    row.update(overrides)
    return {"schemaVersion": 1, "markers": [row]}


def self_test() -> int:
    failures: List[str] = []
    read = lambda path: base_sources().get(path)  # noqa: E731

    def run(sources: Dict[str, str], table: dict, snapshot: Set[str]) -> List[str]:
        return check(sources, table, snapshot, lambda path: sources.get(path))

    if run(base_sources(), base_table(), BASE_SNAPSHOT):
        failures.append(f"a complete fixture failed: {run(base_sources(), base_table(), BASE_SNAPSHOT)}")

    def expect(name: str, sources: Dict[str, str], table: dict, needle: str, snapshot: Set[str] = BASE_SNAPSHOT) -> None:
        errors = run(sources, table, snapshot)
        if not any(needle in error for error in errors):
            failures.append(f"self-test case did not fail ({name}): {errors}")

    seeded = base_sources()
    seeded["src/a.rs"] += 'const OTHER: &str = "seededNewProtocol";\n'
    expect("seeded protocol key", seeded, base_table(), "seededNewProtocol with no table row")

    seeded = base_sources()
    seeded["src/a.rs"] = seeded["src/a.rs"].replace("    run_id: String,", "    run_id: String,\n    seeded_record: String,")
    expect("seeded deny_unknown_fields field", seeded, base_table(), "seededRecord with no table row")

    seeded = base_sources()
    seeded["src/a.rs"] += 'const OTHER_FIELD: &str = "seededExtensionField";\n'
    expect("seeded FIELD constant", seeded, base_table(), "seededExtensionField with no table row")

    seeded = base_sources()
    seeded[LEDGER_ALLOWLIST] = seeded[LEDGER_ALLOWLIST].replace(
        'allowed.push("submittedReason");',
        'allowed.push("submittedReason");\n            allowed.push("seededLedgerField");',
    )
    expect("seeded ledger allowlist field", seeded, base_table(), "start.seededLedgerField with no table row")

    expect("listed marker absent from src", base_sources(), base_table(marker="absentProtocol"), "absentProtocol")

    expect("row without a retirement condition", base_sources(), base_table(retire=""), "no retirement condition")

    stale = base_table()
    stale["markers"].append(
        {
            "id": "gone",
            "marker": "goneProtocol",
            "kind": "protocol",
            "writers": ["src/missing.rs"],
            "readers": ["src/missing.rs"],
            "introduced": "fixture",
            "retire": "when the fixture ends",
        }
    )
    expect("missing writer file", base_sources(), stale, "missing file: src/missing.rs")

    if failures:
        for failure in failures:
            print("FAIL: " + failure, file=sys.stderr)
    else:
        print(
            "Compatibility-marker self-test passed: seeded protocol, deny_unknown_fields, "
            "FIELD and ledger fields refused; absent, stale and retirement-free rows refused"
        )
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
