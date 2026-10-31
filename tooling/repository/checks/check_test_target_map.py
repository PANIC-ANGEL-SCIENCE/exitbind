#!/usr/bin/env python3
"""Validate the live Cargo target relation, features and affected selection.

Git retains old test sources and review decisions. Normal maintenance does not
require a second source-hash/adaptation ledger to change a regression test.
"""
from __future__ import annotations

import copy
import sys
from pathlib import Path
from typing import Optional

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
import test_target_sources  # noqa: E402


def fail(message: str) -> int:
    print(f"FAIL: {message}", file=sys.stderr)
    return 1


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
    # A file the binary embeds (schema, skill body) must select at least the
    # targets that cover its embedding module, not only targets naming it.
    embedded = {
        path: sources for path, sources in test_target_sources.embedded_files(root).items()
        if not path.startswith(("src/", "tests/"))
    }
    if not embedded:
        return "no product-embedded file found; the embedded-file selection rule is untested"
    for path, sources in sorted(embedded.items()):
        selected, _ = test_target_sources.select(root, mapping, manifest, [path])
        covering = {
            name for name, item in mapping["targets"].items()
            if any(source.startswith(cover) for cover in item["covers"] for source in sources)
        }
        if not selected or not covering <= set(selected):
            return f"a change to embedded file {path} does not select the targets covering {sorted(sources)}"
    # A fixture directory a test reads whole, named as a string literal, must
    # select that test for every file inside it. Found here by listing the
    # fixture directories and reading the literal, not through the selector.
    texts = {
        name: "\n".join(
            (root / source).read_text(encoding="utf-8", errors="replace")
            for source in sorted(test_target_sources.target_sources(root, path))
        )
        for name, (path, _) in manifest.items()
    }
    named_directories = 0
    for directory in sorted(p for p in (root / "tests/fixtures").iterdir() if p.is_dir()):
        relative = directory.relative_to(root).as_posix()
        naming = {name for name, text in texts.items() if f'"{relative}"' in text}
        if not naming:
            continue
        named_directories += 1
        for item in sorted(directory.rglob("*")):
            if item.is_file():
                path = item.relative_to(root).as_posix()
                selected, _ = test_target_sources.select(root, mapping, manifest, [path])
                if not naming <= set(selected) or set(selected) == set(manifest):
                    return f"a change to {path} does not select {sorted(naming)}, which read its directory"
    if not named_directories:
        return "no test reads a fixture directory; the directory selection rule is untested"
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
    try:
        mapping, manifest = test_target_sources.load(root)
        errors = test_target_sources.declaration_errors(root, mapping, manifest)
        if errors:
            return fail("; ".join(errors))
        reached = set().union(*(test_target_sources.target_sources(root, path)
                               for path, _ in manifest.values()))
        missing = {p.relative_to(root).as_posix() for p in (root / "tests").glob("*.rs")} - reached
        if missing:
            return fail(f"integration test sources are not compiled by any target: {sorted(missing)}")
        if "autotests = false" not in (root / "Cargo.toml").read_text():
            return fail("Cargo must disable implicit integration-test target discovery")
        for name in mapping.get("repositoryOnlyTargets", []):
            if name not in manifest or "repository-checks" not in manifest[name][1]:
                return fail(f"repository-only target lacks its package feature gate: {name}")
        error = target_declaration_self_test(root, mapping, manifest)
        if error:
            return fail(error)
    except (OSError, ValueError, KeyError, TypeError) as error:
        return fail(f"invalid live test target relation: {error}")
    print(f"Test target relation passed: {len(manifest)} explicit targets; live sources and features")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
