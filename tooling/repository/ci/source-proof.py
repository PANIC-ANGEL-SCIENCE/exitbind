#!/usr/bin/env python3
"""Read provider-owned run evidence; missing or incompatible proof never skips checks."""
import base64
import datetime
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import urllib.parse
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[3]
SHA = re.compile(r"[0-9a-f]{40}\Z")
HEX = re.compile(r"sha256:[0-9a-f]{64}\Z")
MAX_ARCHIVE = 1024 * 1024
CI_COVERAGE = ("source", "release-build", "onboarding", "package", "installer")
NATIVE_COVERAGE = ("source",)
RECIPE = (
    ".github/workflows/ci.yml", ".github/workflows/paired-prerelease.yml",
    ".github/workflows/release.yml", "tooling/repository/ci/source-proof.py",
    "tooling/repository/ci/ci-local.sh", "tooling/repository/ci/ci-stages.sh",
    "tooling/repository/ci/verification-context.sh", "tooling/repository/ci/check-repository-rules.sh", "rust-toolchain.toml",
    "tooling/repository/release/paired-prerelease.py", "Cargo.toml", "Cargo.lock",
)
ENVIRONMENT = (
    "RUNNER_OS", "RUNNER_ARCH", "ImageOS", "ImageVersion", "GITHUB_WORKSPACE",
    "RUNNER_TEMP", "CARGO_TARGET_DIR", "TMPDIR", "CARGO_BUILD_JOBS",
    "CARGO_PROFILE_DEV_OPT_LEVEL", "CARGO_PROFILE_TEST_OPT_LEVEL",
    "CARGO_PROFILE_DEV_DEBUG", "CARGO_PROFILE_TEST_DEBUG", "CARGO_INCREMENTAL",
    "CARGO_BUILD_TARGET", "RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTC_WRAPPER",
    "RUSTC_WORKSPACE_WRAPPER", "EXITBIND_FETCH_VALIDATOR", "CARGO", "CARGO_HOME",
    "RUSTC", "RUSTUP_HOME", "RUSTUP_TOOLCHAIN", "RUSTC_BOOTSTRAP",
    "RUST_TEST_THREADS", "SOURCE_DATE_EPOCH", "HOME", "PATH", "CI",
)
REQUIRED_ENVIRONMENT = (
    "RUNNER_OS", "RUNNER_ARCH", "ImageOS", "ImageVersion", "GITHUB_WORKSPACE",
    "RUNNER_TEMP", "CARGO_TARGET_DIR", "TMPDIR", "CARGO_BUILD_JOBS",
)


def command(*args):
    return subprocess.run(args, cwd=ROOT, check=True, stdout=subprocess.PIPE,
                          stderr=subprocess.PIPE, text=True, timeout=20).stdout.strip()


