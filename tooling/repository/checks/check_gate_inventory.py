#!/usr/bin/env python3
"""Check the gate inventory against every native CI gate and the failure ledger.

The gate set is enumerated from the mechanism itself: the stages that
ci-stages.sh prints from ci_stage_order, plus every label that ci-local.sh
passes to its run helper. Each gate is listed once with a cost class, the
failure class it prevents, and exactly one justification: ledger references to
real failures, a provisional date, or a deterrent reason.

--append records one failure line in the ledger (used by ci-failure.sh).
"""

from __future__ import annotations

import datetime
import json
import re
import subprocess
import sys
from pathlib import Path
from typing import Dict, List, Set

sys.dont_write_bytecode = True

INVENTORY = "tooling/repository/ci/gate-inventory.json"
LEDGER = "tooling/repository/ci/failure-ledger.jsonl"
STAGES = "tooling/repository/ci/ci-stages.sh"
LOCAL = "tooling/repository/ci/ci-local.sh"
COSTS = {"light", "build", "heavy"}
JUSTIFICATIONS = ("ledger", "provisionalUntil", "deterrent")
LEDGER_FIELDS = ["id", "date", "gate", "class", "source", "summary", "resolution"]
LEDGER_ID = re.compile(r"^fl-\d{4}$")
# The run helper at command position: line start, or after ; ) { & | or a
# shell keyword. Its first word (optionally quoted) is the gate label.
RUN_CALL = re.compile(
    r"(?:^|[;){&|]|\bthen\b|\bdo\b|\belse\b)[ \t]*run[ \t]+([\"']?)([A-Za-z0-9_-]+)\1(?=[ \t]|$)",
    re.M,
)
COMMENT = re.compile(r"^[ \t]*#.*$", re.M)


def run_labels(script: str) -> List[str]:
    labels: List[str] = []
    for match in RUN_CALL.finditer(COMMENT.sub("", script)):
        if match.group(2) not in labels:
            labels.append(match.group(2))
    return labels


