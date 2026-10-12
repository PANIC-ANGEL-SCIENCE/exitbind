#!/usr/bin/env python3
"""Focused regression for quiet-command capture, projection and raw recovery."""

import hashlib
import json
import os
import pathlib
import re
import stat
import subprocess
import sys
import tempfile


ROOT = pathlib.Path(__file__).resolve().parent.parent
WRAPPER = ROOT / "scripts" / "quiet-command.sh"
HELPER = ROOT / "scripts" / "quiet-command.py"


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def write_cargo_fixture(path):
    source = r'''#!/usr/bin/env python3
import os
import pathlib
import sys

counter = os.environ.get("FIXTURE_RUN_COUNTER")
if counter:
    with open(counter, "ab") as stream:
        stream.write(b"run\n")
scenario = sys.argv[2]

if scenario == "large":
    sys.stdout.write("    Finished `test` profile [unoptimized] target(s) in 0.02s\n")
    sys.stdout.write("     Running unittests src/lib.rs (target/debug/deps/example)\n")
    sys.stdout.write("running 1200 tests\n")
    for index in range(1200):
        sys.stdout.write(f"test fixture::case_{index:04} ... ok\n")
    sys.stdout.write("fixture note: café 🐈 retained\n")
    sys.stdout.write("Finished with important test diagnostics: retained\n")
    sys.stdout.flush()
    sys.stderr.write("warning: controlled warning café retained\n")
    sys.stderr.flush()
    sys.stdout.write("test result: ok. 1200 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.17s\n")
elif scenario == "failure":
    sys.stdout.write("running 1000 tests\n")
    for index in range(999):
        sys.stdout.write(f"test fixture::case_{index:04} ... ok\n")
    sys.stdout.write("test fixture::late_failure ... FAILED\n")
    sys.stdout.write("late failure detail: assertion failed at the end of the run\n")
    sys.stdout.write("test result: FAILED. 999 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.21s\n")
    sys.stdout.flush()
    raise SystemExit(23)
elif scenario == "unknown":
    sys.stdout.write("running 1 test\n")
    sys.stdout.write("future-format test record: status=ok\n")
    sys.stdout.write("test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n")
elif scenario == "zero":
    sys.stdout.write("running 0 tests\n")
    sys.stdout.write("test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n")
elif scenario == "binary":
    sys.stdout.buffer.write(b"running 1 test\ninvalid-utf8:\xff\n")
elif scenario == "generic":
    sys.stdout.write("ordinary generic output\n")
else:
    raise SystemExit(2)
'''
    path.write_text(source, encoding="utf-8")
    path.chmod(0o755)


