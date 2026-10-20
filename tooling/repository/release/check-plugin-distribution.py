#!/usr/bin/env python3
"""Check the self-contained Exitbind skills-only distribution."""

from __future__ import annotations

import argparse
import hashlib
import json
import tarfile
from pathlib import Path, PurePosixPath
from typing import Any


VERSION = "0.31.19"
NAME = "exitbind"
REPOSITORY = "https://github.com/PANIC-ANGEL-SCIENCE/exitbind"
AUTHOR_URL = "https://github.com/envy404"
DESCRIPTION = (
    "Exitbind — No result exits unbound. Bind the evidence to the exact result "
    "before exit. For your existing Codex or Claude lead; this skills-only "
    "plugin installs guidance, not the separate Exitbind CLI."
)
SHORT_DESCRIPTION = "Evidence-bound acceptance for agent work"
DEFAULT_PROMPT = "Use Exitbind for this material task; keep small reversible work direct."
CANONICAL_SKILL = "skills/exitbind/SKILL.md"
EXPECTED_FILES = {
    "plugin.json",
    ".codex-plugin/plugin.json",
    ".claude-plugin/plugin.json",
    "skills/exitbind/SKILL.md",
    "skills/exitbind/references/preservation.md",
    "skills/exitbind/references/operators.md",
}
FORBIDDEN_COMPONENTS = {
    "hooks",
    "apps",
    "mcp",
    "commands",
    "agents",
    "assets",
    "coffee",
    "soulmate",
    "secrets",
}
FORBIDDEN_MANIFEST_KEYS = {
    "hooks",
    "apps",
    "mcpServers",
    "commands",
    "agents",
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--repo-root",
        type=Path,
        default=Path(__file__).resolve().parents[3],
    )
    parser.add_argument(
        "--archive",
        type=Path,
        help="also inspect an uploadable archive against the package tree",
    )
    return parser.parse_args()


def fail(errors: list[str], message: str) -> None:
    errors.append(message)


def read_json(path: Path, errors: list[str]) -> dict[str, Any] | None:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        fail(errors, f"{path}: invalid JSON ({error})")
        return None
    if not isinstance(value, dict):
        fail(errors, f"{path}: manifest must be a JSON object")
        return None
    return value


def exact(value: Any, expected: Any, label: str, errors: list[str]) -> None:
    if value != expected:
        fail(errors, f"{label}: expected {expected!r}, got {value!r}")


def common_manifest(manifest: dict[str, Any], label: str, errors: list[str]) -> None:
    for key in FORBIDDEN_MANIFEST_KEYS:
        if key in manifest:
            fail(errors, f"{label}: forbidden manifest component {key!r}")
    exact(manifest.get("name"), NAME, f"{label}.name", errors)
    exact(manifest.get("version"), VERSION, f"{label}.version", errors)
    exact(manifest.get("description"), DESCRIPTION, f"{label}.description", errors)
    exact(manifest.get("repository"), REPOSITORY, f"{label}.repository", errors)
    author = manifest.get("author")
    if not isinstance(author, dict):
        fail(errors, f"{label}.author: expected object")
    else:
        exact(author.get("name"), "envy404", f"{label}.author.name", errors)
        exact(author.get("url"), AUTHOR_URL, f"{label}.author.url", errors)
    exact(manifest.get("license"), "MIT", f"{label}.license", errors)
    exact(
        manifest.get("keywords"),
        ["agents", "acceptance", "evidence", "receipts"],
        f"{label}.keywords",
        errors,
    )


def interface(errors: list[str]) -> dict[str, Any]:
    return {
        "displayName": NAME.capitalize(),
        "shortDescription": SHORT_DESCRIPTION,
        "longDescription": DESCRIPTION,
        "developerName": "envy404",
        "category": "Productivity",
        "capabilities": ["Read"],
        "defaultPrompt": [DEFAULT_PROMPT],
    }


def check_manifests(package: Path, errors: list[str]) -> None:
    root = read_json(package / "plugin.json", errors)
    codex = read_json(package / ".codex-plugin/plugin.json", errors)
    claude = read_json(package / ".claude-plugin/plugin.json", errors)
    if root is not None:
        common_manifest(root, "plugin.json", errors)
        exact(
            set(root),
            {
                "$schema",
                "name",
                "version",
                "description",
                "author",
                "repository",
                "license",
                "keywords",
                "extensions",
            },
            "plugin.json fields",
            errors,
        )
        exact(
            root.get("$schema"),
            "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
            "plugin.json.$schema",
            errors,
        )
        exact(
            root.get("extensions"),
            {"com.openai": {"interface": interface(errors)}},
            "plugin.json.extensions",
            errors,
        )
    if codex is not None:
        common_manifest(codex, ".codex-plugin/plugin.json", errors)
        exact(
            set(codex),
            {
                "name",
                "version",
                "description",
                "author",
                "repository",
                "license",
                "keywords",
                "skills",
                "interface",
            },
            ".codex-plugin/plugin.json fields",
            errors,
        )
        exact(codex.get("skills"), "./skills/", ".codex-plugin/plugin.json.skills", errors)
        exact(codex.get("interface"), interface(errors), ".codex-plugin/plugin.json.interface", errors)
    if claude is not None:
        common_manifest(claude, ".claude-plugin/plugin.json", errors)
        exact(
            set(claude),
            {
                "name",
                "version",
                "description",
                "author",
                "repository",
                "license",
                "keywords",
                "skills",
            },
            ".claude-plugin/plugin.json fields",
            errors,
        )
        exact(claude.get("skills"), "./skills/", ".claude-plugin/plugin.json.skills", errors)


