#!/usr/bin/env python3
"""Check that every baseline integration test remains in its selected target."""

from __future__ import annotations

import copy
import hashlib
import json
import re
import sys
from pathlib import Path
from typing import Optional

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
import test_target_sources  # noqa: E402

SELECTOR_RULE = (
    "<old target>::<nested modules>::<test> maps to "
    "<source module>::<nested modules>::<test>; Cargo target names are not part "
    "of libtest names, and source files and declarations remain at original paths."
)


def fail(message: str) -> int:
    print(f"FAIL: {message}", file=sys.stderr)
    return 1


def selector_contract_error(mapping: dict) -> Optional[str]:
    if mapping.get("selectorRule") != SELECTOR_RULE:
        return "selector rule must exclude Cargo target names from libtest selectors"
    groups = mapping.get("groups")
    if not isinstance(groups, dict):
        return "group selector map must be an object"
    for target, modules in groups.items():
        if not isinstance(modules, list):
            return f"group module map must be a list: {target}"
        for item in modules:
            if not isinstance(item, dict):
                return f"group module entry must be an object: {target}"
            module = item.get("module")
            prefix = item.get("moduleQualifiedPrefix")
            if not isinstance(module, str) or not module or "::" in module:
                return f"source module must be a single Rust module identifier: {target}"
            if prefix != module:
                return (
                    "module-qualified prefix must start at the source module and "
                    f"exclude Cargo target {target}: {module}"
                )
    return None


def selector_contract_self_test() -> bool:
    incorrect = {
        "selectorRule": SELECTOR_RULE,
        "groups": {
            "cargo_target": [
                {"module": "source_module", "moduleQualifiedPrefix": "cargo_target::source_module"}
            ]
        },
    }
    error = selector_contract_error(incorrect)
    return error is not None and "exclude Cargo target cargo_target" in error


def target_declaration_self_test(root: Path, mapping: dict, manifest: dict) -> Optional[str]:
    """Reject a gated target declared without its feature and an uncovered module."""
    gated = [
        name for name, (path, required) in manifest.items()
        if not required and test_target_sources.gated_features(
            root, test_target_sources.target_sources(root, path))
    ]
    if gated:
        dropped = copy.deepcopy(mapping)
        dropped["targets"][gated[0]]["features"] = []
        if not any(gated[0] in error for error in test_target_sources.declaration_errors(root, dropped, manifest)):
            return f"feature check accepted {gated[0]} without its gated feature"
    module, files = next(iter(sorted(test_target_sources.top_level_modules(root).items())))
    uncovered = copy.deepcopy(mapping)
    for item in uncovered["targets"].values():
        item["covers"] = [c for c in item["covers"] if not any(f.startswith(c) for f in files)] or ["src/main.rs"]
    if module != "main" and not any(
        f"covered by no product test target: {module}" in error
        for error in test_target_sources.declaration_errors(root, uncovered, manifest)
    ):
        return f"coverage check accepted uncovered module {module}"
    return None


