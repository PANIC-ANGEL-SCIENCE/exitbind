#!/usr/bin/env python3
"""Fail closed: only a nonempty README.md-only diff uses the existing README gates."""
import os
import re
import subprocess


def readme_only(paths):
    return bool(paths) and all(p == "README.md" for p in paths)


def scope(base, head):
    if not re.fullmatch(r"[0-9a-f]{40}", base or "") or set(base) == {"0"}:
        return "full"
    try:
        names = subprocess.check_output(["git", "diff", "--name-only", "-z", base, head]).decode().split("\0")[:-1]
        mode = subprocess.check_output(["git", "ls-tree", head, "README.md"]).decode()
        return "readme" if readme_only(names) and mode.startswith("100644 blob ") else "full"
    except (subprocess.CalledProcessError, UnicodeError):
        return "full"


if __name__ == "__main__":
    print("scope=" + scope(os.environ.get("BASE_SHA"), os.environ["HEAD_SHA"]))