def context():
    env = os.environ
    repository_id = int(env["GITHUB_REPOSITORY_ID"])
    if env["GITHUB_EVENT_NAME"] == "pull_request":
        event = json.loads(Path(env["GITHUB_EVENT_PATH"]).read_text(encoding="utf-8"))
        if event["pull_request"]["head"]["repo"]["id"] != repository_id:
            raise ValueError("fork pull request cannot consume proof")
    sha = command("git", "rev-parse", "HEAD")
    expected = env.get("EXITBIND_EXPECTED_SHA", env.get("GITHUB_SHA", ""))
    if not SHA.fullmatch(sha) or sha != expected:
        raise ValueError("checkout is not the exact selected source")
    if command("git", "status", "--porcelain", "--untracked-files=all"):
        raise ValueError("source checkout is not clean")
    values = {name: env.get(name, "") for name in ENVIRONMENT}
    # These compiler profile inputs can change test semantics or package builds.
    # Keep the list explicit: registry credentials are not proof metadata.
    for profile in ("DEV", "TEST", "RELEASE"):
        for option in ("OPT_LEVEL", "DEBUG", "DEBUG_ASSERTIONS", "OVERFLOW_CHECKS",
                       "LTO", "PANIC", "INCREMENTAL", "CODEGEN_UNITS", "RPATH",
                       "STRIP", "SPLIT_DEBUGINFO"):
            name = f"CARGO_PROFILE_{profile}_{option}"
            values[name] = env.get(name, "")
    if any(not values[name] for name in REQUIRED_ENVIRONMENT):
        raise ValueError("runner environment identity is unavailable")
    if (values["RUNNER_OS"], values["RUNNER_ARCH"]) not in (
            ("Linux", "X64"), ("macOS", "ARM64"), ("macOS", "X64")):
        raise ValueError("unsupported native proof platform")
    base = env.get("VALUE_PROOF_BASE", "")
    if not base or base == "0" * 40:
        base = command("git", "rev-parse", "HEAD^")
    elif SHA.fullmatch(base):
        base = command("git", "rev-parse", "--verify", base + "^{commit}")
    else:
        base = command("git", "merge-base", "HEAD", base)
    if not SHA.fullmatch(base):
        raise ValueError("proof base is unavailable")
    history = sorted(set(command("git", "rev-list", "--all", "HEAD").splitlines()))
    if not history or any(not SHA.fullmatch(commit) for commit in history):
        raise ValueError("reachable history is unavailable")
    recipe = hashlib.sha256()
    for name in RECIPE:
        recipe.update(name.encode() + b"\0" + (ROOT / name).read_bytes() + b"\0")
    return {
        "source": sha, "base": base, "event": env["GITHUB_EVENT_NAME"],
        "history": hashlib.sha256("\n".join(history).encode()).hexdigest(),
        "eventSha": env["GITHUB_SHA"], "repositoryId": repository_id,
        "runId": int(env["GITHUB_RUN_ID"]), "attempt": int(env["GITHUB_RUN_ATTEMPT"]),
        "environment": values, "recipe": recipe.hexdigest(),
        "rustc": command("rustc", "-vV"), "cargo": command("cargo", "--version"),
    }


def artifact_name(kind, current):
    env = current["environment"]
    return (f"{kind}-{current['source']}-{current['runId']}-{current['attempt']}-"
            f"{env['RUNNER_OS']}-{env['RUNNER_ARCH']}")


def write(path, kind):
    if kind not in ("ci", "native"):
        raise ValueError("unknown executed proof kind")
    current = context()
    coverage = CI_COVERAGE if kind == "ci" else NATIVE_COVERAGE
    current.update(schema="exitbind.source-proof.v1", origin="executed", kind=kind, coverage=list(coverage))
    Path(path).write_text(json.dumps(current, sort_keys=True) + "\n", encoding="utf-8")
    return artifact_name("source-proof", current)


class NoCredentialRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        redirected = super().redirect_request(req, fp, code, msg, headers, newurl)
        if redirected is not None:
            redirected.remove_header("Authorization")
            redirected.remove_unredirected_header("Authorization")
        return redirected


class Client:
    def __init__(self):
        self.repo = os.environ["GITHUB_REPOSITORY"]
        if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", self.repo):
            raise ValueError("invalid repository")
        self.token = os.environ["GH_TOKEN"]
        self.opener = urllib.request.build_opener(NoCredentialRedirect())

    def bytes(self, route, limit=4 * 1024 * 1024):
        url = "https://api.github.com/repos/" + self.repo + "/" + route
        request = urllib.request.Request(url, headers={
            "Authorization": "Bearer " + self.token, "Accept": "application/vnd.github+json",
            "X-GitHub-Api-Version": "2022-11-28",
        })
        with self.opener.open(request, timeout=15) as response:
            data = response.read(limit + 1)
        if len(data) > limit:
            raise ValueError("provider response exceeds bound")
        return data

    def get(self, route):
        return json.loads(self.bytes(route))


