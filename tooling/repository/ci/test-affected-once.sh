#!/bin/sh
# Exercise real proportional dispatch with recording Cargo and isolated tools.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd -P)
python3 -B - "$root" <<'PY'
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

root = Path(sys.argv[1])
with tempfile.TemporaryDirectory(prefix="affected-scope.", dir=os.environ.get("TMPDIR")) as temp:
    fixture = Path(temp)
    repo = fixture / "repo"
    subprocess.run(["git", "clone", "-q", "--shared", str(root), str(repo)], check=True)
    for relative in ["Cargo.toml", "tooling/repository/checks/test_target_sources.py",
                     "tooling/repository/checks/test-target-map.json"]:
        shutil.copyfile(root / relative, repo / relative)
    for name in ["ci-local.sh", "ci-stages.sh", "test-affected.sh", "verification-context.sh", "ci-preflight.sh"]:
        shutil.copyfile(root / "tooling/repository/ci" / name, repo / "tooling/repository/ci" / name)
    # Use current target/source declarations; only the affected behavior is
    # stubbed. No host validator, provider, build or real service is discovered.
    shutil.rmtree(repo / "tests")
    shutil.copytree(root / "tests", repo / "tests")
    # An explicitly read source is also compiled by another target. The
    # reader must be selected even though that compiled owner already hit.
    with (repo / "tests/first_impression.rs").open("a") as out:
        out.write('\nconst SELECTION_INPUT: &str = "tests/temp_root_contract/temp_root_guard.rs";\n')
    for relative in ["scripts/check-workflows.sh", "scripts/test-workflow-validation.sh", "scripts/test-ci-stages.sh"]:
        (repo / relative).write_text('#!/bin/sh\nprintf "%s\\n" "$0" >> "$OTHER_RECORD"\n')
    (repo / "scripts/test-quiet-command.py").write_text('import os\nwith open(os.environ["OTHER_RECORD"], "a") as f: f.write("quiet-regression\\n")\n')
    (repo / "tooling/repository/ci/check-repository-rules.sh").write_text('#!/bin/sh\nexit 0\n')
    for key, value in [("user.name", "envy404"), ("user.email", "287204764+envy404@users.noreply.github.com")]:
        subprocess.run(["git", "-C", str(repo), "config", key, value], check=True)
    subprocess.run(["git", "-C", str(repo), "add", "-A"], check=True)
    subprocess.run(["git", "-C", str(repo), "commit", "-qm", "Prepare isolated dispatch fixture"], check=True)
    base = subprocess.check_output(["git", "-C", str(repo), "rev-parse", "HEAD"], text=True).strip()
    cargo = fixture / "cargo"
    cargo.write_text('''#!/bin/sh
printf '%s|%s|%s|%s\\n' "${CARGO_PROFILE_DEV_OPT_LEVEL:-missing}" "${CARGO_PROFILE_TEST_OPT_LEVEL:-missing}" "${CARGO_PROFILE_DEV_DEBUG:-missing}" "${CARGO_PROFILE_TEST_DEBUG:-missing}" >> "$PROFILE_RECORD"
case ${1:-} in
  --version) printf 'cargo scope-fixture\\n'; exit 0 ;;
  fmt|clippy) printf '%s\\n' "$*" >> "$CARGO_RECORD" ;;
  test)
    printf '%s\\n' "$*" >> "$CARGO_RECORD"
    case " $* " in *" --test ${FAIL_TARGET:-__none__} "*) exit 47 ;; esac
    case " $* " in
      *" --test ${ZERO_TARGET:-__none__} "*) printf 'test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\\n' ;;
      *) printf 'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\\n' ;;
    esac ;;
  *) exit 2 ;;
esac
''')
    cargo.chmod(0o700)
    system = os.uname().sysname
    host = {("Linux", "x86_64"): "x86_64-unknown-linux-gnu",
            ("Darwin", "arm64"): "aarch64-apple-darwin",
            ("Darwin", "x86_64"): "x86_64-apple-darwin"}[(system, os.uname().machine)]
    rustc = fixture / "rustc"
    rustc.write_text(f"#!/bin/sh\nprintf 'host: {host}\\n'\n")
    rustc.chmod(0o700)
    for name in ["tmp", "logs"]:
        (fixture / name).mkdir()
    calls = fixture / "cargo.calls"
    other = fixture / "other.calls"
    profiles = fixture / "profile.calls"
    env = dict(os.environ, CARGO=str(cargo), CARGO_TARGET_DIR=str(fixture / "target"),
               TMPDIR=str(fixture / "tmp"), CI_LOG_ROOT=str(fixture / "logs"),
               CARGO_RECORD=str(calls), OTHER_RECORD=str(other), PROFILE_RECORD=str(profiles), VALUE_PROOF_BASE=base,
               PYTHONDONTWRITEBYTECODE="1")
    for name in ["PRE_RETURN_BASE", "FAIL_TARGET", "ZERO_TARGET", "CI_PRE_RETURN_SCOPE",
                 "CARGO_PROFILE_DEV_OPT_LEVEL", "CARGO_PROFILE_TEST_OPT_LEVEL",
                 "CARGO_PROFILE_DEV_DEBUG", "CARGO_PROFILE_TEST_DEBUG"]:
        env.pop(name, None)
    def run(args, success=True, **extra):
        calls.write_text("")
        other.write_text("")
        profiles.write_text("")
        result = subprocess.run(args, cwd=repo, env=dict(env, **extra), capture_output=True, text=True)
        if (result.returncode == 0) != success:
            raise AssertionError(f"{args}: exit {result.returncode}\n{result.stdout}\n{result.stderr}")
        return calls.read_text(), other.read_text(), result
    def restore():
        subprocess.run(["git", "-C", str(repo), "restore", "--worktree", "."], check=True)
    def edit(relative):
        with (repo / relative).open("a") as out:
            out.write("\n// scoped dispatch fixture\n" if relative.endswith(".rs") else "\nscoped dispatch fixture\n")
    ci = ["sh", "tooling/repository/ci/ci-local.sh", "--pre-return"]
    affected = ["sh", "tooling/repository/ci/test-affected.sh", "--base", base]
    edit("docs/examples/value-report.md")
    log, tools, _ = run(ci)
    assert "clippy" not in log and "fmt" not in log and not tools, (log, tools)
    assert "--test privacy_gate" not in log, log
    assert set(profiles.read_text().splitlines()) == {"1|1|0|0"}, profiles.read_text()
    run(ci, CARGO_PROFILE_DEV_OPT_LEVEL="0", CARGO_PROFILE_TEST_OPT_LEVEL="0",
        CARGO_PROFILE_DEV_DEBUG="2", CARGO_PROFILE_TEST_DEBUG="2")
    assert set(profiles.read_text().splitlines()) == {"0|0|2|2"}, profiles.read_text()
    print("public recipe: CI runtime profiles by default; explicit overrides retained")
    print("docs-only: unrelated lint/workflow/privacy targets omitted")
    restore()
    edit("tests/first_impression.rs")
    log, tools, _ = run(ci)
    assert "test --locked --test first_impression\n" in log, log
    if system == "Linux":
        assert "clippy --locked --bin exitbind --test first_impression" in log, log
    else:
        assert "clippy" not in log, log
    assert "--all-targets" not in log and not tools, (log, tools)
    for name in ["FAIL_TARGET", "ZERO_TARGET"]:
        log, _, result = run(ci, success=False, **{name: "first_impression"})
        assert "stage=affected-tests" in result.stdout, result.stdout
        assert log.count("test --locked --test first_impression\n") == 1, log
    print("affected regression: nonzero and zero-test results refuse")
    restore()
    edit("tests/temp_root_contract/temp_root_guard.rs")
    log, _, _ = run(affected)
    assert "test --locked --test first_impression\n" in log, log
    assert "test --locked --features repository-checks --test repository_contracts\n" in log, log
    assert "test --locked --features repository-checks --test privacy_gate\n" in log, log
    assert "--test host_runtime" not in log, log
    print("test input: compiled owner, explicit reader and moved scanner selected")
    restore()
    edit("tests/memory_recall.rs")
    log, _, _ = run(affected)
    assert "test --locked --test memory\n" in log, log
    assert "test --locked --features repository-checks --test repository_contracts\n" in log, log
    assert "test --locked --features repository-checks --test privacy_gate\n" in log, log
    assert "--test first_impression" not in log and "--test host_runtime" not in log, log
    for name in ["FAIL_TARGET", "ZERO_TARGET"]:
        log, _, result = run(affected, success=False, **{name: "repository_contracts"})
        assert "repository_contracts: exit=" in result.stdout, result.stdout
        assert log.count("test --locked --features repository-checks --test repository_contracts\n") == 1, log
    print("test input: target and repository scanners run with required feature and refuse failures")
    restore()
    edit("src/memory/policy.rs")
    log, tools, _ = run(ci)
    assert "--test memory" in log and "--bin exitbind" in log and not tools, (log, tools)
    assert "test --locked --features repository-checks --test repository_contracts\n" in log, log
    assert "test --locked --features repository-checks --test privacy_gate\n" in log, log
    assert "--test goal_basis_requirements" not in log and "--test host_runtime" not in log, log
    print("Rust maintenance: affected targets, unit tests and platform lint policy retained")
    restore()
    edit("src/kernel/result_contract.rs")
    log, _, _ = run(affected)
    assert "test --locked --test goal_basis_requirements\n" in log, log
    print("specific Goal runtime source reader remains selected")
    restore()
    edit("src/cli/work/detail.rs")
    log, _, _ = run(affected)
    assert "test --locked --test host_runtime\n" in log, log
    print("specific host runtime view reader remains selected")
    restore()
    edit("skills/exitbind-bootstrap/SKILL.md")
    log, _, _ = run(affected)
    assert "test --locked --test away_native\n" in log, log
    assert "test --locked --test host_runtime\n" in log, log
    assert "test --locked --test goal_progress_presentation\n" in log, log
    assert "--test memory" not in log, log
    print("embedded input selects its source covers and explicit readers")
    restore()
    edit("tests/support/mod.rs")
    log, _, _ = run(affected)
    assert "--test away_native" in log and "--test memory" in log and "--test repository_contracts" in log, log
    print("shared support selects all targets")
    restore()
    edit("tooling/repository/ci/ci-local.sh")
    # Preserve shell validity while changing a checker input.
    p = repo / "tooling/repository/ci/ci-local.sh"
    p.write_text(p.read_text().replace("\nscoped dispatch fixture\n", "\n# scoped dispatch fixture\n"))
    log, tools, _ = run(ci)
    assert "test-ci-stages.sh" in tools and "quiet-regression" in tools, tools
    assert "--test repository_contracts" in log, log
    print("checker change: broad non-Cargo checks and affected Cargo coverage retained")
    restore()
    unknown = repo / "future.build-input"
    unknown.write_text("unknown dependency\n")
    log, tools, _ = run(ci)
    assert "--test away_native" in log and "test-ci-stages.sh" in tools, (log, tools)
    unknown.unlink()
    untracked = repo / "tests/untracked_scope.rs"
    untracked.write_text("// unreferenced source\n")
    log, _, _ = run(ci)
    assert "--test away_native" in log, log
    log, _, _ = run(affected + ["--skip-passed", "privacy_gate=repository-checks first_impression=repository-checks"])
    assert "--test privacy_gate" not in log and "test --locked --test first_impression" in log, log
    log, _, _ = run(affected)
    assert "--test privacy_gate" in log, log
    print("unknown/untracked: conservative fallback; exact feature pair and standalone semantics retained")
print("affected proportional dispatch contract passed")
PY
