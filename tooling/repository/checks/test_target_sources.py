#!/usr/bin/env python3
"""Resolve Cargo test targets to their sources, features and affected selection.

The test-target map declares, per Cargo test target, the features it must run
with and the repository input prefixes it exercises (`src/` and `tests/`). This module derives the facts
those declarations must agree with (manifest `required-features`, feature-gated
test code reached by each target) and selects the targets a change affects.

Usage: test_target_sources.py affected --base BASE
  prints one `TARGET<TAB>FEATURES` line per selected target (FEATURES is a
  comma-separated list, possibly empty) and `#` reason lines on stderr.
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
from pathlib import Path
from typing import Dict, Iterable, List, Set, Tuple

MAP_PATH = "tooling/repository/checks/test-target-map.json"
# A change to one of these can alter every target's build or shared support.
SELECT_ALL_FILES = ("Cargo.toml", "Cargo.lock")
SELECT_ALL_PREFIXES = ("tests/support/",)
FEATURE = re.compile(r'feature\s*=\s*"([A-Za-z0-9_-]+)"')
PATH_MOD = re.compile(r'#\[path\s*=\s*"([^"]+)"\]\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+\s*;')
PLAIN_MOD = re.compile(r'^(?![^\n]*#\[path)[ \t]*(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*;', re.MULTILINE)
INCLUDE = re.compile(r'include!\s*\(\s*"([^"]+)"\s*\)')
EMBED = re.compile(r'include_(?:str|bytes)!\s*\(\s*"([^"]+)"\s*,?\s*\)')


def manifest_targets(manifest: str) -> Dict[str, Tuple[str, List[str]]]:
    """Return Cargo `[[test]]` targets as name -> (path, required-features)."""
    targets: Dict[str, Tuple[str, List[str]]] = {}
    for section in re.findall(r"(?ms)^\[\[test\]\]\s*(.*?)(?=^\[|\Z)", manifest):
        name = re.search(r'^name\s*=\s*"([^"]+)"\s*$', section, re.MULTILINE)
        path = re.search(r'^path\s*=\s*"([^"]+)"\s*$', section, re.MULTILINE)
        required = re.search(r'^required-features\s*=\s*\[([^\]]*)\]\s*$', section, re.MULTILINE)
        features = re.findall(r'"([^"]+)"', required.group(1)) if required else []
        if name and path:
            targets[name.group(1)] = (path.group(1), sorted(features))
    return targets


def _strip_line_comments(text: str) -> str:
    return re.sub(r"(?m)^\s*//.*$", "", text)


def target_sources(root: Path, entry: str) -> Set[str]:
    """Every repository-relative Rust file a test target compiles or includes."""
    seen: Set[str] = set()
    pending = [entry]
    while pending:
        relative = pending.pop()
        if relative in seen:
            continue
        path = root / relative
        try:
            text = _strip_line_comments(path.read_text(encoding="utf-8"))
        except (OSError, UnicodeError):
            continue
        seen.add(relative)
        directory = path.parent
        stem_dir = directory / path.stem
        candidates: List[Path] = []
        for item in PATH_MOD.findall(text) + INCLUDE.findall(text):
            candidates.extend([directory / item, stem_dir / item])
        for name in PLAIN_MOD.findall(text):
            for base in (directory, stem_dir):
                candidates.extend([base / f"{name}.rs", base / name / "mod.rs"])
        for candidate in candidates:
            resolved = candidate.resolve()
            try:
                nested = resolved.relative_to(root.resolve()).as_posix()
            except ValueError:
                continue
            if resolved.is_file():
                pending.append(nested)
    return seen


def gated_features(root: Path, sources: Iterable[str]) -> Set[str]:
    found: Set[str] = set()
    for relative in sources:
        try:
            found.update(FEATURE.findall((root / relative).read_text(encoding="utf-8")))
        except (OSError, UnicodeError):
            continue
    return found


def top_level_modules(root: Path) -> Dict[str, List[str]]:
    """Map each top-level `src` module to its repository-relative Rust files."""
    modules: Dict[str, List[str]] = {}
    for path in sorted((root / "src").rglob("*.rs")):
        relative = path.relative_to(root).as_posix()
        first = relative.split("/")[1]
        name = first[:-3] if first.endswith(".rs") else first
        modules.setdefault(name, []).append(relative)
    return modules


def embedded_files(root: Path) -> Dict[str, Set[str]]:
    """Map each file the product embeds (include_str!/include_bytes!) to the
    `src` files that embed it, so a change to it selects their covers."""
    embedded: Dict[str, Set[str]] = {}
    resolved_root = root.resolve()
    for path in sorted((root / "src").rglob("*.rs")):
        try:
            text = _strip_line_comments(path.read_text(encoding="utf-8"))
        except (OSError, UnicodeError):
            continue
        source = path.relative_to(root).as_posix()
        for item in EMBED.findall(text):
            try:
                target = (path.parent / item).resolve().relative_to(resolved_root).as_posix()
            except ValueError:
                continue
            embedded.setdefault(target, set()).add(source)
    return embedded


def load(root: Path) -> Tuple[dict, Dict[str, Tuple[str, List[str]]]]:
    mapping = json.loads((root / MAP_PATH).read_text(encoding="utf-8"))
    manifest = (root / "Cargo.toml").read_text(encoding="utf-8")
    return mapping, manifest_targets(manifest)


def declaration_errors(root: Path, mapping: dict, manifest: Dict[str, Tuple[str, List[str]]]) -> List[str]:
    """Check each target's declared features and covers against repository facts."""
    errors: List[str] = []
    declared = mapping.get("targets")
    if not isinstance(declared, dict):
        return ["targets must be an object keyed by Cargo test target"]
    if set(declared) != set(manifest):
        missing = sorted(set(manifest) - set(declared))
        extra = sorted(set(declared) - set(manifest))
        errors.append(f"targets differ from Cargo test targets: missing {missing}, unknown {extra}")
    input_files = [p.relative_to(root).as_posix() for directory in ("src", "tests")
                   for p in (root / directory).rglob("*") if p.is_file()]
    repository_only = set(mapping.get("repositoryOnlyTargets", []))
    product_covers: List[str] = []
    for name in sorted(set(declared) & set(manifest)):
        item = declared[name]
        if not isinstance(item, dict):
            errors.append(f"target entry must be an object: {name}")
            continue
        features = item.get("features")
        covers = item.get("covers")
        if not isinstance(features, list) or not all(isinstance(f, str) and f for f in features):
            errors.append(f"features must be a list of feature names: {name}")
            continue
        if not isinstance(covers, list) or not covers or not all(isinstance(c, str) for c in covers):
            errors.append(f"covers must be a non-empty list of src/ or tests/ path prefixes: {name}")
            continue
        path, required = manifest[name]
        gated = gated_features(root, target_sources(root, path))
        expected = sorted(set(required) | gated)
        if sorted(features) != expected or len(set(features)) != len(features):
            errors.append(
                f"{name} features {sorted(features)} disagree with required-features {required} "
                f"and feature-gated test code {sorted(gated)}; expected {expected}"
            )
        for cover in covers:
            parts = cover.split("/")
            valid = (cover.startswith(("src/", "tests/")) and "\\" not in cover
                     and all(part not in ("", ".", "..") for part in parts[1:-1])
                     and parts[-1] not in (".", ".."))
            if not valid or not any(f.startswith(cover) for f in input_files):
                errors.append(f"{name} cover is invalid or matches no repository input: {cover}")
        if name not in repository_only:
            product_covers.extend(covers)
    for module, files in top_level_modules(root).items():
        if not any(f.startswith(c) for c in product_covers for f in files):
            errors.append(f"top-level src module is covered by no product test target: {module}")
    return errors


