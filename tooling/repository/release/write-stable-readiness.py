#!/usr/bin/env python3
"""Record platform readiness only after every required job succeeded."""
import json
import os
from pathlib import Path

keys = ("CHANNEL_RESULT", "LINUX_RESULT", "MACOS_RESULT", "WSL_RESULT")
if not all(os.environ.get(key) == "success" for key in keys):
    raise SystemExit("stable readiness requires successful channel, Linux, macOS, and WSL jobs")
Path("stable-readiness.json").write_text(json.dumps({
    "source": os.environ["GITHUB_SHA"],
    "ref": os.environ["GITHUB_REF"],
    "runId": os.environ["GITHUB_RUN_ID"],
    "allNativePlatformsAndWsl": True,
    "published": False,
    "mainChanged": False,
}) + "\n", encoding="utf-8")
