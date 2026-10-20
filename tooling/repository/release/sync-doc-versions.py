#!/usr/bin/env python3
"""Keep every inventoried version mention equal to the Cargo package version.

The inventory (doc-versions.json) lists, per file, `current` lines that follow
the package version and `pinned` lines that keep a historical version for a
stated reason. Every tracked Markdown document except the listed archives is
scanned: a version mention on a line that is not inventoried is an error, so a
new mention cannot silently go stale. `--apply` rewrites the `current` lines;
`--check` (the default) changes nothing and fails when any line is stale.
"""
import argparse
import json
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys

INVENTORY = "tooling/repository/release/doc-versions.json"
PLACEHOLDER = "{version}"
SEMVER = (r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)"
          r"(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?(?:\+[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?")
MENTION = re.compile(r"(?<![\d.])v?[0-9]+\.[0-9]+\.[0-9]+")
APPLY_HINT = "python3 tooling/repository/release/sync-doc-versions.py --apply"


def package_version(root):
    """Read [package] version from Cargo.toml without a TOML dependency."""
    package = False
    versions = []
    for line in safe_file(root, "Cargo.toml").read_text(encoding="utf-8").splitlines():
        if line.startswith("["):
            package = line == "[package]"
        elif package and line.startswith('version = "') and line.endswith('"'):
            versions.append(line[len('version = "'):-1])
    if len(versions) != 1 or not re.fullmatch(SEMVER, versions[0]):
        raise ValueError("Cargo.toml must declare one valid package version")
    return versions[0]


def safe_file(root, name):
    """Resolve an inventoried name to a regular, non-symlinked file inside root."""
    relative = PurePosixPath(name)
    if relative.is_absolute() or ".." in relative.parts or not relative.parts:
        raise ValueError(f"inventory path must stay inside the project: {name}")
    path = root.joinpath(*relative.parts)
    if path.is_symlink() or path.resolve() != root.resolve().joinpath(*relative.parts):
        raise ValueError(f"inventoried file must not be a symbolic link: {name}")
    if not path.is_file():
        raise ValueError(f"inventoried file is missing: {name}")
    return path


def tracked_documents(root):
    try:
        listing = subprocess.run(["git", "-C", str(root), "ls-files", "-z", "--", "*.md"],
                                 capture_output=True, check=True).stdout
    except (OSError, subprocess.CalledProcessError) as error:
        raise ValueError(f"cannot list tracked documents with git: {error}") from error
    return sorted(name for name in listing.decode("utf-8").split("\0") if name)


def load_inventory(root):
    inventory = json.loads(safe_file(root, INVENTORY).read_text(encoding="utf-8"))
    archives, documents, sources = (inventory.get(key, {}) for key in ("archives", "documents", "sources"))
    for name, reason in archives.items():
        if not isinstance(reason, str) or not reason.strip():
            raise ValueError(f"archive needs a reason: {name}")
    for group in (documents, sources):
        for name, rules in group.items():
            for rule in rules:
                validate_rule(name, rule)
    overlap = (set(documents) & set(sources)) | (set(archives) & (set(documents) | set(sources)))
    if overlap:
        raise ValueError(f"file listed twice in the inventory: {sorted(overlap)}")
    return archives, documents, sources


def validate_rule(name, rule):
    if "current" in rule:
        template = rule["current"]
        if not isinstance(template, str) or template.count(PLACEHOLDER) < 1:
            raise ValueError(f"current rule needs {PLACEHOLDER}: {name}: {template!r}")
        if not isinstance(rule.get("count", 1), int) or rule.get("count", 1) < 1:
            raise ValueError(f"current rule count must be positive: {name}: {template!r}")
    elif "pinned" in rule:
        if not isinstance(rule.get("reason"), str) or not rule["reason"].strip():
            raise ValueError(f"pinned mention needs a reason: {name}: {rule['pinned']!r}")
    else:
        raise ValueError(f"inventory rule must be current or pinned: {name}: {rule!r}")