def same_workflow(client, run, path):
    ref = urllib.parse.quote(run["head_sha"], safe="")
    remote = client.get(f"contents/{path}?ref={ref}")
    return base64.b64decode(remote["content"], validate=False) == (ROOT / path).read_bytes()


def workflow_path(run):
    path, separator, ref = run.get("path", "").partition("@")
    if separator and ref not in (run.get("head_branch"), "refs/heads/" + str(run.get("head_branch")), run.get("head_sha")):
        return None
    return path


def latest_runs(client, workflow, current, indirect=False):
    if workflow == "paired-prerelease.yml" and indirect:
        runs = paired_source_runs(client, current)
    else:
        runs = client.get(f"actions/workflows/{workflow}/runs?per_page=100")["workflow_runs"]
    candidates = []
    for run in runs:
        if (run.get("id") == current["runId"] or workflow_path(run) != f".github/workflows/{workflow}"
                or run.get("repository", {}).get("id") != current["repositoryId"]
                or (run.get("head_repository") or {}).get("id") != current["repositoryId"]):
            continue
        if not indirect and run.get("head_sha") != current["source"]:
            continue
        if workflow == "paired-prerelease.yml" and run.get("event") not in ("workflow_run", "workflow_dispatch"):
            continue
        if workflow == "ci.yml" and run.get("event") not in ("push", "pull_request", "workflow_dispatch"):
            continue
        if workflow == "release.yml" and (run.get("event") != "workflow_dispatch"
                or not str(run.get("head_branch", "")).startswith("integration/pair-")):
            continue
        candidates.append(run)
    if not candidates:
        return []
    # A later reusable run is not an executed origin, but may follow one.
    # A failed or pending latest run always blocks reuse of every older proof.
    candidates.sort(key=lambda item: (item["run_number"], item["run_attempt"]), reverse=True)
    latest = candidates[0]
    if latest.get("status") != "completed" or latest.get("conclusion") != "success":
        raise ValueError("latest relevant run is not successful")
    if workflow != "ci.yml":
        return [latest]
    latest_jobs = jobs(client, latest)
    executed = succeeded(latest_jobs, "exit", ("Run shared native CI checks and packaging",))
    reused = succeeded(latest_jobs, "exit", ("Find compatible executed source proof",
                       "Recheck changed source context")) and (
        succeeded(latest_jobs, "exit", ("Reuse completed CI qualification",)) or
        succeeded(latest_jobs, "exit", ("Rebuild and verify the current package",)))
    if not (executed or reused):
        raise ValueError("latest source job did not execute or validate reuse")
    return [run for run in candidates if run.get("status") == "completed" and run.get("conclusion") == "success"]


def jobs(client, run):
    fresh = client.get(f"actions/runs/{run['id']}")
    fields = ("id", "path", "head_sha", "head_branch", "event", "run_number", "run_attempt")
    if (fresh.get("status") != "completed" or fresh.get("conclusion") != "success"
            or any(fresh.get(field) != run.get(field) for field in fields)
            or any((fresh.get(field) or {}).get("id") != (run.get(field) or {}).get("id")
                   for field in ("repository", "head_repository"))):
        raise ValueError("run or attempt changed")
    value = client.get(f"actions/runs/{run['id']}/attempts/{run['run_attempt']}/jobs?per_page=100")
    if value.get("total_count", len(value["jobs"])) != len(value["jobs"]):
        raise ValueError("job listing is incomplete")
    return value["jobs"]


def succeeded(items, name, steps=()):
    matches = [item for item in items if item.get("name") == name]
    if len(matches) != 1 or matches[0].get("status") != "completed" or matches[0].get("conclusion") != "success":
        return False
    recorded = matches[0].get("steps", [])
    return all(sum(step.get("name") == name and step.get("status") == "completed"
                   and step.get("conclusion") == "success"
                   for step in recorded) == 1 for name in steps)


