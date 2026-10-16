#!/usr/bin/env python3
"""Compatibility entrypoint for repository README administration."""
from pathlib import Path
import runpy
import sys
target = Path(__file__).resolve().parents[2] / "tooling/repository/ci/readme/badges.py"
sys.argv[0] = str(target)
runpy.run_path(str(target), run_name="__main__")
