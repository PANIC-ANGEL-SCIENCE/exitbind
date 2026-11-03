"""Provider-free controls for exact-source, attempt-bound CI proof reuse."""
import base64
import contextlib
import copy
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch
import zipfile

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("source_proof", ROOT / "tooling/repository/ci/source-proof.py")
PROOF = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PROOF)
SOURCE, OTHER, BASE = "a" * 40, "b" * 40, "c" * 40


def current(os_name="Linux", arch="X64"):
    return {
        "source": SOURCE, "base": BASE, "event": "push", "eventSha": SOURCE,
        "history": "d" * 64,
        "repositoryId": 1, "runId": 99, "attempt": 1,
        "environment": {"RUNNER_OS": os_name, "RUNNER_ARCH": arch, "ImageVersion": "image-a"},
        "recipe": "recipe-a", "rustc": "rustc-a", "cargo": "cargo-a",
    }


def run(run_id=1, path="ci.yml", head=SOURCE, branch="feature/a", event="push"):
    return {"id": run_id, "path": ".github/workflows/" + path, "head_sha": head,
            "head_branch": branch, "event": event, "repository": {"id": 1},
            "head_repository": {"id": 1}, "run_number": run_id, "run_attempt": 1,
            "created_at": f"2026-11-03T00:00:{run_id:02d}Z",
            "updated_at": f"2026-11-03T00:00:{run_id:02d}Z",
            "status": "completed", "conclusion": "success"}


def job(name, steps=()):
    return {"name": name, "status": "completed", "conclusion": "success",
            "steps": [{"name": step, "status": "completed", "conclusion": "success"} for step in steps]}


class FakeProvider:
    def __init__(self):
        self.repo = "fixture/repository"
        self.runs = {"ci.yml": [], "paired-prerelease.yml": [], "release.yml": []}
        self.by_id = {}
        self.job_list = {}
        self.artifacts = {}
        self.archives = {}
        self.statuses = {}
        self.bad_workflow = False
        self.offline = False

    def add_run(self, value, jobs):
        self.runs[value["path"].split("/")[-1]].append(value)
        self.by_id[value["id"]] = value
        self.job_list[value["id"]] = jobs
        self.artifacts[value["id"]] = []

    def add_artifact(self, run_value, name, filename, value):
        data = io.BytesIO()
        with zipfile.ZipFile(data, "w", zipfile.ZIP_DEFLATED) as archive:
            archive.writestr(filename, json.dumps(value))
        raw = data.getvalue()
        identifier = len(self.archives) + 1
        self.archives[identifier] = raw
        self.artifacts[run_value["id"]].append({
            "id": identifier, "name": name, "expired": False, "size_in_bytes": len(raw),
            "digest": "sha256:" + hashlib.sha256(raw).hexdigest(),
            "workflow_run": {"id": run_value["id"], "repository_id": 1, "head_repository_id": 1,
                             "head_sha": run_value["head_sha"], "head_branch": run_value["head_branch"]},
        })

    def get(self, path):
        if self.offline:
            raise OSError("offline")
        if path.startswith("contents/"):
            name = path[len("contents/"):].split("?", 1)[0]
            data = b"changed workflow" if self.bad_workflow else (ROOT / name).read_bytes()
            return {"content": base64.b64encode(data).decode()}
        if path.startswith("actions/workflows/"):
            name = path.split("/")[2]
            return {"workflow_runs": self.runs[name]}
        parts = path.split("/")
        if len(parts) == 3 and parts[0] == "commits" and parts[2].startswith("statuses?"):
            return self.statuses.get(parts[1], [])
        if len(parts) >= 3 and parts[:2] == ["actions", "runs"]:
            identifier = int(parts[2])
            if len(parts) == 3:
                return self.by_id[identifier]
            if len(parts) >= 6 and parts[3] == "attempts":
                return {"jobs": self.job_list[identifier]}
            if parts[3].startswith("artifacts?"):
                return {"artifacts": self.artifacts[identifier]}
        raise AssertionError(path)

    def bytes(self, path, limit):
        return self.archives[int(path.split("/")[2])]


def source_proof(provider, run_value, checked, kind="ci"):
    value = dict(checked, runId=run_value["id"], attempt=run_value["run_attempt"],
                 event=run_value["event"], eventSha=run_value["head_sha"],
                 schema="exitbind.source-proof.v1", origin="executed", kind=kind,
                 coverage=list(PROOF.CI_COVERAGE if kind == "ci" else PROOF.NATIVE_COVERAGE))
    provider.add_artifact(run_value, PROOF.artifact_name("source-proof", value), "source-proof.json", value)
    return value


