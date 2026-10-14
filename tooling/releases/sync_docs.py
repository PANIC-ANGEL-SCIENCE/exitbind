#!/usr/bin/env python3
"""Render inventoried current-release references without rewriting history."""
import argparse
import json
from pathlib import Path
import re
import subprocess
import sys
import tomllib
import tempfile

REPOSITORY = "PANIC-ANGEL-SCIENCE/exitbind"
POLICY = "docs/release-version-policy.md"
INVENTORY = "tooling/releases/doc-versions.json"
SEMVER = r"[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?(?:\+[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?"
VERSION = re.compile(r"(?<![\d.])v?" + SEMVER)
STABLE = re.compile(r"v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)")
TARGETS = ("x86_64-unknown-linux-gnu", "aarch64-apple-darwin", "x86_64-apple-darwin")


def published_tag(root):
    tags = re.findall(r"^Published stable tag: (.*)$", (root / POLICY).read_text(), re.M)
    if len(tags) != 1 or not STABLE.fullmatch(tags[0]):
        raise ValueError("release policy must declare one valid published stable tag")
    return tags[0]


def compile_template(template):
    pattern = re.escape(template)
    for variable in ("stable", "candidate"):
        pattern = pattern.replace(re.escape("{" + variable + "}"), "(?:" + SEMVER + ")")
    return re.compile(pattern)


def plan(root, stable=None):
    """Validate the complete inventory before writing even one file."""
    inventory = json.loads((root / INVENTORY).read_text())
    stable = stable or published_tag(root)
    if not STABLE.fullmatch(stable):
        raise ValueError("invalid stable tag")
    candidate = tomllib.loads((root / "Cargo.toml").read_text())["package"]["version"]
    if not re.fullmatch(SEMVER, candidate):
        raise ValueError("invalid source candidate version")
    values = {"stable": stable[1:], "candidate": candidate}
    files = inventory["files"]
    archives = inventory["archive"]
    changes = {}
    seen = set()
    for path in sorted(root.rglob("*")):
        if ".git" in path.relative_to(root).parts or path.suffix.lower() not in (".md", ".mdx", ".rst", ".adoc"):
            continue
        if path.is_symlink():
            raise ValueError(f"documentation symlink requires explicit review: {path}")
        if not path.is_file():
            continue
        name = path.relative_to(root).as_posix()
        if name in archives:
            if not archives[name]:
                raise ValueError(f"archive requires a reason: {name}")
            continue
        seen.add(name)
        rules = files.get(name, [])
        counts = [0] * len(rules)
        result = []
        with path.open(encoding="utf-8", newline="") as document:
            original = document.read()
        for number, line in enumerate(original.splitlines(keepends=True), 1):
            body = line.rstrip("\r\n")
            matching = []
            for index, rule in enumerate(rules):
                if "template" in rule:
                    if compile_template(rule["template"]).fullmatch(body):
                        rendered = rule["template"]
                        for variable, value in values.items():
                            rendered = rendered.replace("{" + variable + "}", value)
                        matching.append((index, rendered))
                elif rule.get("pinned") == body and rule.get("reason"):
                    matching.append((index, body))
            if len(matching) > 1:
                raise ValueError(f"ambiguous version inventory: {name}:{number}")
            if matching:
                index, rendered = matching[0]
                counts[index] += 1
                line = rendered + line[len(body):]
            elif VERSION.search(body):
                raise ValueError(f"unclassified version reference: {name}:{number}: {body}")
            result.append(line)
        for index, rule in enumerate(rules):
            if "template" in rule and counts[index] != rule["count"]:
                raise ValueError(f"missing or duplicated current reference: {name}: {rule['template']}")
        rendered = "".join(result)
        if original != rendered:
            changes[name] = rendered
    if files.keys() - seen:
        raise ValueError(f"missing inventoried documents: {sorted(files.keys() - seen)}")
    return changes


def promote_changelog(root, selected):
    """Change only the exact selected source-candidate heading, once verified."""
    path = root / "CHANGELOG.md"
    if path.is_symlink():
        raise ValueError("changelog symlink requires explicit review")
    with path.open(encoding="utf-8", newline="") as stream:
        original = stream.read()
    pending = "## Unreleased — " + selected[1:] + " source candidate"
    released = "## " + selected[1:]
    lines = original.splitlines(keepends=True)
    pending_count = sum(line.rstrip("\r\n") == pending for line in lines)
    released_count = sum(line.rstrip("\r\n") == released for line in lines)
    if (pending_count, released_count) == (0, 1):
        return {}
    if (pending_count, released_count) != (1, 0):
        raise ValueError("selected release needs one unambiguous changelog heading")
    rendered = "".join(released + line[len(pending):]
                       if line.rstrip("\r\n") == pending else line for line in lines)
    return {"CHANGELOG.md": rendered}


def publication_plan(root, metadata, selected, producer):
    # Validate prospective pointers in isolation: no real files change on failure.
    changes = plan(root, selected)
    with tempfile.TemporaryDirectory(prefix="exitbind-doc-pointer-") as directory:
        staged = Path(directory)
        for name in ("README.md", POLICY):
            target = staged / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(changes[name].encode("utf-8") if name in changes
                               else (root / name).read_bytes())
        subprocess.run([sys.executable, str(root / "scripts/check-installation-pointer.py"),
                        "--root", str(staged), "--metadata", str(metadata.resolve()),
                        "--expected-version", selected, "--expected-producer", producer],
                       check=True)
    changes.update(promote_changelog(root, selected))
    return changes


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument("--write", action="store_true")
    parser.add_argument("--published", action="store_true")
    parser.add_argument("--metadata", type=Path)
    parser.add_argument("--expected-version")
    parser.add_argument("--expected-producer")
    args = parser.parse_args()
    selection = (args.metadata, args.expected_version, args.expected_producer)
    if args.published:
        if not args.write or not all(selection):
            parser.error("--published requires --write, --metadata, --expected-version and --expected-producer")
    elif any(selection):
        parser.error("release selection requires --published")
    root = args.root.resolve()
    stable = args.expected_version if args.published else published_tag(root)
    if not args.published:
        pending = "## Unreleased — " + stable[1:] + " source candidate"
        if pending in (root / "CHANGELOG.md").read_text().splitlines():
            raise ValueError("selected stable still has an Unreleased heading; use verified --published preparation")
    changes = (publication_plan(root, args.metadata, stable, args.expected_producer)
               if args.published else plan(root))
    if changes and not args.write:
        raise ValueError("stale documentation versions: " + ", ".join(changes)
                         + "; run python3 tooling/releases/sync_docs.py --write")
    for name, content in changes.items():
        (root / name).write_text(content, encoding="utf-8", newline="")
    print(f"documentation versions: {len(changes)} files updated; selected stable={stable}")


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as error:
        sys.exit(error.returncode)
    except (ValueError, OSError, KeyError) as error:
        print(f"documentation versions: {error}", file=sys.stderr)
        sys.exit(1)