def changed_files(root: Path, base: str) -> List[str]:
    def git(*args: str) -> List[str]:
        result = subprocess.run(["git", *args], cwd=root, capture_output=True, text=True)
        if result.returncode != 0:
            raise SystemExit(f"test-affected: git {' '.join(args)} failed: {result.stderr.strip()}")
        return [line for line in result.stdout.splitlines() if line]

    tracked = git("diff", "--name-only", "--no-renames", base, "--")
    untracked = git("ls-files", "--others", "--exclude-standard")
    return sorted(set(tracked) | set(untracked))


def select(root: Path, mapping: dict, manifest: Dict[str, Tuple[str, List[str]]],
           changed: List[str]) -> Tuple[List[str], List[str]]:
    """Return (selected targets, reasons) for the changed repository paths."""
    declared = mapping["targets"]
    repository_only = set(mapping.get("repositoryOnlyTargets", []))
    names = sorted(manifest)
    sources = {name: target_sources(root, manifest[name][0]) for name in names}
    texts: Dict[str, str] = {}
    for name in names:
        texts[name] = "\n".join(
            (root / relative).read_text(encoding="utf-8", errors="replace") for relative in sorted(sources[name])
        )
    embedded = embedded_files(root)
    selected: Set[str] = set()
    reasons: List[str] = []
    for path in changed:
        hits: Set[str]
        if path in SELECT_ALL_FILES or path.startswith(SELECT_ALL_PREFIXES):
            hits = set(names)
            why = "shared build or test support"
        elif path.startswith(("src/", "tests/")):
            covers = {n for n in names if any(path.startswith(c) for c in declared[n]["covers"])}
            owners = {n for n in names if path in sources[n]}
            readers = _mentioning(path, texts)
            hits = covers | owners | readers
            why = "covers, compiled sources and explicit references"
            # A generic repository policy scan cannot establish that a new
            # product or test input has behavioral coverage of its own.
            if not (owners or readers or (covers - repository_only)):
                hits, why = set(names), "unowned input; conservative target selection"
        else:
            hits = _mentioning(path, texts)
            why = "referenced by test source"
            if not hits and not path.endswith((".md", ".png", ".svg", ".jpg", ".gif")):
                hits, why = set(names), "unknown dependency; conservative target selection"
        if path in embedded and not path.startswith("src/"):
            # The binary embeds this file, so the embedding module's covers apply.
            embedding = embedded[path]
            by_embedding = {
                n for n in names
                if any(source.startswith(c) for c in declared[n]["covers"] for source in embedding)
            }
            if by_embedding - hits:
                hits |= by_embedding
                why += f"; embedded by {', '.join(sorted(embedding))}"
        selected |= hits
        reasons.append(f"{path}: {why} -> {', '.join(sorted(hits)) if hits else 'no target'}")
    return sorted(selected), reasons