def invoke(label, command, log=None, extra_env=None, cwd=None):
    env = os.environ.copy()
    env.pop("QUIET_COMMAND_LOG", None)
    if log is not None:
        env["QUIET_COMMAND_LOG"] = str(log)
    if extra_env:
        env.update({key: str(value) for key, value in extra_env.items()})
    return subprocess.run(
        [str(WRAPPER), label, *[str(value) for value in command]],
        cwd=cwd or ROOT,
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


def mode_is_private(path):
    return stat.S_IMODE(path.stat().st_mode) == 0o600 and not path.is_symlink()


def main():
    with tempfile.TemporaryDirectory(prefix="quiet-command-regression-") as temporary:
        root = pathlib.Path(temporary)
        fake_cargo = root / "cargo"
        write_cargo_fixture(fake_cargo)
        counter = root / "large-runs"
        large_log = root / "large.log"

        large = invoke(
            "large-tests",
            [fake_cargo, "test", "large"],
            large_log,
            {"FIXTURE_RUN_COUNTER": counter},
            root,
        )
        require(large.returncode == 0, "recognized successful Cargo test status changed")
        projected = large.stdout.decode("utf-8")
        stdout_path = pathlib.Path(str(large_log) + ".stdout")
        stderr_path = pathlib.Path(str(large_log) + ".stderr")
        require("Cargo test confirmed: 1200 passed" in projected, "recognized test count missing")
        require("warning: controlled warning café retained" in projected, "warning disappeared from projection")
        require("fixture note: café 🐈 retained" in projected, "unknown multibyte line disappeared")
        require("Finished with important test diagnostics: retained" in projected, "unknown Cargo-prefix line disappeared")
        require(
            "raw combined log: " + str(large_log) in projected
            and "raw stdout: " + str(stdout_path) in projected
            and "raw stderr: " + str(stderr_path) in projected,
            "exact protected raw paths missing",
        )
        require(len(large.stdout) < 4096, "large known output was not reduced before model input")

        meta_path = pathlib.Path(str(large_log) + ".meta.json")
        combined = large_log.read_bytes()
        stdout_raw = stdout_path.read_bytes()
        stderr_raw = stderr_path.read_bytes()
        expected_stdout = (
            "    Finished `test` profile [unoptimized] target(s) in 0.02s\n"
            "     Running unittests src/lib.rs (target/debug/deps/example)\n"
            "running 1200 tests\n"
            + "".join(f"test fixture::case_{index:04} ... ok\n" for index in range(1200))
            + "fixture note: café 🐈 retained\n"
            + "Finished with important test diagnostics: retained\n"
            + "test result: ok. 1200 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.17s\n"
        ).encode("utf-8")
        metadata = json.loads(meta_path.read_text(encoding="utf-8"))
        require(stdout_raw == expected_stdout, "stdout original changed")
        require(stderr_raw == b"warning: controlled warning caf\xc3\xa9 retained\n", "stderr original changed")
        require(b"fixture::case_1199" in combined and b"warning: controlled warning" in combined, "combined CI log omitted raw streams")
        require(metadata["argv"] == [str(fake_cargo), "test", "large"], "exact argv identity was not retained")
        require(metadata["argvBytesHex"] == [os.fsencode(value).hex() for value in metadata["argv"]], "argv byte identity was not retained")
        require(metadata["cwd"] == str(root.resolve()), "working-directory source identity was not retained")
        producer = metadata["producerSource"]
        require(producer["helperSha256"] == hashlib.sha256(HELPER.read_bytes()).hexdigest(), "projection producer hash does not identify the tested helper")
        require(producer["git"]["state"] == "verified", "available Git source identity was not recorded")
        require(re.fullmatch(r"[0-9a-f]{40}", producer["git"]["head"]), "verified Git HEAD is malformed")
        require(re.fullmatch(r"[0-9a-f]{64}", producer["git"]["dirtyInputFingerprint"]), "dirty source fingerprint is malformed")
        require(metadata["projection"]["state"] == "projected", "projected state missing from metadata")
        require(metadata["streams"]["combined"]["sha256"] == hashlib.sha256(combined).hexdigest(), "combined raw digest mismatch")
        require(metadata["streams"]["stdout"]["sha256"] == hashlib.sha256(stdout_raw).hexdigest(), "stdout raw digest mismatch")
        require(metadata["streams"]["stderr"]["sha256"] == hashlib.sha256(stderr_raw).hexdigest(), "stderr raw digest mismatch")
        require(all(mode_is_private(path) for path in (large_log, stdout_path, stderr_path, meta_path, pathlib.Path(str(large_log) + ".exit"))), "retained output mode is not private")
        require(pathlib.Path(str(large_log) + ".exit").read_text() == "0\n", "exact successful status missing")

        read = subprocess.run(
            [sys.executable, str(HELPER), "read", str(large_log), "--stream", "stdout", "--offset", "0", "--limit", "32"],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        require(read.returncode == 0 and read.stdout == stdout_raw[:32], "bounded detail read was not exact")
        require(b"nextOffset=32" in read.stderr, "bounded read did not report its continuation offset")
        require(counter.read_bytes() == b"run\n", "raw detail retrieval replayed the child command")

        unknown_log = root / "unknown.log"
        unknown = invoke("unknown-tests", [fake_cargo, "test", "unknown"], unknown_log, cwd=root)
        require(unknown.returncode == 0, "unknown-format child status changed")
        unknown_text = unknown.stdout.decode("utf-8")
        require("unverified: unknown-tests" in unknown_text and "No test success is inferred" in unknown_text and "ok:" not in unknown_text, "unknown format fabricated success")
        require("future-format test record" in unknown_text, "unknown meaningful line disappeared")
        require(json.loads(pathlib.Path(str(unknown_log) + ".meta.json").read_text())["projection"]["state"] == "unverified", "unknown format not marked unverified")

        zero_log = root / "zero.log"
        zero = invoke("zero-tests", [fake_cargo, "test", "zero"], zero_log, cwd=root)
        require(zero.returncode == 0 and b"unverified: zero-tests" in zero.stdout and b"ok:" not in zero.stdout, "zero tests fabricated success")
        require(b"zero successful tests" in zero.stdout, "zero-test reason missing")

        binary_log = root / "binary.log"
        binary = invoke("binary-tests", [fake_cargo, "test", "binary"], binary_log, cwd=root)
        require(binary.returncode == 0 and b"unverified: binary-tests" in binary.stdout and b"ok:" not in binary.stdout, "invalid UTF-8 fabricated success")
        require(b"invalid UTF-8" in binary.stdout and b"base64" in binary.stdout, "invalid UTF-8 recovery missing")
        require(b"\xff" in pathlib.Path(str(binary_log) + ".stdout").read_bytes(), "binary stdout original changed")

        failure_log = root / "failure.log"
        failure = invoke("late-failure", [fake_cargo, "test", "failure"], failure_log, cwd=root)
        require(failure.returncode == 23, "late child failure status was not preserved")
        require(b"failed (23): late-failure" in failure.stderr and b"late failure detail" in failure.stderr and b"ok:" not in failure.stderr, "late failure detail was hidden or success was fabricated")
        require(pathlib.Path(str(failure_log) + ".exit").read_text() == "23\n", "late child status sidecar differs")

        generic = invoke("generic-success", ["/bin/sh", "-c", "printf 'ordinary generic output\\n'"], cwd=root)
        require(generic.returncode == 0 and generic.stdout == b"ok: generic-success\n", "generic success presentation changed")
        generic_failure = invoke(
            "generic-failure",
            [sys.executable, "-c", "import sys; print('generic stdout'); print('generic stderr', file=sys.stderr); sys.exit(17)"],
            root / "generic-failure.log",
            cwd=root,
        )
        require(generic_failure.returncode == 17 and b"failed (17)" in generic_failure.stderr, "generic failure status or presentation changed")

        protected_target = root / "protected-target"
        protected_target.write_bytes(b"do not replace\n")
        collision_log = root / "collision.log"
        pathlib.Path(str(collision_log) + ".stdout").symlink_to(protected_target)
        collision_counter = root / "collision-runs"
        collision = invoke(
            "collision",
            [fake_cargo, "test", "large"],
            collision_log,
            {"FIXTURE_RUN_COUNTER": collision_counter},
            root,
        )
        require(collision.returncode != 0, "pre-existing symlink output was accepted")
        require(protected_target.read_bytes() == b"do not replace\n", "pre-existing symlink target was overwritten")
        require(not collision_counter.exists(), "child ran despite output-path collision")
        require(not collision_log.exists(), "partial bundle was left after safe collision refusal")

        measurements = {
            "fixture": "large-success",
            "knownPassed": 1200,
            "combinedRawBytes": len(combined),
            "stdoutRawBytes": len(stdout_raw),
            "stderrRawBytes": len(stderr_raw),
            "projectedBytes": len(large.stdout),
            "combinedRawSha256": hashlib.sha256(combined).hexdigest(),
            "projectedSha256": hashlib.sha256(large.stdout).hexdigest(),
        }
        print(json.dumps({"result": "passed", "measurements": measurements}, sort_keys=True))


if __name__ == "__main__":
    main()
