#!/usr/bin/env python3
"""Exercise owned verification with a real product fixture and controlled Cargo transport."""

import json
import os
import pathlib
import signal
import subprocess
import sys
import time


ROOT = pathlib.Path(__file__).resolve().parents[3]
NATIVE = (
    "CODEX_THREAD_ID", "CODEX_SESSION_ID", "CLAUDE_CODE_SESSION_ID",
    "EXITBIND_NATIVE_SESSION_ID", "EXITBIND_SESSION_GOALS_DIR",
    "CLAUDE_ENV_FILE", "XDG_STATE_HOME",
)
GIT_CONTEXT = (
    "GIT_DIR", "GIT_WORK_TREE", "GIT_COMMON_DIR", "GIT_INDEX_FILE",
    "GIT_CEILING_DIRECTORIES", "GIT_DISCOVERY_ACROSS_FILESYSTEM",
)


def snapshot(root):
    return {str(p.relative_to(root)): p.read_bytes() for p in root.rglob("*") if p.is_file()}


def call(binary, project, env, *arguments):
    result = subprocess.run(
        [binary, *arguments, "--config", str(project / "exitbind.json")],
        cwd=project, env=env, capture_output=True, timeout=30,
    )
    assert result.returncode == 0, (result.stdout, result.stderr)
    return json.loads(result.stdout)


