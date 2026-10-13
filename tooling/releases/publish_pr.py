#!/usr/bin/env python3
"""Publish only generated documentation to one non-force branch and draft PR."""
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile

from sync_docs import INVENTORY, REPOSITORY, compile_template, plan

BRANCH = "automation/stable-docs"
REF = "refs/heads/" + BRANCH
IDENTITY = ["-c", "user.name=envy404", "-c",
            "user.email=287204764+envy404@users.noreply.github.com"]


def run(*args):
    return subprocess.check_output(args, text=True).strip()


def git(*args):
    return run("git", *args)


def blob(revision, name):
    return subprocess.check_output(["git", "show", revision + ":" + name]).decode("utf-8")


def remote_head(ref):
    lines = git("ls-remote", "--refs", "origin", ref).splitlines()
    if not lines:
        return None
    if len(lines) != 1 or not re.fullmatch(r"[0-9a-f]{40}\t" + re.escape(ref), lines[0]):
        raise ValueError("unexpected remote ref response")
    return lines[0].split("\t")[0]


def prs():
    items = json.loads(run("gh", "pr", "list", "--repo", REPOSITORY,
                          "--head", BRANCH, "--state", "open", "--json",
                          "number,baseRefName,headRefName,headRefOid,url"))
    if len(items) > 1:
        raise ValueError("multiple open documentation PRs require review")
    if items and (items[0]["baseRefName"] != "main" or items[0]["headRefName"] != BRANCH):
        raise ValueError("existing documentation PR has another base or head; preserve its review")
    return items


def ancestor(older, newer):
    status = subprocess.run(["git", "merge-base", "--is-ancestor", older, newer]).returncode
    if status not in (0, 1):
        raise ValueError("cannot verify branch ancestry")
    return status == 0


def normalized(text, rules):
    result = []
    for line in text.splitlines(keepends=True):
        body = line.rstrip("\r\n")
        matches = [r["template"] for r in rules if "template" in r
                   and compile_template(r["template"]).fullmatch(body)]
        if len(matches) > 1:
            raise ValueError("ambiguous current-version line")
        result.append((matches[0] if matches else body) + line[len(body):])
    return "".join(result)


def generated_only(base, head, inventory):
    paths = git("diff", "--name-only", "-z", base, head).split("\0")
    for name in filter(None, paths):
        rules = inventory.get(name, [])
        if not any("template" in rule for rule in rules):
            raise ValueError(f"unexpected documentation branch edit: {name}")
        # Modes, renames, symlinks and additions/deletions are not version updates.
        for revision in (base, head):
            if not git("ls-tree", revision, "--", name).startswith("100644 blob "):
                raise ValueError(f"unexpected document mode or missing file: {name}")
        before = blob(base, name)
        after = blob(head, name)
        if normalized(before, rules) != normalized(after, rules):
            raise ValueError(f"non-version documentation edit requires review: {name}")


def publish():
    root = Path.cwd()
    if os.environ.get("GITHUB_REPOSITORY") != REPOSITORY:
        raise ValueError("publication is limited to the canonical repository")
    origin = git("remote", "get-url", "origin")
    if origin not in (f"https://github.com/{REPOSITORY}.git", f"https://github.com/{REPOSITORY}",
                      f"git@github.com:{REPOSITORY}.git"):
        raise ValueError("origin must name the canonical repository")
    if git("diff", "--cached", "--name-only") or git("ls-files", "--others", "--exclude-standard"):
        raise ValueError("publication requires an unstaged documentation-only worktree")
    if plan(root):
        raise ValueError("render and validate documentation before publication")
    base = git("rev-parse", "HEAD")
    if remote_head("refs/heads/main") != base:
        raise ValueError("main advanced; regenerate on current main")
    opened = prs()  # A stacked implementation PR must never be overwritten.
    previous = remote_head(REF)
    if opened and opened[0]["headRefOid"] != previous:
        raise ValueError("PR and branch head disagree; inspect before retrying")
    inventory = json.loads((root / INVENTORY).read_text())["files"]
    if previous:
        git("fetch", "--no-tags", "origin", REF)
        if not ancestor(previous, base):
            common = git("merge-base", previous, base)
            try:
                generated_only(common, previous, inventory)
            except ValueError:
                # GitHub permits squash merging here. A merged implementation
                # head may not be an ancestor, but its changes must already be
                # incorporated in main. Compute a tree only; never run its code.
                merged = subprocess.run(["git", "merge-tree", "--write-tree", base, previous],
                                        text=True, capture_output=True)
                if merged.returncode or merged.stdout.splitlines()[0] != git("rev-parse", base + "^{tree}"):
                    raise
    if not git("diff", "--name-only"):
        print("Documentation already matches main; no remote mutation")
        return
    paths = list(filter(None, git("diff", "--name-only", "-z").split("\0")))
    if any(not any("template" in r for r in inventory.get(name, [])) for name in paths):
        raise ValueError("unmanaged worktree changes require review")
    git("add", "--", *paths)
    tree = git("write-tree")
    # Inspect index bytes without committing or executing any branch code.
    for name in paths:
        if normalized(blob(base, name), inventory[name]) != normalized(
                blob("", name), inventory[name]):
            raise ValueError(f"non-version worktree edit requires review: {name}")
        if not git("ls-files", "--stage", "--", name).startswith("100644 "):
            raise ValueError("unexpected document mode")
    if previous and git("rev-parse", previous + "^{tree}") == tree:
        head = previous
    else:
        parents = [base] if not previous or ancestor(previous, base) else [previous]
        if previous and not ancestor(previous, base) and not ancestor(base, previous):
            parents.append(base)
        args = ["git", *IDENTITY, "commit-tree", tree]
        for parent in parents:
            args.extend(["-p", parent])
        head = run(*args, "-m", "docs: synchronize published stable release references")
        if remote_head("refs/heads/main") != base or remote_head(REF) != previous:
            raise ValueError("remote advanced; no push attempted")
        # A normal fast-forward push preserves concurrent writes; never force.
        git("push", "origin", head + ":" + REF)
    if remote_head(REF) != head:
        raise ValueError("published branch SHA is uncertain; inspect before retrying")
    if remote_head("refs/heads/main") != base:
        raise ValueError("main advanced during publication; branch retained for review")
    if not opened:
        with tempfile.TemporaryDirectory() as directory:
            body = Path(directory) / "body.txt"
            body.write_text("Synchronize current stable documentation from the verified published release. "
                            "Candidate versions and historical references remain separate. "
                            "Generated references and existing release checks passed; review before merging.\n")
            result = subprocess.run(["gh", "pr", "create", "--repo", REPOSITORY,
                                     "--base", "main", "--head", BRANCH, "--draft",
                                     "--title", "Synchronize published stable documentation",
                                     "--body-file", str(body)])
            opened = prs()  # Also resolves an uncertain create response without another write.
            if result.returncode and not opened:
                raise ValueError("PR creation failed; branch retained, no blind retry")
    else:
        opened = prs()
    if len(opened) != 1 or opened[0]["headRefOid"] != head:
        raise ValueError("PR head SHA is uncertain; inspect before retrying")
    print(f"Verified documentation PR {opened[0]['url']} at {head}")


if __name__ == "__main__":
    try:
        publish()
    except (ValueError, OSError, subprocess.CalledProcessError, KeyError) as error:
        print(f"documentation PR: {error}", file=sys.stderr)
        sys.exit(1)