def _mentioning(path: str, texts: Dict[str, str]) -> Set[str]:
    needles = {path}
    if path.startswith("tests/"):
        needles.add(path[len("tests/"):])
        # A fixture directory a test reads as a whole is named by its path.
        parent = path.rsplit("/", 1)[0]
        if parent not in ("tests", "tests/fixtures"):
            needles.add(parent + "\"")
    return {name for name, text in texts.items() if any(needle in text for needle in needles)}


def pre_return_scope(changed: List[str]) -> Set[str]:
    """Classify changed inputs; ci-stages.sh remains the stage policy owner.

    This relation supplies no second stage registry. Unknown build/check
    inputs keep the broad recipe; Cargo target ownership stays in select().
    """
    kinds: Set[str] = set()
    for path in changed:
        if path.startswith(("src/", "tests/")) and path.endswith(".rs"):
            kinds.add("rust")
        elif path in SELECT_ALL_FILES or path.startswith(("rust-toolchain", ".github/", "scripts/", "tooling/")):
            kinds.add("broad")
        elif path.startswith(("schema/", "skills/", "plugins/", "compatibility/")):
            kinds.add("build")
        elif not path.endswith((".md", ".png", ".svg", ".jpg", ".gif")):
            kinds.add("broad")
    return kinds


def main(argv: List[str]) -> int:
    if len(argv) != 3 or argv[0] not in ("affected", "scope") or argv[1] != "--base":
        print("usage: test_target_sources.py affected|scope --base BASE", file=sys.stderr)
        return 2
    root = Path(__file__).resolve().parents[3]
    changed = changed_files(root, argv[2])
    if argv[0] == "scope":
        print(" ".join(sorted(pre_return_scope(changed))) or "docs")
        return 0
    mapping, manifest = load(root)
    errors = declaration_errors(root, mapping, manifest)
    if errors:
        for error in errors:
            print(f"test-affected: {error}", file=sys.stderr)
        return 1
    selected, reasons = select(root, mapping, manifest, changed)
    for reason in reasons:
        print(f"# {reason}", file=sys.stderr)
    for name in selected:
        print(f"{name}\t{','.join(mapping['targets'][name]['features'])}")
    # The binary's unit tests have no integration-test owner. Unknown/shared
    # dependencies select them too, alongside all integration targets.
    if (selected == sorted(manifest) or any(
            path.startswith("src/") or path in SELECT_ALL_FILES for path in changed)):
        print("bin:exitbind\t")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