def main():
    binary, temporary = sys.argv[1:]
    temporary = pathlib.Path(temporary)
    env = {k: v for k, v in os.environ.items() if k not in NATIVE + GIT_CONTEXT}
    env.pop("QUIET_COMMAND_LOG", None)
    project = temporary / "project"
    project.mkdir()
    init = subprocess.run(
        [binary, "init", "--mode", "portable", "--root", str(project), "--skip-skills"],
        env=env, capture_output=True, timeout=30,
    )
    assert init.returncode == 0, (init.stdout, init.stderr)
    store = temporary / "outer-store"
    store.mkdir(mode=0o700)
    outer = dict(env, CODEX_THREAD_ID="controlled-outer", EXITBIND_SESSION_GOALS_DIR=str(store))
    call(binary, project, outer, "goal", "incorporate", "--goal-id", "outer",
         "--goal", "Keep the unrelated outer task unchanged", "--none-applicable",
         "obligations,findings,blockers,decisions,externalActions")
    before = snapshot(store)
    before_status = call(binary, project, outer, "goal", "status", "--json")

    # This transport avoids nested Cargo builds inside Cargo tests. Its test
    # operation really starts Exitbind Work with the raw inherited environment.
    cargo = temporary / "cargo"
    cargo.write_text("#!" + sys.executable + "\n" + r'''
import json, os, pathlib, subprocess, sys, time
if sys.argv[1:] == ["--version"]:
    print("cargo controlled-verification-transport")
    raise SystemExit(0)
assert sys.argv[1:7] == ["test", "--locked", "--features", "repository-checks", "--test", "controlled_fixture"]
assert sys.argv[7:] == ["filter with spaces", "--", "--exact"]
selected = {k: v for k, v in os.environ.items() if k in json.loads(os.environ["CONTEXT_NAMES"])}
pathlib.Path(os.environ["CONTEXT_CAPTURE"]).write_text(json.dumps(selected))
if os.environ["SCENARIO"] == "explicit":
    os.environ["CODEX_THREAD_ID"] = "controlled-inner"
    os.environ["EXITBIND_SESSION_GOALS_DIR"] = os.environ["INNER_STORE"]
    incorporated = subprocess.run([os.environ["PRODUCT_BINARY"], "goal", "incorporate",
        "--goal-id", "inner", "--goal", "Owned synthetic native task",
        "--none-applicable", "obligations,findings,blockers,decisions,externalActions",
        "--config", os.environ["FIXTURE_CONFIG"]], capture_output=True)
    assert incorporated.returncode == 0, incorporated.stderr
p = subprocess.run([os.environ["PRODUCT_BINARY"], "work", "begin", "change",
    "--goal", "owned verification fixture", "--check-command", "true",
    "--review-policy", "omitted", "--detail", "--config", os.environ["FIXTURE_CONFIG"]],
    capture_output=True)
sys.stdout.buffer.write(p.stdout)
sys.stderr.buffer.write(p.stderr)
assert p.returncode == 0
pathlib.Path(os.environ["READY"]).write_text(str(os.getpid()))
print("fixture stderr retained", file=sys.stderr, flush=True)
print("fixture stdout retained", flush=True)
if os.environ["SCENARIO"] == "interrupt":
    child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"])
    pathlib.Path(os.environ["DESCENDANT"]).write_text(str(child.pid))
    time.sleep(60)
if os.environ["SCENARIO"] == "failure":
    raise SystemExit(7)
print("running 1 test")
print("test controlled_fixture ... ok")
print("test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s")
''')
    cargo.chmod(0o700)
    for scenario in ("success", "failure", "interrupt", "explicit"):
        case = temporary / scenario
        case.mkdir()
        target = case / "target"
        logs = case / "logs"
        ready = case / "ready"
        descendant = case / "descendant"
        inner = case / "inner-store"
        inner.mkdir(mode=0o700)
        seeded = dict(env)
        seeded.update({name: "controlled-" + name for name in NATIVE + GIT_CONTEXT})
        seeded.update({
            "CODEX_THREAD_ID": "controlled-outer", "EXITBIND_SESSION_GOALS_DIR": str(store),
            "CARGO": str(cargo), "CARGO_TARGET_DIR": str(target), "TMPDIR": str(case),
            "CI_LOG_ROOT": str(logs), "PRODUCT_BINARY": binary,
            "FIXTURE_CONFIG": str(project / "exitbind.json"), "SCENARIO": scenario,
            "CONTEXT_NAMES": json.dumps(NATIVE + GIT_CONTEXT),
            "CONTEXT_CAPTURE": str(case / "context.json"), "READY": str(ready),
            "DESCENDANT": str(descendant), "INNER_STORE": str(inner),
        })
        argv = [str(ROOT / "scripts/test-focused.sh"), "--test", "controlled_fixture",
                "filter with spaces", "--", "--exact"]
        child = subprocess.Popen(argv, cwd=temporary, env=seeded,
                                 stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            if scenario == "interrupt":
                deadline = time.monotonic() + 20
                while not descendant.exists() and child.poll() is None and time.monotonic() < deadline:
                    time.sleep(0.02)
                assert descendant.exists(), "fixture did not reach interruption boundary"
                child.send_signal(signal.SIGTERM)
            stdout, stderr = child.communicate(timeout=30)
        finally:
            if child.poll() is None:
                child.terminate()
                child.communicate(timeout=10)
        expected = {"failure": 7, "interrupt": 143}.get(scenario, 0)
        assert child.returncode == expected, (scenario, child.returncode, stdout, stderr)
        assert json.loads((case / "context.json").read_text()) == {}, "ambient selectors survived"
        assert snapshot(store) == before, "fixture changed unrelated outer Goal bytes or membership"
        current = call(binary, project, outer, "goal", "status", "--json")
        assert current["goalId"] == before_status["goalId"] and current["goal"] == before_status["goal"]
        runs = list(logs.glob("run.*"))
        assert len(runs) == 1
        run = runs[0]
        assert (run / "run.exit").read_text().strip() == str(expected)
        assert (run / "focused.log.exit").read_text().strip() == str(expected)
        metadata = json.loads((run / "focused.log.meta.json").read_text())
        assert metadata["state"] == "complete" and metadata["shellExitCode"] == expected
        assert metadata["argv"] == [str(cargo), "test", "--locked", "--features", "repository-checks", *argv[1:]]
        assert metadata["cwd"] == str(ROOT)
        assert b"fixture stdout retained" in (run / "focused.log.stdout").read_bytes()
        assert b"fixture stderr retained" in (run / "focused.log.stderr").read_bytes()
        assert (target / ".ci-local-checkout").read_text().strip() == str(ROOT)
        assert not (target / ".ci-local-lock").exists(), "owned lock survived completed capture"
        if scenario == "explicit":
            assert snapshot(inner), "explicit isolated identity was not re-added"
        else:
            assert not snapshot(inner)
        if scenario == "interrupt":
            # A process killed with its owned group can briefly remain a zombie;
            # it cannot keep writing or outlive its parent as a running process.
            pid = int(descendant.read_text())
            probe = subprocess.run(["ps", "-p", str(pid), "-o", "stat="], capture_output=True)
            assert not probe.stdout.strip() or probe.stdout.strip().startswith(b"Z")
        # A different checkout cannot claim this completed target.
        (target / ".ci-local-checkout").write_text("another checkout\n")
        refused = subprocess.run(argv, cwd=temporary, env=seeded, capture_output=True, timeout=20)
        assert refused.returncode != 0
        assert len(list(logs.glob("run.*"))) == 1, "refusal created another run"
        for override in ("--target-dir", "--manifest-path", "--target"):
            bypass = subprocess.run(
                [argv[0], "--test", "controlled_fixture", override, str(case / "bypass")],
                cwd=temporary, env=seeded, capture_output=True, timeout=20,
            )
            assert bypass.returncode != 0
            assert b"only --test targets" in bypass.stderr
            assert not (case / "bypass").exists()
    print("verification context: outer bytes/membership preserved; failure, interruption, explicit identity, argv/cwd/cache/status checked")


if __name__ == "__main__":
    main()
