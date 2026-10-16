#!/usr/bin/env python3
"""Focused regression for quiet-command capture, projection and raw recovery."""

import hashlib
import json
import os
import pathlib
import re
import shlex
import stat
import subprocess
import sys
import tempfile


ROOT = pathlib.Path(__file__).resolve().parents[3]
WRAPPER = ROOT / "scripts" / "quiet-command.sh"
HELPER = ROOT / "tooling" / "repository" / "ci" / "quiet-command.py"
MODEL_LIMIT = 16 * 1024


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
    sys.stdout.write("long diagnostic begins: " + ("café 🐈" * 4000) + " long diagnostic ends\n")
    sys.stdout.flush()
    sys.stderr.write("warning: controlled warning café retained\n")
    sys.stderr.flush()
    sys.stdout.write("test result: ok. 1200 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.17s\n")
elif scenario == "clean-success":
    sys.stdout.write("    Finished `test` profile [unoptimized] target(s) in 0.02s\n")
    sys.stdout.write("     Running unittests src/lib.rs (target/debug/deps/example)\n")
    sys.stdout.write("running 1 test\n")
    sys.stdout.write("test fixture::only_case ... ok\n")
    sys.stdout.write("test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n")
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
elif scenario == "unknown-suffix":
    sys.stdout.write("running 1 test\n")
    sys.stdout.write("test fixture::case ... ok, warning: controlled diagnostic\n")
    sys.stdout.write("test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n")
elif scenario == "malformed-duration":
    sys.stdout.write("running 1 test\n")
    sys.stdout.write("test fixture::case ... ok\n")
    sys.stdout.write("test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in MALFORMED warning: retained?\n")
elif scenario == "ignored":
    sys.stdout.write("running 2 tests\n")
    sys.stdout.write("test fixture::active ... ok\n")
    sys.stdout.write("test fixture::external ... ignored, requires a configured external service\n")
    sys.stdout.write("test result: ok. 1 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.01s\n")
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


def full_paths_text(log):
    return (
        "raw combined log: " + shlex.quote(str(log)) + "\n"
        + "raw stdout: " + shlex.quote(str(log) + ".stdout") + "\n"
        + "raw stderr: " + shlex.quote(str(log) + ".stderr") + "\n"
        + "source metadata: " + shlex.quote(str(log) + ".meta.json") + "\n"
    )


def compact_paths_text():
    return "raw stdout, stderr and metadata use the combined log's .stdout, .stderr and .meta.json suffixes\n"


def read_hint_text(log):
    command = [
        sys.executable,
        str(HELPER.resolve()),
        "read",
        str(log.resolve()),
        "--stream",
        "combined",
        "--offset",
        "0",
        "--limit",
        "8192",
    ]
    return "bounded detail read: " + " ".join(shlex.quote(value) for value in command) + "\n"


def compact_success_label(log):
    count_without_label = "ok:  (Cargo test confirmed: 1 passed, 0 failed, 0 ignored, 0 filtered, 1 suites)\n"
    explicit_bytes = len((count_without_label + full_paths_text(log)).encode("utf-8"))
    compact_bytes = len((count_without_label + compact_paths_text() + read_hint_text(log)).encode("utf-8"))
    label_bytes = max(1, MODEL_LIMIT - explicit_bytes + 1)
    require(explicit_bytes + label_bytes > MODEL_LIMIT, "clean-success label did not overflow explicit paths")
    require(compact_bytes + label_bytes <= MODEL_LIMIT, "clean-success label leaves no room for compact paths and read route")
    return "x" * label_bytes, explicit_bytes + label_bytes, compact_bytes + label_bytes