def main() -> int:
    root = Path(__file__).resolve().parents[3]
    map_path = root / "tooling/repository/checks/test-target-map.json"
    manifest_path = root / "Cargo.toml"
    try:
        mapping = json.loads(map_path.read_text(encoding="utf-8"))
        manifest = manifest_path.read_text(encoding="utf-8")
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        return fail(f"cannot read test target map or Cargo manifest: {error}")

    selector_error = selector_contract_error(mapping)
    if selector_error:
        return fail(selector_error)
    if not selector_contract_self_test():
        return fail("selector validator accepted the target-prefixed negative case")

    entries = [*mapping["standalone"]]
    additions = mapping.get("additions", [])
    if not isinstance(additions, list):
        return fail("new test target additions must be a list")
    entries.extend(additions)
    expected_targets: dict[str, str] = {}
    for item in entries:
        expected_targets[item["newTarget"]] = item["source"]
    for target, modules in mapping["groups"].items():
        expected_targets[target] = f"tests/groups/{target}.rs"
        wrapper = root / expected_targets[target]
        try:
            wrapper_text = wrapper.read_text(encoding="utf-8")
        except (OSError, UnicodeError) as error:
            return fail(f"missing grouped test wrapper {wrapper.relative_to(root)}: {error}")
        actual = re.findall(r'^\s*#\[path\s*=\s*"\.\./([^\"]+\.rs)"\]\s*\n\s*mod\s+(\w+)\s*;', wrapper_text, re.MULTILINE)
        wanted = [(item["source"].removeprefix("tests/"), item["module"]) for item in modules]
        if actual != wanted:
            return fail(f"{wrapper.relative_to(root)} module map differs from the recorded source map")

    root_sources = {path.stem for path in (root / "tests").glob("*.rs")}
    mapped_roots = {Path(item["source"]).stem for item in entries}
    mapped_roots.update(
        Path(item["source"]).stem
        for modules in mapping["groups"].values()
        for item in modules
    )
    if root_sources != mapped_roots or len(root_sources) != mapping["rootCount"] + len(additions):
        return fail("the original integration-test roots are missing, duplicated, or unclassified")

    for item in additions:
        path = root / item["source"]
        try:
            text = path.read_text(encoding="utf-8")
        except (OSError, UnicodeError) as error:
            return fail(f"new test target source is missing: {item['source']}: {error}")
        direct_tests = re.search(r"#\s*\[\s*test\s*\]", text)
        included = re.findall(
            r'^\s*#\[path\s*=\s*"([^"]+\.rs)"\]\s*\n\s*mod\s+\w+\s*;',
            text,
            re.MULTILINE,
        )
        if not direct_tests and not included:
            return fail(f"new test target has no registered tests or explicit test modules: {item['source']}")
        if included and not any(
            re.search(r"#\s*\[\s*test\s*\]", (path.parent / entry).read_text(encoding="utf-8"))
            for entry in included
        ):
            return fail(f"new test target's explicit modules contain no registered tests: {item['source']}")

    target_sections = re.findall(r"(?ms)^\[\[test\]\]\s*(.*?)(?=^\[|\Z)", manifest)
    actual_targets: dict[str, str] = {}
    for section in target_sections:
        name = re.search(r'^name\s*=\s*"([^"]+)"\s*$', section, re.MULTILINE)
        path = re.search(r'^path\s*=\s*"([^"]+)"\s*$', section, re.MULTILINE)
        if name and path:
            actual_targets[name.group(1)] = path.group(1)
    if actual_targets != expected_targets:
        return fail("Cargo integration targets differ from the old-to-new target map")
    repository_targets = set(mapping.get("repositoryOnlyTargets", []))
    for section in target_sections:
        name = re.search(r'^name\s*=\s*"([^"]+)"\s*$', section, re.MULTILINE)
        if name and name.group(1) in repository_targets and 'required-features = ["repository-checks"]' not in section:
            return fail(f"repository-only test target lacks its package feature gate: {name.group(1)}")
    target_manifest = test_target_sources.manifest_targets(manifest)
    declaration_errors = test_target_sources.declaration_errors(root, mapping, target_manifest)
    if declaration_errors:
        return fail("; ".join(declaration_errors))
    self_test_error = target_declaration_self_test(root, mapping, target_manifest)
    if self_test_error:
        return fail(self_test_error)
    if "autotests = false" not in manifest:
        return fail("Cargo must disable implicit integration-test target discovery")

    source_files = mapping["baselineSourceFiles"]
    source_adaptations = mapping.get("sourceAdaptations", {})
    if not isinstance(source_adaptations, dict):
        return fail("source adaptations must be a path-keyed object")
    if len(source_files) != mapping["sourceFileCount"]:
        return fail("baseline test source inventory count does not match its manifest")
    test_count = ignored_count = 0
    adapted_paths: set[str] = set()
    for item in source_files:
        path = root / item["path"]
        try:
            raw = path.read_bytes()
        except OSError as error:
            return fail(f"baseline test source is missing: {item['path']}: {error}")
        actual_sha = hashlib.sha256(raw).hexdigest()
        if actual_sha != item["sha256"]:
            adaptation = source_adaptations.get(item["path"])
            if (not isinstance(adaptation, dict)
                    or adaptation.get("baselineSha256") != item["sha256"]
                    or adaptation.get("currentSha256") != actual_sha
                    or not isinstance(adaptation.get("reason"), str)
                    or not adaptation["reason"].strip()):
                return fail(f"baseline test source changed without an exact recorded adaptation: {item['path']}")
            adapted_paths.add(item["path"])
        elif item["path"] in source_adaptations:
            return fail(f"stale source adaptation entry: {item['path']}")
        text = raw.decode("utf-8")
        test_count += len(re.findall(r"#\s*\[\s*test\s*\]", text))
        ignored_count += len(re.findall(r"#\s*\[\s*ignore\b", text))
    if set(source_adaptations) != adapted_paths:
        return fail("source adaptation entries do not match the changed baseline files")
    if test_count != mapping["declarations"] or ignored_count != mapping["ordinaryIgnoredDeclarations"]:
        return fail("baseline test or ignore declarations differ from the preserved inventory")

    print(
        f"Test target map passed: {mapping['rootCount']} baseline roots plus {len(additions)} new root(s), "
        f"{len(actual_targets)} explicit targets, {test_count} preserved test declarations, "
        f"{len(adapted_paths)} exact fixture-oracle adaptations"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