def source_steps(kind="ci", linux=True):
    steps = ["Run shared native CI checks and packaging" if kind == "ci" else "Run shared native CI checks",
             "Write executed source proof", "Upload executed source proof"]
    if kind == "ci" or linux:
        steps += ["Test paired prerelease controls without network writes", "Test source proof controls without network writes"]
    if kind == "ci":
        steps += ["Test README tooling", "Verify Cargo package excludes README presentation tooling"]
    return steps


def find(provider, checked, mode="ci"):
    try:
        return PROOF.discover(provider, checked, mode)
    except (OSError, KeyError, ValueError, TypeError):
        return None


class SourceProofTests(unittest.TestCase):
    def setUp(self):
        self.current = current()
        self.api = FakeProvider()
        self.run = run()
        self.api.add_run(self.run, [job("exit", source_steps())])
        self.proof = source_proof(self.api, self.run, self.current)

    def test_exact_source_reuses_direct_origin_and_tracks_context_deltas(self):
        result = find(self.api, self.current)
        self.assertEqual(PROOF.decision(result, self.current),
                         {"reuse": "true", "ci_package": "true", "schema": "0", "privacy": "0", "origin": "1:1"})
        changed = dict(self.current, base=OTHER, event="workflow_dispatch")
        self.assertEqual(PROOF.decision(find(self.api, changed), changed)["schema"], "1")
        self.assertEqual(PROOF.decision(find(self.api, changed), changed)["privacy"], "1")
        changed_history = dict(self.current, history="e" * 64)
        self.assertEqual(PROOF.decision(find(self.api, changed_history), changed_history)["privacy"], "1")
        base_only = dict(self.current, base=OTHER)
        self.assertEqual(PROOF.decision(find(self.api, base_only), base_only)["privacy"], "0")
        self.run["path"] += "@feature/a"
        self.assertIsNotNone(find(self.api, self.current))
        self.run["path"] = ".github/workflows/ci.yml@other-branch"
        self.assertIsNone(find(self.api, self.current))

    def test_mismatched_missing_skipped_or_failed_evidence_cannot_reuse(self):
        cases = ("sha", "repository", "fork", "workflow", "platform", "image", "recipe", "toolchain", "attempt",
                 "artifact-fork", "missing", "expired", "digest", "skipped", "failure", "consumer", "offline", "chain", "event", "coverage")
        for case in cases:
            with self.subTest(case=case):
                api, checked = copy.deepcopy(self.api), copy.deepcopy(self.current)
                if case == "sha": checked["source"] = OTHER
                elif case == "repository": api.runs["ci.yml"][0]["repository"]["id"] = 2
                elif case == "fork": api.runs["ci.yml"][0]["head_repository"]["id"] = 2
                elif case == "workflow": api.bad_workflow = True
                elif case == "platform": checked["environment"]["RUNNER_ARCH"] = "ARM64"
                elif case == "image": checked["environment"]["ImageVersion"] = "image-b"
                elif case == "recipe": checked["recipe"] = "other-recipe"
                elif case == "toolchain": checked["rustc"] = "different"
                elif case == "attempt": api.runs["ci.yml"][0]["run_attempt"] = 2
                elif case == "artifact-fork": api.artifacts[1][0]["workflow_run"]["head_repository_id"] = 2
                elif case == "missing": api.artifacts[1] = []
                elif case == "expired": api.artifacts[1][0]["expired"] = True
                elif case == "digest": api.artifacts[1][0]["digest"] = "sha256:" + "0" * 64
                elif case == "skipped": api.job_list[1][0]["steps"][0]["conclusion"] = "skipped"
                elif case == "failure":
                    later = run(2)
                    later["conclusion"] = "failure"
                    api.add_run(later, [])
                elif case == "consumer": checked["runId"] = 1
                elif case == "offline": api.offline = True
                elif case == "chain":
                    api.artifacts[1] = []
                    changed = dict(self.proof, origin="reused")
                    api.add_artifact(api.by_id[1], PROOF.artifact_name("source-proof", changed), "source-proof.json", changed)
                elif case == "event":
                    api.artifacts[1] = []
                    changed = dict(self.proof, event="workflow_dispatch")
                    api.add_artifact(api.by_id[1], PROOF.artifact_name("source-proof", changed), "source-proof.json", changed)
                elif case == "coverage":
                    api.artifacts[1] = []
                    changed = dict(self.proof, coverage=list(PROOF.NATIVE_COVERAGE))
                    api.add_artifact(api.by_id[1], PROOF.artifact_name("source-proof", changed), "source-proof.json", changed)
                self.assertIsNone(find(api, checked))


    def test_a_new_attempt_during_artifact_read_refuses_the_old_proof(self):
        original = self.api.bytes
        def rerun(path, limit):
            data = original(path, limit)
            self.api.by_id[1] = dict(self.api.by_id[1], run_attempt=2, status="in_progress", conclusion=None)
            return data
        self.api.bytes = rerun
        self.assertIsNone(find(self.api, self.current))






    def test_later_valid_reuse_keeps_direct_origin_but_skipped_latest_refuses(self):
        later = run(2)
        reuse_steps = ["Find compatible executed source proof", "Recheck changed source context",
                       "Reuse completed CI qualification"]
        self.api.add_run(later, [job("exit", reuse_steps)])
        self.assertEqual(find(self.api, self.current)["runId"], 1)
        self.api.job_list[2][0]["steps"][2]["name"] = "Rebuild and verify the current package"
        self.assertEqual(find(self.api, self.current)["runId"], 1)
        self.api.job_list[2][0]["steps"][1]["conclusion"] = "skipped"
        self.assertIsNone(find(self.api, self.current))
        self.api.job_list[2] = [job("readme")]
        self.assertIsNone(find(self.api, self.current))

    def test_api_failure_emits_only_the_fallback_decision(self):
        output = io.StringIO()
        with patch.object(PROOF, "context", return_value=self.current), \
             patch.object(PROOF, "Client", side_effect=OSError("offline")), \
             patch.object(PROOF.sys, "argv", ["source-proof.py", "find", "ci"]), \
             contextlib.redirect_stdout(output), contextlib.redirect_stderr(io.StringIO()):
            PROOF.main()
        self.assertEqual(output.getvalue(), "reuse=false\n")

    def test_context_uses_checked_out_candidate_and_rejects_fork_or_wrong_sha(self):
        with tempfile.TemporaryDirectory(prefix="source-proof-context.") as name:
            root = Path(name)
            for item in PROOF.RECIPE:
                path = root / item
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(item)
            event = root / "event.json"
            event.write_text(json.dumps({"pull_request": {"head": {"repo": {"id": 1}}}}))
            env = {key: "set" for key in PROOF.REQUIRED_ENVIRONMENT}
            env.update(RUNNER_OS="Linux", RUNNER_ARCH="X64", GITHUB_REPOSITORY_ID="1",
                       GITHUB_EVENT_NAME="pull_request", GITHUB_EVENT_PATH=str(event),
                       GITHUB_SHA=OTHER, EXITBIND_EXPECTED_SHA=SOURCE,
                       GITHUB_RUN_ID="11", GITHUB_RUN_ATTEMPT="1", VALUE_PROOF_BASE=BASE)
            def command(*args):
                if args[:3] == ("git", "status", "--porcelain"):
                    return ""
                if args[:2] == ("git", "rev-list"):
                    return SOURCE + "\n" + BASE
                if args[:2] == ("git", "rev-parse"):
                    return BASE if "--verify" in args else SOURCE
                return "toolchain"
            with patch.dict(os.environ, env, clear=True), patch.object(PROOF, "ROOT", root), \
                 patch.object(PROOF, "command", side_effect=command):
                observed = PROOF.context()
                self.assertEqual((observed["source"], observed["eventSha"]), (SOURCE, OTHER))
                os.environ["CARGO_PROFILE_RELEASE_LTO"] = "thin"
                os.environ["CARGO_REGISTRIES_PRIVATE_TOKEN"] = "fixture-credential"
                changed_profile = PROOF.context()["environment"]
                self.assertNotEqual(observed["environment"], changed_profile)
                self.assertEqual(changed_profile["CARGO_PROFILE_RELEASE_LTO"], "thin")
                self.assertNotIn("CARGO_REGISTRIES_PRIVATE_TOKEN", changed_profile)
                event.write_text(json.dumps({"pull_request": {"head": {"repo": {"id": 2}}}}))
                with self.assertRaises(ValueError):
                    PROOF.context()
                event.write_text(json.dumps({"pull_request": {"head": {"repo": {"id": 1}}}}))
                os.environ["EXITBIND_EXPECTED_SHA"] = OTHER
                with self.assertRaises(ValueError):
                    PROOF.context()

    def test_executed_manifest_names_only_its_actual_qualification(self):
        with tempfile.TemporaryDirectory(prefix="source-proof-coverage.") as name, \
             patch.object(PROOF, "context", side_effect=lambda: dict(self.current)):
            output = Path(name) / "proof.json"
            PROOF.write(output, "ci")
            ci = json.loads(output.read_text())
            self.assertEqual(ci["coverage"], ["source", "release-build", "onboarding", "package", "installer"])
            self.assertEqual(PROOF.decision(ci, self.current)["ci_package"], "true")
            PROOF.write(output, "native")
            native = json.loads(output.read_text())
            self.assertEqual(native["coverage"], ["source"])
            self.assertEqual(PROOF.decision(native, self.current)["ci_package"], "false")


    def test_linux_readiness_consumes_only_original_ci_proof(self):
        self.assertEqual(find(self.api, self.current, "readiness")["runId"], 1)
        mac = current("macOS", "ARM64")
        self.assertIsNone(find(self.api, mac, "readiness"))

    def test_ci_reuses_native_source_only_and_keeps_ci_only_obligations(self):
        api = FakeProvider()
        ready = run(7, "release.yml", SOURCE, "integration/pair-test", "workflow_dispatch")
        names = ["channel", "linux", "macos-aarch64-apple-darwin", "macos-x86_64-apple-darwin", "windows-wsl"]
        all_jobs = [job(name) for name in names]
        all_jobs[1] = job("linux", source_steps("native"))
        all_jobs.append(job("stable-readiness-result", ["Require every platform before recording stable readiness", "Upload stable readiness"]))
        api.add_run(ready, all_jobs)
        source_proof(api, ready, self.current, "native")
        api.add_artifact(ready, f"stable-readiness-{SOURCE}-1", "stable-readiness.json", {
            "source": SOURCE, "runId": "7", "runAttempt": "1", "ref": "refs/heads/integration/pair-test",
            "allNativePlatformsAndWsl": True, "published": False, "mainChanged": False})
        proof = find(api, self.current, "ci")
        self.assertEqual((proof["kind"], PROOF.decision(proof, self.current)["ci_package"]), ("native", "false"))
        ci = (ROOT / ".github/workflows/ci.yml").read_text()
        for name in ("Test README tooling", "Verify Cargo package excludes README presentation tooling"):
            step = ci.split("- name: " + name + "\n", 1)[1]
            self.assertTrue(step.startswith("        if: steps.source-proof.outputs.reuse != 'true' || steps.source-proof.outputs.ci_package != 'true'\n"))
        self.assertIn("if: steps.source-proof.outputs.reuse == 'true' && steps.source-proof.outputs.ci_package != 'true'\n        run: ./tooling/repository/ci/ci-local.sh --package-only", ci)

    def test_release_can_follow_readiness_back_to_the_executed_linux_origin(self):
        ready = run(7, "release.yml", SOURCE, "integration/pair-test", "workflow_dispatch")
        names = ["channel", "linux", "macos-aarch64-apple-darwin", "macos-x86_64-apple-darwin", "windows-wsl"]
        all_jobs = [job(name) for name in names]
        all_jobs.append(job("stable-readiness-result", ["Require every platform before recording stable readiness", "Upload stable readiness"]))
        self.api.add_run(ready, all_jobs)
        self.api.add_artifact(ready, f"stable-readiness-{SOURCE}-1", "stable-readiness.json", {
            "source": SOURCE, "runId": "7", "runAttempt": "1", "ref": "refs/heads/integration/pair-test",
            "allNativePlatformsAndWsl": True, "published": False, "mainChanged": False})
        self.assertEqual(find(self.api, self.current, "release")["runId"], 1)
        original = self.api.bytes
        def newer_readiness(path, limit):
            data = original(path, limit)
            if path == "actions/artifacts/1/zip":
                later = run(8, "release.yml", SOURCE, "integration/pair-new", "workflow_dispatch")
                later.update(status="in_progress", conclusion=None)
                self.api.add_run(later, [])
            return data
        self.api.bytes = newer_readiness
        self.assertIsNone(find(self.api, self.current, "release"))
        self.api.bytes = original
        self.api.runs["release.yml"].pop()
        self.api.job_list[7][4]["conclusion"] = "failure"
        self.assertIsNone(find(self.api, self.current, "release"))

    def test_artifact_and_publication_steps_remain_after_reuse(self):
        ci = (ROOT / ".github/workflows/ci.yml").read_text()
        release = (ROOT / ".github/workflows/release.yml").read_text()
        paired = (ROOT / ".github/workflows/paired-prerelease.yml").read_text()
        for block in (ci.split("  exit:\n", 1)[1].split("  audit:\n", 1)[0],
                      release.split("  linux:\n", 1)[1].split("  macos:\n", 1)[0],
                      release.split("  macos:\n", 1)[1].split("  windows-wsl:\n", 1)[0],
                      paired.split("  check:\n", 1)[1].split("  finish:\n", 1)[0]):
            self.assertIn("      statuses: read\n", block)
            self.assertNotIn("      statuses: write\n", block)
        self.assertIn("if: steps.source-proof.outputs.reuse == 'true' && steps.source-proof.outputs.ci_package != 'true'\n        run: ./tooling/repository/ci/ci-local.sh --package-only", ci)
        self.assertIn("if: steps.source-proof.outputs.reuse == 'true' && steps.source-proof.outputs.ci_package == 'true'", ci)
        self.assertIn("  audit:\n", ci)
        self.assertIn("- run: cargo audit", ci)
        for required in ("Build native Linux stable release", "Build native macOS stable release",
                         "Exercise first-user path without language toolchains", "Verify tag matches the binary",
                         "Package native Linux stable release", "Package native macOS stable release",
                         "Exercise packaged Linux installer", "Exercise packaged macOS installer",
                         "Attest packaged archive provenance", "Verify packaged archive provenance",
                         "Exercise the installed Linux release inside WSL 2", "Verify transferred release evidence",
                         "Publish tag assets", "Retrieve and verify published installer"):
            step = release.split("- name: " + required + "\n", 1)
            self.assertEqual(len(step), 2, required)
            self.assertNotIn("steps.source-proof.outputs", step[1].split("\n      - ", 1)[0], required)
        self.assertEqual(ci.count("if: steps.source-checks.outcome == 'success'"), 1)
        self.assertEqual(release.count("&& steps.source-checks.outcome == 'success'"), 2)

    def test_context_guard_selects_only_live_history_and_identity(self):
        selector = ROOT / "tooling/repository/ci/ci-stages.sh"
        script = '''. "$1"
for check in path-rendering product-surface release-refs public-history commit-identity; do
  if ci_repository_rule_check_selected "$check" "$2" "$3"; then printf '%s\\n' "$check"; fi
done
'''
        for platform in ("linux", "macos"):
            def selected(mode):
                return subprocess.check_output(["sh", "-c", script, "guard", str(selector), platform, mode],
                                               text=True).splitlines()
            self.assertEqual(selected("context"), ["public-history", "commit-identity"])
            expected = (["path-rendering", "product-surface", "release-refs"] if platform == "linux" else [])
            self.assertEqual(selected("full"), expected + ["public-history", "commit-identity"])
            self.assertEqual(selected("pre-return"), selected("full"))

    def test_readiness_manifest_is_written_outside_the_checkout(self):
        with tempfile.TemporaryDirectory(prefix="readiness-output.") as name:
            work = Path(name) / "checkout"
            temp = Path(name) / "runner-temp"
            work.mkdir()
            temp.mkdir()
            env = dict(os.environ, RUNNER_TEMP=str(temp), GITHUB_SHA=SOURCE,
                       GITHUB_REF="refs/heads/integration/pair-test", GITHUB_RUN_ID="7",
                       GITHUB_RUN_ATTEMPT="2", CHANNEL_RESULT="success", LINUX_RESULT="success",
                       MACOS_RESULT="success", WSL_RESULT="success")
            subprocess.run(["python3", str(ROOT / "tooling/repository/release/write-stable-readiness.py")],
                           cwd=work, env=env, check=True)
            self.assertEqual(list(work.iterdir()), [])
            value = json.loads((temp / "stable-readiness.json").read_text())
            self.assertEqual((value["source"], value["runId"], value["runAttempt"]), (SOURCE, "7", "2"))
            self.assertIn("path: ${{ runner.temp }}/stable-readiness.json",
                          (ROOT / ".github/workflows/release.yml").read_text())

    def test_actual_context_and_package_dispatch_uses_checkout_identity(self):
        with tempfile.TemporaryDirectory(prefix="source-proof-dispatch.") as name:
            root = Path(name)
            ci = root / "tooling/repository/ci"
            scripts = root / "scripts"
            tools = root / "bin"
            for path in (ci, scripts, tools):
                path.mkdir(parents=True)
            for file in ("ci-local.sh", "ci-stages.sh", "assert-test-ran.sh"):
                shutil.copyfile(ROOT / "tooling/repository/ci" / file, ci / file)
            shutil.copyfile(ROOT / "scripts/assert-test-ran.sh", scripts / "assert-test-ran.sh")
            (scripts / "assert-test-ran.sh").chmod(0o755)
            (ci / "verification-context.sh").write_text('''
prepare_verification_context() { :; }
begin_verification_run() { run_dir="$TMPDIR/logs"; mkdir -p "$run_dir"; }
run() {
  printf '%s|%s|%s\\n' "$1" "$*" "${EXITBIND_BUILD_COMMIT:-}" >> "$TRACE"
  if [ "$1" = context-schema ]; then
    printf '%s\\n' 'running 1 test' 'test schema_lock_matches_all_current_marked_schemas ... ok' > "$run_dir/context-schema.log"
  fi
}
''')
            (tools / "git").write_text(f'''#!/bin/sh
if [ "$1" = rev-parse ]; then printf "%s\\n" {SOURCE}; fi
if [ "$1" = status ] && [ "${{FAKE_GIT_DIRTY:-0}}" = 1 ]; then printf '%s\\n' '?? new-file'; fi
exit 0
''')
            (tools / "rustc").write_text('''#!/bin/sh
case "$(uname -s):$(uname -m)" in
  Linux:x86_64) echo 'host: x86_64-unknown-linux-gnu' ;;
  Darwin:arm64) echo 'host: aarch64-apple-darwin' ;;
  Darwin:x86_64) echo 'host: x86_64-apple-darwin' ;;
esac
''')
            for executable in ("cargo", "tmux"):
                (tools / executable).write_text("#!/bin/sh\nexit 0\n")
            for path in tools.iterdir():
                path.chmod(0o755)
            trace = root / "trace"
            env = dict(os.environ, PATH=str(tools) + os.pathsep + os.defpath,
                       CARGO=str(tools / "cargo"), CARGO_TARGET_DIR=str(root / "target"),
                       TMPDIR=str(root), TRACE=str(trace), VALUE_PROOF_BASE=BASE,
                       GITHUB_SHA=OTHER)
            for privacy, schema, expected in [("0", "0", ["repository-rules"]),
                                               ("0", "1", ["repository-rules", "context-schema"]),
                                               ("1", "0", ["repository-rules", "context-privacy"])]:
                trace.write_text("")
                result = subprocess.run(["sh", str(ci / "ci-local.sh"), "--context-only"], cwd=root,
                                        env=dict(env, CI_REUSE_PRIVACY=privacy, CI_REUSE_SCHEMA=schema),
                                        capture_output=True, text=True, timeout=15)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                rows = trace.read_text().splitlines()
                self.assertEqual([row.split("|", 1)[0] for row in rows], expected)
                self.assertIn(f"check-repository-rules.sh {BASE} ", rows[0])
                self.assertIn(" context|", rows[0])
                self.assertFalse(any("test --locked --features repository-checks|" in row for row in rows))
            trace.write_text("")
            dirty = subprocess.run(["sh", str(ci / "ci-local.sh"), "--context-only"], cwd=root,
                                   env=dict(env, FAKE_GIT_DIRTY="1"), capture_output=True, text=True, timeout=15)
            self.assertNotEqual(dirty.returncode, 0)
            self.assertIn("source changed after proof discovery", dirty.stderr)
            self.assertEqual(trace.read_text(), "")
            trace.write_text("")
            result = subprocess.run(["sh", str(ci / "ci-local.sh"), "--package-only"], cwd=root,
                                    env=env, capture_output=True, text=True, timeout=15)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            rows = trace.read_text().splitlines()
            expected = ["release-build", "onboarding", "package", "installer"] if platform.system() == "Linux" else ["release-build", "package", "installer"]
            self.assertEqual([row.split("|", 1)[0] for row in rows], expected)
            self.assertTrue(all(row.endswith("|" + SOURCE) for row in rows), rows)


if __name__ == "__main__":
    unittest.main()