def template_pattern(template):
    return re.compile(re.escape(template).replace(re.escape(PLACEHOLDER), "(?:" + SEMVER + ")"))


def render_file(name, text, rules, version, scan):
    """Return the rewritten text; fail on unlisted mentions or count drift."""
    patterns = [template_pattern(rule["current"]) if "current" in rule else None for rule in rules]
    counts = [0] * len(rules)
    lines = text.splitlines(keepends=True)
    previous = None
    for number, line in enumerate(lines, 1):
        body = line.rstrip("\r\n")
        matches = []
        for index, rule in enumerate(rules):
            if "after" in rule and rule["after"] != previous:
                continue
            if patterns[index] is not None and patterns[index].fullmatch(body):
                matches.append((index, rule["current"].replace(PLACEHOLDER, version)))
            elif rule.get("pinned") == body:
                matches.append((index, body))
        previous = body
        if len(matches) > 1:
            raise ValueError(f"line matches several inventory rules: {name}:{number}")
        if matches:
            index, rendered = matches[0]
            counts[index] += 1
            lines[number - 1] = rendered + line[len(body):]
        elif scan and MENTION.search(body):
            raise ValueError(
                f"unlisted version mention: {name}:{number}: {body.strip()}\n"
                f"  list it in {INVENTORY} as current (follows Cargo.toml) or pinned with a reason")
    for index, rule in enumerate(rules):
        expected = rule.get("count", 1)
        if counts[index] != expected:
            line = rule.get("current", rule.get("pinned"))
            raise ValueError(f"{name}: expected {expected} line(s) {line!r}, found {counts[index]}")
    return "".join(lines)


def plan(root, version=None):
    """Validate the whole inventory and return {name: new text} for stale files."""
    root = Path(root)
    version = version or package_version(root)
    if not re.fullmatch(SEMVER, version):
        raise ValueError(f"invalid package version: {version}")
    archives, documents, sources = load_inventory(root)
    changes = {}
    for name, rules in sorted(sources.items()):
        text = safe_file(root, name).read_text(encoding="utf-8")
        rendered = render_file(name, text, rules, version, scan=False)
        if rendered != text:
            changes[name] = rendered
    tracked = tracked_documents(root)
    missing = sorted(set(documents) - set(tracked))
    if missing:
        raise ValueError(f"inventoried documents are not tracked Markdown: {missing}")
    for name in tracked:
        if name in archives:
            continue
        rules = documents.get(name, [])
        text = safe_file(root, name).read_text(encoding="utf-8")
        rendered = render_file(name, text, rules, version, scan=True)
        if rendered != text:
            changes[name] = rendered
    return changes


def inventory_size(root):
    _, documents, sources = load_inventory(Path(root))
    rules = [rule for group in (documents, sources) for entries in group.values() for rule in entries]
    current = sum(rule.get("count", 1) for rule in rules if "current" in rule)
    pinned = sum(rule.get("count", 1) for rule in rules if "pinned" in rule)
    return current, pinned


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[3])
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--check", action="store_true", help="fail on stale or unlisted mentions (default)")
    mode.add_argument("--apply", action="store_true", help="rewrite current mentions from Cargo.toml")
    args = parser.parse_args()
    root = args.root.resolve()
    version = package_version(root)
    changes = plan(root, version)
    current, pinned = inventory_size(root)
    if args.apply:
        for name, text in changes.items():
            with safe_file(root, name).open("w", encoding="utf-8", newline="") as stream:
                stream.write(text)
        print(f"version mentions: {len(changes)} file(s) rewritten to {version}; "
              f"{current} current, {pinned} pinned")
        return 0
    if changes:
        print(f"version mentions: stale current mentions in {', '.join(sorted(changes))}; "
              f"run {APPLY_HINT}", file=sys.stderr)
        return 1
    print(f"version mentions: all current mentions read {version}; {current} current, {pinned} pinned")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (ValueError, OSError, UnicodeError, json.JSONDecodeError) as error:
        print(f"version mentions: {error}", file=sys.stderr)
        sys.exit(1)
