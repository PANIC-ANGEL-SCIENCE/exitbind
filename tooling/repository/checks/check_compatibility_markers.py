#!/usr/bin/env python3
"""Check the compatibility-marker table against the production source tree.

`tooling/repository/checks/compatibility-markers.json` names the compatibility
markers the product recognises, with one writer, one reader, the change that
introduced it and the condition under which it may be retired.

How a marker or record field in `src/` is recognised, so that a new one cannot
land without a row:

- `protocol`: any string literal matching ``^[a-z][A-Za-z0-9]*Protocol$``, and
  any ``#[serde(deny_unknown_fields)]`` field whose snake_case name ends in
  ``_protocol`` (its serde camelCase name is the persisted key).
- `FIELD` extension: the value of a ``const FIELD: &str`` or
  ``const *_FIELD: &str`` declaration (the product's own way of naming a
  record extension).
- record field: every field of a ``#[serde(deny_unknown_fields)]`` struct.
- ledger field: every field the run-ledger allowlist
  (`src/run/state/unknown_fields.rs::reject_unknown`) admits per action,
  read with the same condition/segment structure the source uses; a
  ``module::CONST`` push whose module is unresolved or ambiguous, or whose
  constant is not declared, is a failure, not a skipped field.

Rows of kind `record-kind`, `env` and `text` are presence-checked only: the
check fails when the value is absent from a named writer or reader, but a new
stored line kind, environment control or message text is not discovered.

The record-field rows name the fields those mechanisms declare that the
previous published release's snapshot
(`tests/fixtures/release-record-fields.txt`) does not hold: the one-release
ratchet. A row stays listed, with its writer, reader and retirement condition,
once the snapshot starts holding it, so the table is a superset of the
source-derived additions. `tests/release_integrity/record_fields.rs` reads
`src/` itself and fails when it admits an added field the table does not list
as a `record-field` row; the CHANGELOG's mixed-version paragraph must name
both sets. Protocol markers are not release-scoped: every one in `src/` needs
a row. A row's writer must carry its marker (a record field under its
camelCase or snake_case spelling) and a protocol row's writers and readers
must hold the key literal.

Test-only authoring is not production: files a `#[cfg(test)]` `mod` item
declares (directly or transitively), `tests.rs`/`*_tests.rs` files and items
gated `#[cfg(test)]` or `#[cfg(all(test, ..))]` are cut by their extent.
`compatibility_source.py` mirrors `tests/inventory/rust_source.rs` for that.

`--self-test` seeds D1 (a `protocol` row does not satisfy a record field),
D2/D2b (an ambiguous or unresolved module constant push), D3 (a serde
`_protocol` field) and D4 (a file only a `cfg(test)` `mod` declares), plus the
four record-field mechanisms, an absent row, a missing file and a row without
a retirement condition.

No cargo build: this runs in the product-surface static stage.
"""

from __future__ import annotations

import json
import re
import sys
from functools import lru_cache
from pathlib import Path
from typing import Callable, Dict, List, Optional, Sequence, Set, Tuple

# Set before importing the local scanner so no `__pycache__` lands in the tree.
sys.dont_write_bytecode = True

from compatibility_source import blank, bracket_end, identifier, production, scan  # noqa: E402

TABLE = "tooling/repository/checks/compatibility-markers.json"
SNAPSHOT = "tests/fixtures/release-record-fields.txt"
LEDGER_ALLOWLIST = "src/run/state/unknown_fields.rs"
KINDS = {"protocol", "record-field", "record-kind", "env", "text"}
ROW_FIELDS = ["id", "marker", "kind", "writers", "readers", "introduced", "retire"]
NOT_A_CONDITION = {"never", "none", "n/a", "-"}

PROTOCOL = re.compile(r"^[a-z][A-Za-z0-9]*Protocol$")


# --- recognition ----------------------------------------------------------

