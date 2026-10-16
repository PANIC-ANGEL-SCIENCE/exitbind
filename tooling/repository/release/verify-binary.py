#!/usr/bin/env python3
"""Compare one exact executable with caller-selected producer and byte identity."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--class", dest="acquisition", choices=["development", "release"], required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--sha256", required=True)
    args = parser.parse_args()
    for value, length in [(args.commit, 40), (args.sha256, 64)]:
        if not re.fullmatch(r"[0-9a-f]{%d}" % length, value):
            parser.error("expected commit/SHA256 must be full lowercase hexadecimal identities")
    binary = args.binary.absolute()
    report = {"executable": str(binary), "acquisitionClass": args.acquisition,
              "expectedCommit": args.commit, "expectedSha256": args.sha256,
              "resolved": False, "authentication": "none: caller must authenticate expected release values"}
    try:
        with binary.open("rb") as source:
            digest = hashlib.file_digest(source, "sha256").hexdigest() if hasattr(hashlib, "file_digest") else hashlib.sha256(source.read()).hexdigest()
        observed = subprocess.run([str(binary), "version", "--json"], capture_output=True, check=True, timeout=10)
        identity = json.loads(observed.stdout)
        if not isinstance(identity, dict):
            raise ValueError("version --json did not return an identity object")
        report.update({"producer": identity, "sha256": digest})
        if identity.get("commit") is None:
            report["reason"] = "producer unresolved: version alone does not identify a build"
        elif identity.get("name") != "exitbind" or identity.get("commit") != args.commit:
            report["reason"] = "producer mismatch"
        elif digest != args.sha256 or identity.get("executableSha256") != digest or identity.get("executableSha256Error") is not None:
            report["reason"] = "executable digest mismatch or unavailable"
        else:
            report.update({"resolved": True, "reason": "producer and local bytes match the selected values"})
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        report["reason"] = "binary identity unavailable: " + str(error)
    print(json.dumps(report, sort_keys=True))
    return 0 if report["resolved"] else 1


if __name__ == "__main__":
    sys.exit(main())