def stage_order(root: Path) -> List[str]:
    output = subprocess.run(
        ["sh", "-c", '. "$1"; ci_stage_order', "sh", str(root / STAGES)],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    return [line for line in output.splitlines() if line]


def valid_date(value: object) -> bool:
    if not isinstance(value, str) or not re.fullmatch(r"\d{4}-\d{2}-\d{2}", value):
        return False
    try:
        datetime.date.fromisoformat(value)
    except ValueError:
        return False
    return True


def workflow_step_exists(root: Path, gate: str) -> bool:
    reference = gate[len("workflow:") :]
    path, _, step = reference.partition("#")
    if not path.startswith(".github/workflows/") or not step:
        return False
    workflow = root / path
    if not workflow.is_file():
        return False
    names = re.findall(r"^\s*-?\s*name:\s*(.+?)\s*$", workflow.read_text(encoding="utf-8"), re.M)
    return step in [name.strip("'\"") for name in names]


def ledger_errors(root: Path, lines: List[str], gate_ids: Set[str]) -> List[str]:
    errors: List[str] = []
    seen: Set[str] = set()
    for number, line in enumerate(lines, 1):
        try:
            entry = json.loads(line)
        except json.JSONDecodeError:
            errors.append(f"ledger line {number} is not JSON")
            continue
        if not isinstance(entry, dict) or list(entry) != LEDGER_FIELDS:
            errors.append(f"ledger line {number} must have fields {LEDGER_FIELDS} in order")
            continue
        if any(not isinstance(entry[field], str) or not entry[field].strip() for field in LEDGER_FIELDS):
            errors.append(f"ledger line {number} has an empty field")
            continue
        if not LEDGER_ID.match(entry["id"]) or entry["id"] in seen:
            errors.append(f"ledger line {number} id must be a new fl-NNNN: {entry['id']}")
        seen.add(entry["id"])
        if not valid_date(entry["date"]):
            errors.append(f"ledger line {number} date must be YYYY-MM-DD: {entry['date']}")
        gate = entry["gate"]
        if gate.startswith("workflow:"):
            if not workflow_step_exists(root, gate):
                errors.append(f"ledger line {number} names no existing workflow step: {gate}")
        elif gate not in gate_ids:
            errors.append(f"ledger line {number} names an unknown gate: {gate}")
    return errors


def read_ledger(root: Path) -> List[str]:
    path = root / LEDGER
    if not path.is_file():
        return []
    return [line for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]


def check(root: Path, inventory: dict, stages: List[str], labels: List[str], ledger: List[str]) -> List[str]:
    errors: List[str] = []
    gates = inventory.get("gates")
    if inventory.get("schemaVersion") != 1 or not isinstance(gates, list):
        return ["gate inventory must have schemaVersion 1 and a gates list"]
    expected: Dict[str, str] = {stage: "stage" for stage in stages}
    for label in labels:
        expected.setdefault(label, "trailing")
    listed: Dict[str, dict] = {}
    for gate in gates:
        gate_id = gate.get("id") if isinstance(gate, dict) else None
        if not isinstance(gate_id, str) or gate_id in listed:
            errors.append(f"gate id missing or duplicated: {gate_id}")
            continue
        listed[gate_id] = gate
    for gate_id in expected:
        if gate_id not in listed:
            errors.append(f"gate missing from the inventory: {gate_id}")
    for gate_id in listed:
        if gate_id not in expected:
            errors.append(f"inventory lists a gate that no longer runs: {gate_id}")
    if [g for g in listed if g in expected] != list(expected):
        errors.append("inventory order must follow ci_stage_order, then ci-local.sh run order")
    stage_label_gaps = [stage for stage in stages if stage not in labels]
    for stage in stage_label_gaps:
        errors.append(f"stage has no ci-local.sh run label: {stage}")
    ledger_gates: Dict[str, str] = {}
    for line in ledger:
        try:
            entry = json.loads(line)
            ledger_gates[entry["id"]] = entry["gate"]
        except (json.JSONDecodeError, KeyError, TypeError):
            pass
    for gate_id, gate in listed.items():
        if gate_id in expected and gate.get("kind") != expected[gate_id]:
            errors.append(f"gate {gate_id} kind must be {expected[gate_id]}")
        if gate.get("cost") not in COSTS:
            errors.append(f"gate {gate_id} cost must be one of {sorted(COSTS)}")
        if not isinstance(gate.get("prevents"), str) or not gate["prevents"].strip():
            errors.append(f"gate {gate_id} must name the failure class it prevents")
        present = [key for key in JUSTIFICATIONS if key in gate]
        if len(present) != 1:
            errors.append(f"gate {gate_id} needs exactly one of {list(JUSTIFICATIONS)}")
            continue
        value = gate[present[0]]
        if present[0] == "ledger":
            if not isinstance(value, list) or not value:
                errors.append(f"gate {gate_id} ledger must list failure ids")
                continue
            for ref in value:
                if ledger_gates.get(ref) != gate_id:
                    errors.append(f"gate {gate_id} ledger ref {ref} is not a ledger line for this gate")
        elif present[0] == "provisionalUntil" and not valid_date(value):
            errors.append(f"gate {gate_id} provisionalUntil must be YYYY-MM-DD")
        elif present[0] == "deterrent" and (not isinstance(value, str) or not value.strip()):
            errors.append(f"gate {gate_id} deterrent reason is empty")
    errors.extend(ledger_errors(root, ledger, set(expected)))
    return errors


def load(root: Path) -> List[str]:
    inventory = json.loads((root / INVENTORY).read_text(encoding="utf-8"))
    labels = run_labels((root / LOCAL).read_text(encoding="utf-8"))
    return check(root, inventory, stage_order(root), labels, read_ledger(root))


def append(root: Path, args: List[str]) -> int:
    options: Dict[str, str] = {}
    allowed = {"--gate", "--class", "--source", "--summary", "--resolution", "--date"}
    while args:
        if len(args) < 2 or args[0] not in allowed or args[0] in options:
            print(f"usage: --append {' '.join(sorted(o + ' VALUE' for o in allowed))}", file=sys.stderr)
            return 2
        options[args[0]] = args[1]
        args = args[2:]
    missing = sorted(allowed - {"--date"} - set(options))
    if missing:
        print(f"--append requires {', '.join(missing)}", file=sys.stderr)
        return 2
    ledger = read_ledger(root)
    numbers = [int(json.loads(line)["id"][3:]) for line in ledger if LEDGER_ID.match(json.loads(line).get("id", ""))]
    entry = {
        "id": f"fl-{max(numbers, default=0) + 1:04d}",
        "date": options.get("--date", datetime.date.today().isoformat()),
        "gate": options["--gate"],
        "class": options["--class"],
        "source": options["--source"],
        "summary": options["--summary"],
        "resolution": options["--resolution"],
    }
    line = json.dumps(entry, separators=(",", ":"))
    inventory = json.loads((root / INVENTORY).read_text(encoding="utf-8"))
    gate_ids = {gate["id"] for gate in inventory.get("gates", []) if isinstance(gate, dict) and "id" in gate}
    errors = ledger_errors(root, ledger + [line], gate_ids)
    if errors:
        for error in errors:
            print(f"FAIL: {error}", file=sys.stderr)
        return 1
    with open(root / LEDGER, "a", encoding="utf-8") as handle:
        handle.write(line + "\n")
    print(line)
    return 0


def self_test(root: Path) -> int:
    script = "\n".join(
        [
            "# Shared checks; run from any directory.",
            "run() {",
            "  :",
            "}",
            "for stage in a b; do",
            "  case $stage in",
            "    a) run a ./a.sh ;;",
            '    b) run  "b" ./b.sh ;;',
            "  esac",
            "done",
            "if true; then run tail-one x; fi",
            "test -n x && run tail-two y",
        ]
    )
    failures: List[str] = []
    labels = run_labels(script)
    if labels != ["a", "b", "tail-one", "tail-two"]:
        failures.append(f"run labels in mixed spellings not all found: {labels}")
    good = {
        "schemaVersion": 1,
        "gates": [
            {"id": "a", "kind": "stage", "cost": "light", "prevents": "x", "ledger": ["fl-0001"]},
            {"id": "b", "kind": "stage", "cost": "build", "prevents": "x", "deterrent": "why"},
            {"id": "tail-one", "kind": "trailing", "cost": "heavy", "prevents": "x", "provisionalUntil": "2027-01-01"},
            {"id": "tail-two", "kind": "trailing", "cost": "light", "prevents": "x", "deterrent": "why"},
        ],
    }
    line = json.dumps(dict(zip(LEDGER_FIELDS, ["fl-0001", "2026-10-21", "a", "c", "s", "m", "r"])))
    if check(root, good, ["a", "b"], labels, [line]):
        failures.append(f"a complete inventory failed: {check(root, good, ['a', 'b'], labels, [line])}")

    def variant(change) -> dict:
        copy = json.loads(json.dumps(good))
        change(copy["gates"])
        return copy

    cases = {
        "unlisted trailing gate": (good, labels + ["tail-three"], [line], "missing from the inventory"),
        "stale gate": (variant(lambda g: g.append(dict(g[1], id="gone"))), labels, [line], "no longer runs"),
        "two justifications": (variant(lambda g: g[1].update(provisionalUntil="2027-01-01")), labels, [line], "exactly one"),
        "bad cost": (variant(lambda g: g[0].update(cost="cheap")), labels, [line], "cost must be"),
        "dangling ledger ref": (good, labels, [], "not a ledger line"),
        "unknown ledger gate": (good, labels, [line, line.replace("fl-0001", "fl-0002").replace('"a"', '"zz"')], "unknown gate"),
    }
    for name, (inventory, case_labels, ledger, expected) in cases.items():
        if not any(expected in error for error in check(root, inventory, ["a", "b"], case_labels, ledger)):
            failures.append(f"self-test case did not fail: {name}")
    for failure in failures:
        print(f"FAIL: {failure}", file=sys.stderr)
    if not failures:
        print("Gate inventory self-test passed: mixed run spellings found; missing, stale and unjustified gates rejected")
    return 1 if failures else 0


def main(argv: List[str]) -> int:
    root = Path(__file__).resolve().parents[3]
    if argv == ["--self-test"]:
        return self_test(root)
    if argv[:1] == ["--append"]:
        return append(root, argv[1:])
    if argv:
        print("usage: check_gate_inventory.py [--self-test | --append ...]", file=sys.stderr)
        return 2
    errors = load(root)
    for error in errors:
        print(f"FAIL: {error}", file=sys.stderr)
    if not errors:
        inventory = json.loads((root / INVENTORY).read_text(encoding="utf-8"))
        kinds = [gate["kind"] for gate in inventory["gates"]]
        print(
            f"Gate inventory passed: {kinds.count('stage')} stages, {kinds.count('trailing')} trailing gates, "
            f"{len(read_ledger(root))} ledger lines"
        )
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