def compact_fallback_label(log, raw, reason):
    shared = "No test success is inferred. Read the protected original with the bounded command below.\n"
    suffix = "; Cargo output was not projected (" + reason + "); child exit=0\n"
    summary = "combined bytes: " + str(len(raw)) + " sha256: " + hashlib.sha256(raw).hexdigest() + "\n"
    prefix = "unverified: "
    trailer = read_hint_text(log).encode("utf-8") + b"raw excerpt:\n"

    def fixed_header(path_details):
        return (
            prefix.encode("utf-8")
            + suffix.encode("utf-8")
            + shared.encode("utf-8")
            + path_details.encode("utf-8")
            + summary.encode("utf-8")
            + trailer
        )

    explicit_header = fixed_header(full_paths_text(log))
    compact_header = fixed_header(compact_paths_text())
    label_bytes = max(1, MODEL_LIMIT - len(explicit_header) + 1)
    explicit_with_label = len(explicit_header) + label_bytes
    compact_with_label_and_raw = len(compact_header) + label_bytes + len(raw) + 1
    require(explicit_with_label > MODEL_LIMIT, "ambiguous label did not overflow explicit path framing")
    require(compact_with_label_and_raw <= MODEL_LIMIT, "ambiguous label leaves no room for compact paths, read route and raw excerpt")
    return "x" * label_bytes, explicit_with_label, compact_with_label_and_raw