def protocol_by_file(sources: Dict[str, str]) -> Dict[str, Set[str]]:
    """Every `*Protocol` key in production source, by file.

    A key is a string literal matching the pattern, or the serde name of a
    `deny_unknown_fields` field whose snake_case name ends in `_protocol`.
    """
    found: Dict[str, Set[str]] = {}
    for path, text in sources.items():
        for _, _, value in scan(text)[0]:
            if PROTOCOL.match(value):
                found.setdefault(value, set()).add(path)
        for rust_name, serde_name in struct_field_pairs(text):
            if rust_name.endswith("_protocol") or PROTOCOL.match(serde_name):
                found.setdefault(serde_name, set()).add(path)
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
def struct_field_pairs(text: str) -> List[Tuple[str, str]]:
    """`(rust, serde)` names of every `deny_unknown_fields` struct field."""
    code = blank(text)
    fields: List[Tuple[str, str]] = []
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
                fields.append((name, rename))
                rename = None
            elif renamed:
                fields.append((name, camel_case(name)))
            else:
                fields.append((name, name))
    return fields


def struct_fields(text: str) -> List[str]:
    """Serde field names of every `#[serde(deny_unknown_fields)]` struct."""
    return [serde for _, serde in struct_field_pairs(text)]


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


def module_constant(
    sources: Dict[str, str], module: str, name: str
) -> Tuple[Optional[str], Optional[str]]:
    """The source file declaring `module::name`, or why it cannot be resolved."""
    matches = [
        path
        for path in sources
        if path.endswith("/" + module + ".rs") or path.endswith("/" + module + "/mod.rs")
    ]
    if not matches:
        return None, (
            f"the run-ledger allowlist pushes {module}::{name}, whose module "
            "resolves to no src/ file"
        )
    if len(matches) > 1:
        return None, (
            f"the run-ledger allowlist pushes {module}::{name}, whose module is "
            f"ambiguous: {', '.join(sorted(matches))}"
        )
    file = matches[0]
    if not re.search(r"\bconst\s+" + re.escape(name) + r"\s*:", blank(sources[file])):
        return None, (
            f"the run-ledger allowlist pushes {module}::{name}, which {file} "
            "does not declare"
        )
    return file, None


def segment_fields(
    sources: Dict[str, str], text: str, start: int, end: int
) -> Tuple[List[str], List[str]]:
    """Fields one run-ledger allowlist segment admits, and resolution errors."""
    code = blank(text)
    literals = [
        item
        for item in scan(text)[0]
        if item[0] >= start and item[1] <= end and identifier(item[2])
    ]
    values = [item[2] for item in literals]
    errors: List[str] = []
    segment = code[start:end]
    for match in re.finditer(
        r"([A-Za-z0-9_]+)::([A-Z][A-Z0-9_]*)", segment
    ):
        module, error = module_constant(sources, match.group(1), match.group(2))
        if error is not None:
            errors.append(error)
        elif module is not None:
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
    return values, errors


def ledger_fields(
    sources: Dict[str, str]
) -> Tuple[Dict[str, Set[str]], List[str]]:
    """`action.field` for the fields the run-ledger allowlist admits.

    The second value names a module constant push the source reader could not
    resolve: the Rust reader fails loudly there, so this check must too.
    """
    text = sources.get(LEDGER_ALLOWLIST)
    if text is None:
        return {}, []
    code = blank(text)
    span = function_span(code, "reject_unknown")
    if span is None:
        return {}, []
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
    errors: List[str] = []
    for index, (_, after, actions) in enumerate(conditions):
        end = conditions[index + 1][0] if index + 1 < len(conditions) else hi
        values, resolution = segment_fields(sources, text, after, end)
        errors.extend(resolution)
        for value in values:
            for action in actions:
                found.setdefault(action + "." + value, set()).add(LEDGER_ALLOWLIST)
    return found, errors


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
    ledger, resolution_errors = ledger_fields(sources)
    errors.extend(resolution_errors)
    added = admitted(records, ledger) - snapshot
    record_rows = {
        row.get("marker")
        for row in rows
        if isinstance(row, dict) and row.get("kind") == "record-field"
    }

    seen_ids: Set[str] = set()
    seen_markers: Dict[Tuple[object, str], int] = {}
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
        kind_key: object = kind if isinstance(kind, str) else repr(kind)
        if (kind_key, marker) in seen_markers:
            errors.append(f"row {number} repeats marker {marker} for kind {kind_key}")
        seen_markers[(kind_key, marker)] = number
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
        if name not in record_rows and bare not in record_rows:
            errors.append(
                f"src/ declares the record field {name} with no table row; "
                "a record-field row is required because it is new since the "
                "previous release's snapshot"
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
        from check_compatibility_markers_selftest import self_test

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


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