def artifact(client, run, name, filename, repository_id):
    value = client.get(f"actions/runs/{run['id']}/artifacts?per_page=100")
    listing = value["artifacts"]
    if value.get("total_count", len(listing)) != len(listing):
        raise ValueError("artifact listing is incomplete")
    matches = [item for item in listing if item.get("name") == name]
    if len(matches) != 1:
        return None
    item = matches[0]
    owner = item.get("workflow_run") or {}
    if (item.get("expired") is not False or not 0 < item.get("size_in_bytes", 0) <= MAX_ARCHIVE
            or not HEX.fullmatch(item.get("digest") or "") or owner.get("id") != run["id"]
            or owner.get("repository_id") != repository_id or owner.get("head_repository_id") != repository_id
            or owner.get("head_sha") != run["head_sha"] or owner.get("head_branch") != run["head_branch"]):
        return None
    data = client.bytes(f"actions/artifacts/{int(item['id'])}/zip", MAX_ARCHIVE)
    if "sha256:" + hashlib.sha256(data).hexdigest() != item["digest"]:
        return None
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        files = archive.infolist()
        if len(files) != 1 or files[0].filename != filename or files[0].file_size > 65536:
            return None
        return json.loads(archive.read(files[0]))


def proof_from_run(client, run, current, kind):
    workflow = workflow_path(run).split("/")[-1]
    paths = [".github/workflows/ci.yml"] if kind == "ci" else [".github/workflows/release.yml"]
    if workflow == "paired-prerelease.yml":
        paths.append(".github/workflows/paired-prerelease.yml")
    if not all(same_workflow(client, run, path) for path in paths):
        return None
    job = ("check / exit" if workflow == "paired-prerelease.yml" else "exit") if kind == "ci" else (
        "linux" if current["environment"]["RUNNER_OS"] == "Linux" else
        "macos-aarch64-apple-darwin" if current["environment"]["RUNNER_ARCH"] == "ARM64" else
        "macos-x86_64-apple-darwin")
    step = "Run shared native CI checks and packaging" if kind == "ci" else "Run shared native CI checks"
    required = [step, "Write executed source proof", "Upload executed source proof"]
    if kind == "ci" or current["environment"]["RUNNER_OS"] == "Linux":
        required += ["Test paired prerelease controls without network writes", "Test source proof controls without network writes"]
    if kind == "ci":
        required += ["Test README tooling", "Verify Cargo package excludes README presentation tooling"]
    if not succeeded(jobs(client, run), job, required):
        return None
    identity = dict(current, runId=run["id"], attempt=run["run_attempt"])
    proof = artifact(client, run, artifact_name("source-proof", identity), "source-proof.json", current["repositoryId"])
    if not isinstance(proof, dict):
        return None
    if (proof.get("schema") != "exitbind.source-proof.v1" or proof.get("origin") != "executed"
            or proof.get("kind") != kind or proof.get("source") != current["source"]
            or proof.get("runId") != run["id"] or proof.get("attempt") != run["run_attempt"]
            or proof.get("event") != run["event"] or proof.get("eventSha") != run["head_sha"]
            or proof.get("repositoryId") != current["repositoryId"]):
        return None
    coverage = CI_COVERAGE if kind == "ci" else NATIVE_COVERAGE
    if proof.get("coverage") != list(coverage):
        return None
    if (not isinstance(proof.get("event"), str) or not SHA.fullmatch(proof.get("base", ""))
            or not re.fullmatch(r"[0-9a-f]{64}", proof.get("history", ""))):
        return None
    if any(proof.get(key) != current[key] for key in ("environment", "recipe", "rustc", "cargo")):
        return None
    jobs(client, run)
    latest = latest_runs(client, workflow, current, indirect=workflow == "paired-prerelease.yml")
    if not any(item["id"] == run["id"] and item["run_attempt"] == run["run_attempt"] for item in latest):
        return None
    return proof


