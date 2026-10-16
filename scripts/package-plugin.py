#!/usr/bin/env python3
"""Compatibility entrypoint for the repository plugin packager."""
from pathlib import Path
import runpy
import sys

target = Path(__file__).resolve().parents[1] / "tooling/repository/release/package-plugin.py"
sys.argv[0] = str(target)
runpy.run_path(str(target), run_name="__main__")
