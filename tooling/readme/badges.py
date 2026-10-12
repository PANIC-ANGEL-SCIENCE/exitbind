#!/usr/bin/env python3
"""Read public repository status and render three inert README images (stdlib only)."""
import argparse
import datetime
import html
import json
import os
from pathlib import Path
import re
import urllib.request
import urllib.error

REPO = "PANIC-ANGEL-SCIENCE/exitbind"
API = "https://api.github.com/repos/" + REPO


def get(path):
    request = urllib.request.Request(API + path, headers={
        "Accept": "application/vnd.github+json", "User-Agent": "exitbind-readme-badges",
        "Authorization": "Bearer " + os.environ["GH_TOKEN"],
        "X-GitHub-Api-Version": "2022-11-28",
    })
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.load(response)


def ci_status(runs, sha):
    eligible = [r for r in runs if r.get("head_sha") == sha
                and r.get("head_branch") == "main"
                and r.get("event") in ("push", "workflow_dispatch")
                and r.get("path") == ".github/workflows/ci.yml"]
    if not eligible:
        return {"label": "unknown", "run_id": None, "sha": sha}
    run = max(eligible, key=lambda r: (r["id"], r.get("run_attempt", 1)))
    status, conclusion = run.get("status"), run.get("conclusion")
    if status != "completed":
        label = "running" if status == "in_progress" else "pending"
    else:
        label = {"success": "passing", "failure": "failing", "cancelled": "cancelled",
                 "timed_out": "timed out", "action_required": "action req.",
                 "startup_failure": "failed start", "skipped": "skipped",
                 "neutral": "neutral"}.get(conclusion, "unknown")
    return {"label": label, "run_id": run["id"], "sha": sha}


def release_status(releases):
    stable = [r for r in releases if not r.get("draft") and not r.get("prerelease")
              and r.get("published_at")]
    if not stable:
        return {"label": "none", "tag": None}
    latest = max(stable, key=lambda r: (r["published_at"], r["id"]))
    tag = latest["tag_name"]
    return {"label": tag if re.fullmatch(r"v?\d+\.\d+\.\d+", tag) and len(tag) <= 14 else "published",
            "tag": tag, "release_id": latest["id"]}


def license_status(data):
    spdx = data.get("license", {}).get("spdx_id") if data.get("license") else None
    label = spdx if spdx and re.fullmatch(r"[A-Za-z0-9.+-]{1,16}", spdx) and spdx != "NOASSERTION" else "unknown"
    return {"label": label, "spdx": spdx}


def render(template, label, value, description):
    return template.format(**{k: html.escape(v, quote=True) for k, v in {
        "label": label, "value": value, "title": label + ": " + value,
        "description": description}.items()})


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    sha = get("/commits/main")["sha"]
    # Filter by exact current main; never inherit an old green run.
    runs = get("/actions/workflows/ci.yml/runs?branch=main&head_sha=" + sha + "&per_page=100")["workflow_runs"]
    # /releases/latest excludes drafts and prereleases and implements GitHub's latest policy.
    try:
        latest = get("/releases/latest")
        release = release_status([latest])
    except urllib.error.HTTPError as error:
        if error.code != 404:
            raise
        release = release_status([])
    state = {"main_sha": sha, "ci": ci_status(runs, sha), "release": release,
             "license": license_status(get("")),
             "checked_at": datetime.datetime.now(datetime.timezone.utc).isoformat()}
    template = (root / "assets/readme/black-glass.svg.template").read_text()
    output = Path(args.output)
    output.mkdir(parents=True, exist_ok=True)
    for kind, label in (("ci", "CI"), ("release", "Release"), ("license", "License")):
        detail = f"Public GitHub snapshot checked {state['checked_at']}. Main {sha}. GitHub image caching can delay display."
        (output / (kind + ".svg")).write_text(render(template, label, state[kind]["label"], detail))
    (output / "status.json").write_text(json.dumps(state, indent=2) + "\n")
    # Avoid publishing a status for a main that changed while data was collected.
    if get("/commits/main")["sha"] != sha:
        raise SystemExit("main changed during status collection; nothing should be published")


if __name__ == "__main__":
    main()
