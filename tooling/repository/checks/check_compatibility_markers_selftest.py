#!/usr/bin/env python3
"""Seeded scenarios for `check_compatibility_markers.py --self-test`.

D1 keeps a protocol row from satisfying a record field; D2/D2b refuse an
ambiguous or unresolved module constant push; D3 recognises a serde
`*_protocol` field; D4 treats a file only a `cfg(test)` `mod` declares as test
code. The remaining cases cover the four record-field mechanisms and the
table's row, presence and retirement rules. Each case is chosen so the 2ac1635
check passes it silently; the repaired check must refuse it.

Run through `check_compatibility_markers.py --self-test`; no cargo build.
"""

from __future__ import annotations

import sys
import tempfile
from pathlib import Path
from typing import Dict, List, Set

sys.dont_write_bytecode = True

from check_compatibility_markers import LEDGER_ALLOWLIST, check  # noqa: E402
from compatibility_source import production  # noqa: E402

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

    # D1: the 0.31.22 shape — a new extension constant is both a `*Protocol`
    # key and a start record field. A `protocol` row carries the key but is not
    # a record-field row, so it does not satisfy the field's release entry.
    seeded = base_sources()
    seeded["src/a.rs"] += 'pub(crate) const NEXT_FIELD: &str = "nextNoveltyProtocol";\n'
    seeded[LEDGER_ALLOWLIST] = seeded[LEDGER_ALLOWLIST].replace(
        'allowed.push("submittedReason");',
        'allowed.push("submittedReason");\n            allowed.push(super::a::NEXT_FIELD);',
    )
    protocol_only = base_table()
    protocol_only["markers"].append(
        {
            "id": "next-protocol",
            "marker": "nextNoveltyProtocol",
            "kind": "protocol",
            "writers": ["src/a.rs"],
            "readers": ["src/a.rs"],
            "introduced": "fixture",
            "retire": "when the fixture ends",
        }
    )
    expect(
        "a protocol row does not satisfy a record field",
        seeded,
        protocol_only,
        "nextNoveltyProtocol with no table row",
    )
    record_row = dict(protocol_only["markers"][1])
    record_row["id"] = "next-protocol-field"
    record_row["kind"] = "record-field"
    satisfied = {"schemaVersion": 1, "markers": protocol_only["markers"] + [record_row]}
    if run(seeded, satisfied, BASE_SNAPSHOT):
        failures.append(
            "a record-field row beside the protocol row still failed: "
            f"{run(seeded, satisfied, BASE_SNAPSHOT)}"
        )

    # D2: the module of `record::CONST` names three real files; the Rust
    # reader asserts exactly one and fails, so this check must not skip it.
    seeded = base_sources()
    seeded["src/one/record.rs"] = (
        'pub(crate) const SEEDED_NAME: &str = "ambiguousField";\n'
    )
    seeded["src/two/record.rs"] = (
        'pub(crate) const SEEDED_NAME: &str = "ambiguousField";\n'
    )
    seeded[LEDGER_ALLOWLIST] = seeded[LEDGER_ALLOWLIST].replace(
        'allowed.push("submittedReason");',
        'allowed.push("submittedReason");\n            allowed.push(record::SEEDED_NAME);',
    )
    expect("an ambiguous module constant push", seeded, base_table(), "ambiguous")

    # D2b: `self::CONST` names no module file at all.
    seeded = base_sources()
    seeded[LEDGER_ALLOWLIST] = seeded[LEDGER_ALLOWLIST].replace(
        'allowed.push("submittedReason");',
        'allowed.push("submittedReason");\n            allowed.push(self::SEEDED_NAME);',
    )
    expect(
        "an unresolved module constant push",
        seeded,
        base_table(),
        "resolves to no src/ file",
    )

    # D3: a serde `*_protocol` field writes a key with no literal anywhere.
    seeded = base_sources()
    seeded["src/a.rs"] += (
        "#[derive(Deserialize)]\n"
        '#[serde(rename_all = "camelCase", deny_unknown_fields)]\n'
        "struct Next {\n"
        "    next_header_protocol: String,\n"
        "}\n"
    )
    expect("a serde *_protocol field", seeded, base_table(), "protocol marker nextHeaderProtocol")

    # D4: a file only a `#[cfg(test)] mod` declares is test code, directly and
    # transitively (the Rust `production_in` drops it).
    with tempfile.TemporaryDirectory(prefix="compatibility-selftest-") as temp:
        root = Path(temp)
        for relative, text in base_sources().items():
            (root / relative).parent.mkdir(parents=True, exist_ok=True)
            (root / relative).write_text(text, encoding="utf-8")
        (root / "src/main.rs").write_text(
            "#[cfg(test)]\nmod test_root;\n", encoding="utf-8"
        )
        (root / "src/test_root.rs").write_text(
            'mod deeper;\nconst SEEDED_PROTOCOL: &str = "seededTestOnlyProtocol";\n'
            "#[derive(Deserialize)]\n"
            '#[serde(rename_all = "camelCase", deny_unknown_fields)]\n'
            "struct TestOnly {\n    seeded_test_field: String,\n}\n",
            encoding="utf-8",
        )
        (root / "src/test_root").mkdir(parents=True, exist_ok=True)
        (root / "src/test_root/deeper.rs").write_text(
            'const DEEPER_PROTOCOL: &str = "deeperTestOnlyProtocol";\n',
            encoding="utf-8",
        )
        sources = production(root)
        errors = check(sources, base_table(), BASE_SNAPSHOT, lambda path: sources.get(path))
        if errors:
            failures.append(
                f"a cfg(test)-declared file was read as production (D4): {errors}"
            )

    if failures:
        for failure in failures:
            print("FAIL: " + failure, file=sys.stderr)
    else:
        print(
            "Compatibility-marker self-test passed: protocol/record-field rows "
            "kept distinct; ambiguous and unresolved module pushes refused; "
            "serde *_protocol fields and cfg(test)-declared files recognised; "
            "seeded FIELD, deny_unknown_fields and ledger fields refused; "
            "absent, stale and retirement-free rows refused"
        )
    return 1 if failures else 0


