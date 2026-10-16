#!/usr/bin/env python3
"""Compatibility entrypoint; implementation is owned by tooling/repository."""
from pathlib import Path
import runpy
import sys

target = Path(__file__).resolve().parent.parent / "tooling/repository/release/paired-prerelease.py"
sys.argv[0] = str(target)
runpy.run_path(str(target), run_name="__main__")