def package_files(package: Path, errors: list[str]) -> set[str]:
    found: set[str] = set()
    if not package.is_dir():
        fail(errors, f"missing package directory: {package}")
        return found
    for path in package.rglob("*"):
        relative = path.relative_to(package).as_posix()
        if path.is_symlink():
            fail(errors, f"symlink is not allowed: {relative}")
        if any(part.lower() in FORBIDDEN_COMPONENTS for part in PurePosixPath(relative).parts):
            fail(errors, f"forbidden package path component: {relative}")
        if path.is_file():
            found.add(relative)
    exact(found, EXPECTED_FILES, "package files", errors)
    return found


def check_skill(repo_root: Path, package: Path, errors: list[str]) -> None:
    for relative in [CANONICAL_SKILL, "skills/exitbind/references/preservation.md", "skills/exitbind/references/operators.md"]:
        check_skill_asset(repo_root, package, relative, errors)


def check_skill_asset(repo_root: Path, package: Path, relative: str, errors: list[str]) -> None:
    canonical = repo_root / relative
    bundled = package / relative
    try:
        canonical_bytes = canonical.read_bytes()
        bundled_bytes = bundled.read_bytes()
    except OSError as error:
        fail(errors, f"skill bytes unavailable: {error}")
        return
    if canonical_bytes != bundled_bytes:
        fail(
            errors,
            "bundled skill differs from canonical "
            f"(canonical sha256={hashlib.sha256(canonical_bytes).hexdigest()}, "
            f"bundled sha256={hashlib.sha256(bundled_bytes).hexdigest()})",
        )


def check_catalogs(repo_root: Path, errors: list[str]) -> None:
    codex_path = repo_root / ".agents/plugins/marketplace.json"
    claude_path = repo_root / ".claude-plugin/marketplace.json"
    codex = read_json(codex_path, errors)
    claude = read_json(claude_path, errors)
    expected_codex = {
        "name": "exitbind-plugins",
        "interface": {"displayName": "envy404"},
        "plugins": [
            {
                "name": NAME,
                "source": {"source": "local", "path": "./plugins/exitbind"},
                "policy": {"installation": "AVAILABLE", "authentication": "ON_INSTALL"},
                "category": "Productivity",
            }
        ],
    }
    expected_claude = {
        "name": "exitbind-plugins",
        "owner": {"name": "envy404", "url": AUTHOR_URL},
        "plugins": [{"name": NAME, "source": "./plugins/exitbind"}],
    }
    if codex is not None:
        exact(codex, expected_codex, str(codex_path), errors)
    if claude is not None:
        exact(claude, expected_claude, str(claude_path), errors)


def check_archive(package: Path, archive: Path, errors: list[str]) -> None:
    # Directories are the prefixes the expected files imply; adding a file in a
    # new directory must not need a second list to be edited by hand.
    directories = {"exitbind"}
    for path in EXPECTED_FILES:
        parts = path.split("/")
        directories.update(
            "exitbind/" + "/".join(parts[:index]) for index in range(1, len(parts))
        )
    expected = directories | {"exitbind/" + path for path in EXPECTED_FILES}
    try:
        with tarfile.open(archive, mode="r:gz") as tar:
            members = tar.getmembers()
            names = {member.name for member in members}
            exact(names, expected, "archive files", errors)
            for member in members:
                if member.name not in expected:
                    fail(errors, f"unexpected archive member: {member.name}")
                    continue
                relative = member.name.removeprefix("exitbind/")
                if member.name in directories:
                    if not member.isdir():
                        fail(errors, f"archive directory is not a directory: {member.name}")
                    continue
                if not member.isfile():
                    fail(errors, f"archive member is not a regular file: {member.name}")
                    continue
                if tar.extractfile(member).read() != (package / relative).read_bytes():
                    fail(errors, f"archive content differs from package: {member.name}")
    except (OSError, tarfile.TarError) as error:
        fail(errors, f"invalid archive {archive}: {error}")


def main() -> int:
    args = parse_args()
    repo_root = args.repo_root.resolve()
    package = repo_root / "plugins/exitbind"
    errors: list[str] = []
    package_files(package, errors)
    check_manifests(package, errors)
    check_skill(repo_root, package, errors)
    check_catalogs(repo_root, errors)
    if args.archive is not None:
        check_archive(package, args.archive.resolve(), errors)
    if errors:
        print("Plugin distribution check failed:")
        for error in errors:
            print(f"- {error}")
        return 1
    print(f"Plugin distribution check passed: {package}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