def readiness(client, current):
    for run in latest_runs(client, "release.yml", current):
        if not same_workflow(client, run, ".github/workflows/release.yml"):
            continue
        all_jobs = jobs(client, run)
        required = ("channel", "linux", "macos-aarch64-apple-darwin", "macos-x86_64-apple-darwin", "windows-wsl")
        if not all(succeeded(all_jobs, name) for name in required):
            continue
        if not succeeded(all_jobs, "stable-readiness-result", ("Require every platform before recording stable readiness", "Upload stable readiness")):
            continue
        name = f"stable-readiness-{current['source']}-{run['run_attempt']}"
        value = artifact(client, run, name, "stable-readiness.json", current["repositoryId"])
        if (isinstance(value, dict) and value.get("source") == current["source"]
                and value.get("runId") == str(run["id"]) and value.get("runAttempt") == str(run["run_attempt"])
                and value.get("ref") == "refs/heads/" + run["head_branch"]
                and value.get("allNativePlatformsAndWsl") is True
                and value.get("published") is False and value.get("mainChanged") is False):
            return run
    return None


def provider_time(value):
    if not isinstance(value, str):
        raise ValueError("provider timestamp is unavailable")
    parsed = datetime.datetime.fromisoformat(value.replace("Z", "+00:00"))
    if parsed.tzinfo is None:
        raise ValueError("provider timestamp has no timezone")
    return parsed.astimezone(datetime.timezone.utc)


def paired_context():
    path = ROOT / "tooling/repository/release/paired-prerelease.py"
    spec = importlib.util.spec_from_file_location("paired_prerelease", path)
    if spec is None or spec.loader is None:
        raise ValueError("paired status owner is unavailable")
    owner = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(owner)
    return owner.CONTEXT


def paired_statuses(client, current):
    statuses = client.get(f"commits/{current['source']}/statuses?per_page=100")
    if not isinstance(statuses, list) or len(statuses) >= 100:
        raise ValueError("paired status listing may be incomplete")
    return statuses


def paired_source_runs(client, current):
    context = paired_context()
    prefix = f"https://github.com/{client.repo}/actions/runs/"
    run_ids = set()
    for status in paired_statuses(client, current):
        target = status.get("target_url")
        if status.get("context") != context or status.get("state") != "pending" or not isinstance(target, str):
            continue
        suffix = target.removeprefix(prefix) if target.startswith(prefix) else ""
        if suffix.isascii() and suffix.isdigit():
            run_ids.add(int(suffix))
    runs = []
    for run_id in run_ids:
        if run_id == current["runId"]:
            continue
        run = client.get(f"actions/runs/{run_id}")
        if run.get("id") != run_id:
            raise ValueError("paired run identity changed")
        runs.append(run)
    return runs


