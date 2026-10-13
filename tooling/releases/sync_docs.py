#!/usr/bin/env python3
"""Render inventoried current-release references without rewriting history."""
import argparse
import json
from pathlib import Path
import re
import subprocess
import sys
import tomllib

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


def verify_release(release, previous):
    """Only a published stable release with every native asset may advance docs."""
    tag = release.get("tag_name", "")
    if (not STABLE.fullmatch(tag) or release.get("draft") is not False
            or release.get("prerelease") is not False or not release.get("published_at")
            or release.get("html_url") != f"https://github.com/{REPOSITORY}/releases/tag/{tag}"):
        raise ValueError("expected a published canonical stable release")
    if tuple(map(int, tag[1:].split("."))) < tuple(map(int, previous[1:].split("."))):
        raise ValueError("refusing to move published documentation backwards")
    assets = {asset["name"] for asset in release.get("assets", [])
              if asset.get("state") == "uploaded" and asset.get("size", 0) > 0}
    expected = {f"exitbind-{target}{suffix}" for target in TARGETS
                for suffix in ("", ".tar.gz", ".tar.gz.sha256")}
    if not expected <= assets:
        raise ValueError("published stable release is missing native release assets")
    return tag


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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument("--write", action="store_true", help="render from the policy stable tag and Cargo candidate")
    parser.add_argument("--from-github", action="store_true", help="verify GitHub's latest published stable release before rendering")
    args = parser.parse_args()
    if args.from_github and not args.write:
        parser.error("--from-github requires --write")
    root = args.root.resolve()
    stable = published_tag(root)
    if args.from_github:
        release = json.loads(subprocess.check_output(
            ["gh", "api", f"repos/{REPOSITORY}/releases/latest"], text=True))
        stable = verify_release(release, stable)
    changes = plan(root, stable)
    if changes and not args.write:
        raise ValueError("stale documentation versions: " + ", ".join(changes)
                         + "; run python3 tooling/releases/sync_docs.py --write")
    for name, content in changes.items():
        (root / name).write_text(content, encoding="utf-8", newline="")
    print(f"documentation versions: {len(changes)} files updated; stable={stable}")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, subprocess.CalledProcessError, KeyError) as error:
        print(f"documentation versions: {error}", file=sys.stderr)
        sys.exit(1)
