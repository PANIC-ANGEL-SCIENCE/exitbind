#!/usr/bin/env python3
"""Validate an offline readback of one selected stable GitHub release."""

import argparse
import json
import re
import sys
from pathlib import Path


TARGETS = (
    "x86_64-unknown-linux-gnu",
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
)
VERSION = re.compile(
    r"v(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\Z"
)
COMMIT = re.compile(r"[0-9a-f]{40}\Z")
SHA256 = re.compile(r"sha256:[0-9a-f]{64}\Z")
# GitHub's latest stable release moves only when a stable release is published,
# so this command never names an unpublished version.
LATEST_INSTALL_COMMAND = (
    "curl -fsSL https://github.com/PANIC-ANGEL-SCIENCE/exitbind/releases/latest/download/install.sh | sh"
)


def fail(message, *, unavailable=False):
    category = "verification unavailable" if unavailable else "installation pointer"
    print(f"{category}: {message}", file=sys.stderr)
    return 2 if unavailable else 1


def expected_assets():
    names = []
    for target in TARGETS:
        archive = f"exitbind-{target}.tar.gz"
        names.extend((f"exitbind-{target}", archive, f"{archive}.sha256"))
    names.extend(("install.sh", "install.sh.sha256"))
    return set(names)


def verify_source_pointer(root, expected_version):
    try:
        policy = (root / "docs/release-version-policy.md").read_text(encoding="utf-8")
        readme = (root / "README.md").read_text(encoding="utf-8")
    except (OSError, UnicodeError) as error:
        return fail(f"cannot read project installation pointers: {error}", unavailable=True)

    policy_tags = [
        line.removeprefix("Source candidate tag: ")
        for line in policy.splitlines()
        if line.startswith("Source candidate tag: ")
    ]
    if len(policy_tags) != 1 or not VERSION.fullmatch(policy_tags[0]):
        return fail("release policy must declare one valid source candidate tag", unavailable=True)
    if policy_tags[0] != expected_version:
        return fail("release policy source candidate tag does not match the selected version")

    visible_readme = re.sub(r"<!--.*?(?:-->|\Z)", "", readme, flags=re.DOTALL)
    commands = [line for line in visible_readme.splitlines() if line.startswith("curl -fsSL ")]
    if len(commands) != 1:
        return fail("README must contain one visible canonical stable install command", unavailable=True)
    if commands[0] != LATEST_INSTALL_COMMAND:
        return fail("README visible install command is not the latest-release command")
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--metadata", required=True, type=Path, help="offline JSON release and tag readback"
    )
    parser.add_argument(
        "--root", type=Path, default=Path.cwd(),
        help="project root containing README.md and release policy",
    )
    parser.add_argument("--expected-version", required=True,
                        help="exact selected stable tag")
    parser.add_argument(
        "--expected-producer",
        required=True,
        help="expected peeled tag commit (40 lowercase hex digits)",
    )
    args = parser.parse_args()

    if not VERSION.fullmatch(args.expected_version):
        return fail("expected version must be an exact stable vMAJOR.MINOR.PATCH tag")
    if not COMMIT.fullmatch(args.expected_producer):
        return fail("expected producer must be a full lowercase commit SHA")
    try:
        metadata = json.loads(args.metadata.read_text(encoding="utf-8"))
    except (OSError, UnicodeError) as error:
        return fail(f"cannot read release metadata: {error}", unavailable=True)
    except json.JSONDecodeError as error:
        return fail(f"release metadata cannot be parsed: {error}", unavailable=True)
    if not isinstance(metadata, dict):
        return fail("release metadata must be a JSON object")

    if metadata.get("tag_name") != args.expected_version:
        return fail("release tag does not match the exact selected version")
    if metadata.get("draft") is not False or metadata.get("prerelease") is not False:
        return fail("selected release is not a published stable release")
    if (
        not isinstance(metadata.get("published_at"), str)
        or not metadata["published_at"].strip()
    ):
        return fail("selected release has no published timestamp")
    if metadata.get("producer_commit") != args.expected_producer:
        return fail("peeled tag producer does not match the expected commit")

    assets = metadata.get("assets")
    if not isinstance(assets, list):
        return fail("release assets must be a JSON array")
    observed = {}
    for asset in assets:
        if not isinstance(asset, dict):
            return fail("release asset entries must be JSON objects")
        name, digest = asset.get("name"), asset.get("digest")
        if not isinstance(name, str) or not isinstance(digest, str):
            return fail("every release asset needs a name and SHA-256 digest")
        if name in observed:
            return fail(f"duplicate release asset: {name}")
        if not SHA256.fullmatch(digest):
            return fail(f"release asset has no valid SHA-256 digest: {name}")
        observed[name] = digest

    expected = expected_assets()
    missing = sorted(expected - observed.keys())
    extra = sorted(observed.keys() - expected)
    if missing or extra:
        problems = []
        if missing:
            problems.append("missing: " + ", ".join(missing))
        if extra:
            problems.append("unexpected: " + ", ".join(extra))
        return fail(
            "release asset inventory differs from the delivery set ("
            + "; ".join(problems)
            + ")"
        )

    pointer_status = verify_source_pointer(args.root, args.expected_version)
    if pointer_status:
        return pointer_status

    print(json.dumps({
        "result": "selected_published_stable_validated",
        "tag": args.expected_version,
        "producerCommit": args.expected_producer,
        "assetCount": len(observed),
    }, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