def require_no_newer_failure(client, current, origin):
    """Use other workflows and paired status only as exact-source failure fences."""
    origin_started = provider_time(origin["created_at"])
    origin_updated = provider_time(origin["updated_at"])
    for workflow, events in (("ci.yml", ("push", "pull_request", "workflow_dispatch")),
                             ("release.yml", ("push", "workflow_dispatch"))):
        runs = client.get(f"actions/workflows/{workflow}/runs?head_sha={current['source']}&per_page=100")["workflow_runs"]
        if len(runs) >= 100:
            raise ValueError("source run listing may be incomplete")
        for run in runs:
            if run.get("id") == current["runId"]:
                continue
            if not isinstance(run.get("head_sha"), str) or not SHA.fullmatch(run["head_sha"]):
                raise ValueError("source run commit is unavailable")
            if run["head_sha"] != current["source"]:
                continue
            if not isinstance(run.get("id"), int) or workflow_path(run) != f".github/workflows/{workflow}":
                raise ValueError("malformed source run identity")
            repository_id = (run.get("repository") or {}).get("id")
            head_repository_id = (run.get("head_repository") or {}).get("id")
            if not isinstance(repository_id, int) or not isinstance(head_repository_id, int):
                raise ValueError("source run repository is unavailable")
            if repository_id != current["repositoryId"] or head_repository_id != current["repositoryId"]:
                continue
            if not isinstance(run.get("event"), str) or not isinstance(run.get("status"), str):
                raise ValueError("source run state is unavailable")
            if run["event"] not in events:
                continue
            if run["status"] == "completed" and not isinstance(run.get("conclusion"), str):
                raise ValueError("source run conclusion is unavailable")
            started, updated = provider_time(run.get("created_at")), provider_time(run.get("updated_at"))
            if run.get("status") == "completed" and run.get("conclusion") == "success":
                continue
            if started >= origin_started or updated >= origin_updated:
                raise ValueError("same-source workflow failed or is pending")

    statuses = paired_statuses(client, current)
    pending = []
    context = paired_context()
    # The provider returns newest first. A terminal status completes the most
    # recent pending admission; the terminal record itself has no target URL.
    for status in reversed(statuses):
        if status.get("context") != context:
            continue
        state = status.get("state")
        when = provider_time(status.get("created_at"))
        if state == "pending":
            pending.append((status, when))
        elif state in ("success", "failure", "error"):
            if pending:
                pending.pop()
            if state != "success" and when >= origin_started:
                raise ValueError("same-source paired check failed")
        else:
            raise ValueError("unknown paired status")
    own_url = f"https://github.com/{client.repo}/actions/runs/{current['runId']}"
    if any(when >= origin_started and status.get("target_url") != own_url for status, when in pending):
        raise ValueError("same-source paired check is pending")


def discover(client, current, mode):
    if mode not in ("ci", "readiness", "release"):
        raise ValueError("unknown proof consumer")
    if current["attempt"] != 1:
        return None
    ready = readiness(client, current) if mode == "release" else None
    if mode == "release" and ready is None:
        return None
    if ready is not None:
        direct = proof_from_run(client, ready, current, "native")
        if direct is not None:
            require_no_newer_failure(client, current, ready)
            return direct
    if current["environment"]["RUNNER_OS"] != "Linux":
        return None
    for workflow in ("ci.yml", "paired-prerelease.yml"):
        for run in latest_runs(client, workflow, current, indirect=workflow != "ci.yml"):
            proof = proof_from_run(client, run, current, "ci")
            if proof is not None:
                if ready is not None:
                    jobs(client, ready)
                    latest = latest_runs(client, "release.yml", current)
                    if not latest or (latest[0]["id"], latest[0]["run_attempt"]) != (ready["id"], ready["run_attempt"]):
                        return None
                require_no_newer_failure(client, current, run)
                return proof
    if mode == "ci":
        ready = readiness(client, current)
        if ready is not None:
            proof = proof_from_run(client, ready, current, "native")
            if proof is not None:
                require_no_newer_failure(client, current, ready)
            return proof
    return None


def decision(proof, current):
    if proof is None:
        return {"reuse": "false"}
    return {
        "reuse": "true",
        "ci_package": "true" if proof["kind"] == "ci" and proof["coverage"] == list(CI_COVERAGE) else "false",
        "schema": "0" if proof["base"] == current["base"] else "1",
        "privacy": "0" if (proof["event"], proof["eventSha"], proof["history"]) == (current["event"], current["eventSha"], current["history"]) else "1",
        "origin": f"{proof['runId']}:{proof['attempt']}",
    }


def main():
    if len(sys.argv) == 4 and sys.argv[1] == "write":
        print(write(sys.argv[2], sys.argv[3]))
        return
    if len(sys.argv) != 3 or sys.argv[1] != "find":
        raise SystemExit("usage: source-proof.py write PATH ci|native | find ci|readiness|release")
    proof = None
    try:
        current = context()
        proof = discover(Client(), current, sys.argv[2])
    except Exception:  # Provider data is optional; any discovery failure executes checks.
        print("No compatible provider proof; checks will execute.", file=sys.stderr)
    for key, value in decision(proof, current if proof else {}).items():
        print(f"{key}={value}")


if __name__ == "__main__":
    main()
