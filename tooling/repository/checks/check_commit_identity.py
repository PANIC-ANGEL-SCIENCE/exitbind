#!/usr/bin/env python3
"""No-build scan of unpublished commit identity.

The build-dependent authority is
``cargo test --locked --features repository-checks --test privacy_gate``; this
scan applies its metadata rule from the same source
(``tests/privacy_gate.rs`` canonical identities) so a non-canonical author or
committer fails before any build starts. It reads commit metadata only and
prints SHAs, never names or messages.

``--base`` names the last published commit; defaults to ``origin/main``.
``--self-test`` seeds each identity outcome the rule defines.
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Dict, List, Sequence

sys.dont_write_bytecode = True

ROOT = Path(__file__).resolve().parents[3]
RULES = "tests/privacy_gate.rs"
AUTHORITY = "cargo test --locked --features repository-checks --test privacy_gate"
FIELDS = {
    "name": "ALLOWED_NAME",
    "email": "ALLOWED_EMAIL",
    "bot_name": "GITHUB_COMMITTER_NAME",
    "bot_email": "GITHUB_COMMITTER_EMAIL",
}


def identities(source: Path) -> Dict[str, str]:
    text = source.read_text(encoding="utf-8")
    found: Dict[str, str] = {}
    for field, constant in FIELDS.items():
        match = re.search(rf'const {constant}: &str = "([^"]*)"', text)
        if not match:
            raise ValueError(f"cannot read {constant} from {source}")
        found[field] = match.group(1)
    return found


def log(root: Path, base: str) -> str:
    result = subprocess.run(
        [
            "git",
            "log",
            "--use-mailmap",
            "--format=%H%x00%aN%x00%aE%x00%cn%x00%ce",
            f"{base}..HEAD",
        ],
        cwd=root,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    if result.returncode:
        raise RuntimeError(result.stderr.strip() or f"git log failed for {base}..HEAD")
    return result.stdout


def mismatches(text: str, expected: Dict[str, str]) -> List[str]:
    name = expected["name"]
    mail = expected["email"].lower()
    bot_name = expected["bot_name"]
    bot_mail = expected["bot_email"].lower()
    found: List[str] = []
    for line in text.splitlines():
        if not line:
            continue
        commit, author_name, author_email, committer_name, committer_email = line.split("\0")
        author_ok = author_name.strip() == name and author_email.strip().lower() == mail
        committer_ok = (
            committer_name.strip() == name and committer_email.strip().lower() == mail
        ) or (
            committer_name.strip() == bot_name
            and committer_email.strip().lower() == bot_mail
        )
        if not author_ok or not committer_ok:
            found.append(
                commit[:12]
                + (" author" if not author_ok else "")
                + (" committer" if not committer_ok else "")
            )
    return found


def check(root: Path, base: str) -> List[str]:
    if subprocess.run(
        ["git", "rev-parse", "--verify", "-q", f"{base}^{{commit}}"],
        cwd=root,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    ).returncode:
        raise SystemExit(
            f"FAIL: base {base} is unavailable; pass the last published commit SHA "
            "as the argument (an all-zero SHA is a base mistake, not a base)"
        )
    text = log(root, base)
    found = mismatches(text, identities(root / RULES))
    for item in found:
        print(f"identity mismatch: {item}", file=sys.stderr)
    if found:
        print(f"the build-dependent authority is: {AUTHORITY}", file=sys.stderr)
    return found


def self_test() -> int:
    problems: List[str] = []
    expected = {
        "name": "Canonical Committer",
        "email": "canonical@example.invalid",
        "bot_name": "GitHub",
        "bot_email": "noreply@github.com",
    }

    def head(repository: Path) -> str:
        return subprocess.run(
            ["git", "rev-parse", "HEAD"],
            cwd=repository,
            stdout=subprocess.PIPE,
            text=True,
            check=True,
        ).stdout.strip()

    def commit(repository: Path, author: Sequence[str], committer: Sequence[str]) -> None:
        environment = dict(os.environ)
        environment.update(
            {
                "GIT_AUTHOR_NAME": author[0],
                "GIT_AUTHOR_EMAIL": author[1],
                "GIT_COMMITTER_NAME": committer[0],
                "GIT_COMMITTER_EMAIL": committer[1],
                "GIT_CONFIG_NOSYSTEM": "1",
                "GIT_TERMINAL_PROMPT": "0",
            }
        )
        subprocess.run(
            ["git", "commit", "-q", "--allow-empty", "-m", "fixture"],
            cwd=repository,
            env=environment,
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )

    with tempfile.TemporaryDirectory(prefix="commit-identity-") as directory:
        repository = Path(directory)
        subprocess.run(
            ["git", "init", "-q", str(repository)],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        canonical = (expected["name"], expected["email"])
        # The root commit is the bootstrap; every later commit is checked as
        # the only commit of its own base..HEAD range.
        commit(repository, canonical, canonical)
        base = head(repository)
        commit(repository, canonical, canonical)
        if mismatches(log(repository, base), expected):
            problems.append("a canonical author and committer was refused")
        base = head(repository)
        commit(repository, ("Other Author", expected["email"]), canonical)
        if " author" not in " ".join(mismatches(log(repository, base), expected)):
            problems.append("a non-canonical author was accepted")
        base = head(repository)
        commit(repository, canonical, ("Other Committer", expected["email"]))
        if " committer" not in " ".join(mismatches(log(repository, base), expected)):
            problems.append("a non-canonical committer was accepted")
        base = head(repository)
        commit(repository, canonical, (expected["bot_name"], expected["bot_email"]))
        if mismatches(log(repository, base), expected):
            problems.append("the GitHub web-flow committer was refused")
        if mismatches(log(repository, "HEAD"), expected):
            problems.append("an empty range still reported a mismatch")
    for problem in problems:
        print(f"FAIL: {problem}", file=sys.stderr)
    if not problems:
        print(
            "Commit identity self-test passed: canonical and GitHub web-flow accepted; "
            "wrong author and wrong committer refused; empty range clean"
        )
    return 1 if problems else 0


def main(argv: List[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", default="origin/main")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args(argv)
    if args.self_test:
        return self_test()
    try:
        found = check(ROOT, args.base)
    except RuntimeError as error:
        print(f"FAIL: {error}", file=sys.stderr)
        return 2
    if found:
        return 1
    text = log(ROOT, args.base)
    print(f"Commit identity passed: {len(text.splitlines())} unpublished commit(s) from {args.base}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