def main():
    with tempfile.TemporaryDirectory(prefix="quiet-command-regression-") as temporary:
        root = pathlib.Path(temporary)
        fake_cargo = root / "cargo"
        write_cargo_fixture(fake_cargo)
        counter = root / "large-runs"
        long_log_dir = (
            root
            / ("workspace-" + "w" * 60)
            / ("checkout data-" + "c" * 80)
            / ("test logs-" + "l" * 60)
        )
        long_log_dir.mkdir(parents=True)
        large_log = long_log_dir / "large.log"

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
            "raw combined log: " + shlex.quote(str(large_log)) in projected
            and "raw stdout: " + shlex.quote(str(stdout_path)) in projected
            and "raw stderr: " + shlex.quote(str(stderr_path)) in projected,
            "exact protected raw paths missing",
        )
        require(len(large.stdout) <= 16 * 1024, "large known output exceeded the model-input byte limit")
        require("long diagnostic begins:" in projected and "long diagnostic ends" in projected, "bounded excerpt lost its diagnostic boundaries")
        require("[omitted " in projected, "large diagnostic was not explicitly marked as clipped")
        require(projected.endswith("\n"), "bounded output omitted its final newline")
        next_stage = large.stdout + b"next stage: value-proof\n"
        require(next_stage.splitlines()[-1] == b"next stage: value-proof", "following stage was concatenated onto the projection footer")
        require(next_stage.splitlines()[-2].startswith(b"bounded detail read: "), "read route is not on its own complete line")

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
            + "long diagnostic begins: " + ("café 🐈" * 4000) + " long diagnostic ends\n"
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

        read_route_line = next(line for line in projected.splitlines() if line.startswith("bounded detail read: "))
        expected_route = [
            sys.executable,
            str(HELPER.resolve()),
            "read",
            str(large_log.resolve()),
            "--stream",
            "combined",
            "--offset",
            "0",
            "--limit",
            "8192",
        ]
        require(
            read_route_line == "bounded detail read: " + " ".join(shlex.quote(value) for value in expected_route),
            "complete executable read route was clipped or changed",
        )
        route_read = subprocess.run(
            shlex.split(read_route_line[len("bounded detail read: ") :]),
            cwd=root,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        require(route_read.returncode == 0 and route_read.stdout == combined[:8192], "printed read route did not retrieve exact retained bytes")
        require(b"nextOffset=8192" in route_read.stderr, "printed read route omitted its continuation metadata")

        long_path_dir = root
        path_depth = 16
        path_max = int(os.pathconf(root, "PC_PATH_MAX"))
        name_max = int(os.pathconf(root, "PC_NAME_MAX"))
        require(path_max > 0 and name_max > 0, "filesystem path limits are unavailable")
        target_log_path_bytes = min(4075, path_max - len(os.fsencode(".meta.json")) - 1)
        component_bytes = (
            target_log_path_bytes
            - len(os.fsencode(root))
            - len(os.fsencode("/clean-success.log"))
            - path_depth
        ) // path_depth
        require(1 <= component_bytes <= name_max, "filesystem root cannot support the bounded path fixture")
        component = "c" * component_bytes
        for _ in range(path_depth):
            long_path_dir = long_path_dir / component
        long_path_dir.mkdir(parents=True)
        long_path_log = long_path_dir / "ambiguous.log"
        long_path_counter = root / "ambiguous-runs"
        unknown_raw = (
            "running 1 test\n"
            "future-format test record: status=ok\n"
            "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n"
        ).encode("utf-8")
        unknown_reason = "test records did not match the reported suite counts"
        unknown_label, fallback_explicit_bytes, fallback_compact_bytes = compact_fallback_label(
            long_path_log, unknown_raw, unknown_reason
        )
        long_path_unknown = invoke(
            unknown_label,
            [fake_cargo, "test", "unknown"],
            long_path_log,
            {"FIXTURE_RUN_COUNTER": long_path_counter},
            root,
        )
        long_path_text = long_path_unknown.stdout.decode("utf-8")
        long_path_bytes = len(os.fsencode(long_path_log))
        require(long_path_bytes > 0 and long_path_bytes <= target_log_path_bytes, "path fixture exceeds the selected filesystem limit")
        for suffix in (".stdout", ".stderr", ".meta.json", ".exit"):
            require(len(os.fsencode(str(long_path_log) + suffix)) < path_max, "retained sidecar exceeds the filesystem path limit")
        require(long_path_unknown.returncode == 0, "long-path child status changed")
        require(fallback_explicit_bytes > MODEL_LIMIT and fallback_compact_bytes <= MODEL_LIMIT, "fallback label pressure did not bracket the model limit")
        require(len(long_path_unknown.stdout) <= 16 * 1024 and long_path_unknown.stdout.endswith(b"\n"), "compact path fallback exceeded its bound or lost the final newline")
        require(("unverified: " + unknown_label).encode("utf-8") in long_path_unknown.stdout, "long-path parse uncertainty label was not retained")
        require("raw stdout, stderr and metadata use the combined log's" in long_path_text, "compact path explanation was not selected")
        require("future-format test record" in long_path_text, "long-path fallback hid meaningful raw output")
        require("raw combined log:" not in long_path_text, "optional repeated full paths displaced the bounded read route")
        require(long_path_counter.read_bytes() == b"run\n", "long-path fixture executed more than once")
        long_path_combined = long_path_log.read_bytes()
        long_route_line = next(line for line in long_path_text.splitlines() if line.startswith("bounded detail read: "))
        long_route_bytes = len(long_route_line.encode("utf-8")) + 1
        long_route_args = shlex.split(long_route_line[len("bounded detail read: ") :])
        require(
            long_route_args == [
                sys.executable,
                str(HELPER.resolve()),
                "read",
                str(long_path_log.resolve()),
                "--stream",
                "combined",
                "--offset",
                "0",
                "--limit",
                "8192",
            ],
            "long-path route executable or protected source identity changed",
        )
        long_route_read = subprocess.run(long_route_args, cwd=root, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        require(long_route_read.returncode == 0 and long_route_read.stdout == long_path_combined, "compact framing did not preserve exact long-path retrieval")
        require(long_path_counter.read_bytes() == b"run\n", "bounded long-path retrieval replayed the Cargo command")

        clean_log = long_path_dir / "clean-success.log"
        clean_counter = root / "clean-success-runs"
        clean_label, success_explicit_bytes, success_compact_bytes = compact_success_label(clean_log)
        clean_success = invoke(
            clean_label,
            [fake_cargo, "test", "clean-success"],
            clean_log,
            {"FIXTURE_RUN_COUNTER": clean_counter},
            root,
        )
        clean_text = clean_success.stdout.decode("utf-8")
        clean_combined = clean_log.read_bytes()
        require(clean_success.returncode == 0, "recognized no-detail child status changed")
        require(b"Cargo test confirmed: 1 passed, 0 failed, 0 ignored" in clean_success.stdout, "recognized no-detail test count missing")
        require(("ok: " + clean_label).encode("ascii") in clean_success.stdout, "compact no-detail summary lost its producer label")
        require(success_explicit_bytes > MODEL_LIMIT and success_compact_bytes <= MODEL_LIMIT, "clean-success pressure calculation did not bracket the model limit")
        require("raw stdout, stderr and metadata use the combined log's" in clean_text, "long-path no-detail output did not use compact path framing")
        require("raw combined log:" not in clean_text and "bounded detail read: " in clean_text, "compact no-detail framing omitted the combined path read route")
        require(len(clean_success.stdout) <= 16 * 1024 and clean_success.stdout.endswith(b"\n"), "long-path no-detail output exceeded its bound or lost the final newline")
        clean_route_line = next(line for line in clean_text.splitlines() if line.startswith("bounded detail read: "))
        clean_route_args = shlex.split(clean_route_line[len("bounded detail read: ") :])
        require(
            clean_route_args == [
                sys.executable,
                str(HELPER.resolve()),
                "read",
                str(clean_log.resolve()),
                "--stream",
                "combined",
                "--offset",
                "0",
                "--limit",
                "8192",
            ],
            "no-detail read route does not preserve the exact command and combined log",
        )
        clean_route_read = subprocess.run(clean_route_args, cwd=root, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        require(clean_route_read.returncode == 0 and clean_route_read.stdout == clean_combined, "no-detail compact framing did not retrieve exact raw output")
        require(clean_counter.read_bytes() == b"run\n", "no-detail bounded read replayed the Cargo command")
        clean_metadata = json.loads(pathlib.Path(str(clean_log) + ".meta.json").read_text(encoding="utf-8"))
        require(clean_metadata["projection"]["state"] == "projected", "recognized no-detail run was not marked projected")

        short_clean_log = root / "short-clean-success.log"
        short_clean = invoke("short-path-clean-success", [fake_cargo, "test", "clean-success"], short_clean_log, cwd=root)
        short_clean_text = short_clean.stdout.decode("utf-8")
        require(short_clean.returncode == 0 and b"Cargo test confirmed: 1 passed" in short_clean.stdout, "short-path no-detail success changed")
        require("raw combined log: " + shlex.quote(str(short_clean_log)) in short_clean_text, "short-path output omitted the explicit combined path")
        require("bounded detail read: " not in short_clean_text, "short-path no-detail success gained an unnecessary footer")

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

        suffix_log = root / "unknown-suffix.log"
        suffix = invoke("unknown-suffix", [fake_cargo, "test", "unknown-suffix"], suffix_log, cwd=root)
        require(suffix.returncode == 0, "unknown suffix child status changed")
        require(b"unverified: unknown-suffix" in suffix.stdout and b"No test success is inferred" in suffix.stdout, "unknown success suffix was promoted to confirmed success")
        require(b"ok: unknown-suffix" not in suffix.stdout and b"ok, warning: controlled diagnostic" in suffix.stdout, "unknown success suffix was discarded")
        require(json.loads(pathlib.Path(str(suffix_log) + ".meta.json").read_text())["projection"]["state"] == "unverified", "unknown suffix was not marked unverified")

        duration_log = root / "malformed-duration.log"
        duration = invoke("malformed-duration", [fake_cargo, "test", "malformed-duration"], duration_log, cwd=root)
        require(duration.returncode == 0, "malformed-duration child status changed")
        require(b"unverified: malformed-duration" in duration.stdout and b"No test success is inferred" in duration.stdout, "malformed summary duration was promoted to confirmed success")
        require(b"ok: malformed-duration" not in duration.stdout and b"finished in MALFORMED warning: retained?" in duration.stdout, "malformed summary line was discarded")
        require(json.loads(pathlib.Path(str(duration_log) + ".meta.json").read_text())["projection"]["state"] == "unverified", "malformed duration was not marked unverified")

        ignored_log = root / "ignored.log"
        ignored = invoke("ignored-reason", [fake_cargo, "test", "ignored"], ignored_log, cwd=root)
        require(ignored.returncode == 0, "supported ignored-reason child status changed")
        require(b"Cargo test confirmed: 1 passed, 0 failed, 1 ignored" in ignored.stdout, "supported ignored reason changed recognized counts")
        require(b"ignored, requires a configured external service" in ignored.stdout, "supported ignored reason disappeared")

        oversized_label = "long-label-" + ("x" * (16 * 1024))
        oversized_log = root / "oversized-label.log"
        oversized = invoke(oversized_label, [fake_cargo, "test", "ignored"], oversized_log, cwd=root)
        require(oversized.returncode == 0, "child status changed when an optional label exceeded the output limit")
        require(len(oversized.stdout) <= 16 * 1024 and oversized.stdout.endswith(b"\n"), "compact label fallback exceeded the limit or lost its newline")
        require(b"unverified:" in oversized.stdout and b"No test success is inferred" in oversized.stdout, "omitted optional label framing was not disclosed")
        require(b"Cargo test confirmed" not in oversized.stdout, "optional label clipping falsely presented success")
        oversized_route = next(line for line in oversized.stdout.decode("utf-8").splitlines() if line.startswith("bounded detail read: "))
        oversized_route_read = subprocess.run(shlex.split(oversized_route[len("bounded detail read: ") :]), stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        require(oversized_route_read.returncode == 0 and oversized_route_read.stdout == pathlib.Path(str(oversized_log)).read_bytes(), "optional oversized label displaced the complete read route")

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

        # Inject TERM after real child creation but before Popen returns. The
        # capture must retain that signal, reap its child and record its status.
        early_log = root / "early-interruption.log"
        early_driver = r'''
import importlib.util, os, signal, sys
sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("quiet", sys.argv[1])
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
original = module.subprocess.Popen
def launch(*args, **kwargs):
    child = original(*args, **kwargs)
    if kwargs.get("start_new_session"):
        os.kill(os.getpid(), signal.SIGTERM)
    return child
module.subprocess.Popen = launch
os.environ["QUIET_COMMAND_LOG"] = sys.argv[2]
raise SystemExit(module.run("early-interruption", [sys.executable, "-c", "import time; time.sleep(30)"]))
'''
        early = subprocess.run(
            [sys.executable, "-c", early_driver, str(HELPER), str(early_log)],
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=15,
        )
        require(early.returncode == 143, "early interruption lost the child status")
        early_meta = json.loads(pathlib.Path(str(early_log) + ".meta.json").read_text())
        require(early_meta["state"] == "complete" and early_meta["childReturnCode"] == -15,
                "early interruption left a live or unknown child")
        require(pathlib.Path(str(early_log) + ".exit").read_text() == "143\n",
                "early interruption did not retain status")

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
            "readRouteExecutedWithoutReplay": True,
            "combinedRawSha256": hashlib.sha256(combined).hexdigest(),
            "projectedSha256": hashlib.sha256(large.stdout).hexdigest(),
            "longPathFallback": {
                "logPathBytes": long_path_bytes,
                "fullReadRouteBytes": long_route_bytes,
                "pathCopiesLowerBoundBytes": 5 * long_path_bytes,
                "filesystemPathMax": path_max,
                "filesystemNameMax": name_max,
                "labelBytes": len(unknown_label),
                "explicitFramingBytes": fallback_explicit_bytes,
                "compactFramingAndRawBytes": fallback_compact_bytes,
                "projectedBytes": len(long_path_unknown.stdout),
                "rawCombinedBytes": len(long_path_combined),
                "completeReadRouteExecutedWithoutReplay": True,
                "rawCombinedSha256": hashlib.sha256(long_path_combined).hexdigest(),
            },
            "longPathCleanSuccess": {
                "logPathBytes": len(os.fsencode(clean_log)),
                "labelBytes": len(clean_label),
                "explicitFramingBytes": success_explicit_bytes,
                "compactFramingAndReadRouteBytes": success_compact_bytes,
                "projectedBytes": len(clean_success.stdout),
                "rawCombinedBytes": len(clean_combined),
                "completeReadRouteExecutedWithoutReplay": True,
                "rawCombinedSha256": hashlib.sha256(clean_combined).hexdigest(),
            },
        }
        print(json.dumps({"result": "passed", "measurements": measurements}, sort_keys=True))


if __name__ == "__main__":
    main()
