#!/usr/bin/env python3
"""Capture command output once and project only recognized Cargo test records."""

import argparse
import base64
import collections
import hashlib
import json
import os
import pathlib
import re
import selectors
import shlex
import subprocess
import sys
import tempfile


MAX_MODEL_BYTES = 16 * 1024
MAX_FAILURE_BYTES = 8 * 1024
MAX_DETAIL_BYTES = 64 * 1024
RUNNING = re.compile(r"running (\d+) tests?\Z")
TEST = re.compile(r"test (.+) \.\.\. (ok|ignored|FAILED)(?:, .*)?\Z")
RESULT = re.compile(
    r"test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; "
    r"(\d+) ignored; (\d+) measured; (\d+) filtered out; finished in .+\Z"
)
PACKAGE = r"[A-Za-z0-9][A-Za-z0-9_-]*"
VERSION = r"v[0-9]+\.[0-9]+\.[0-9]+(?:-[A-Za-z0-9.-]+)?(?:\+[A-Za-z0-9.-]+)?"
NOISE = (
    re.compile(rf"(?:Compiling|Checking|Fresh) {PACKAGE} {VERSION}(?: \(.+\))?"),
    re.compile(rf"(?:Downloading|Downloaded) {PACKAGE} {VERSION}"),
    re.compile(r"Locking [0-9]+ packages? to latest compatible versions"),
    re.compile(r"Updating crates\.io index"),
    re.compile(r"Finished `(?:dev|test|release|check|doc|bench|build)` profile \[[^\]\r\n]+\] target\(s\) in [0-9]+(?:\.[0-9]+)?s"),
    re.compile(r"Finished (?:dev|test|release|check|doc|bench|build) \[[^\]\r\n]+\] target\(s\) in [0-9]+(?:\.[0-9]+)?s"),
    re.compile(r"Running (?:unittests|tests|examples|benchmarks|doc-tests) \S+ \(target/(?:debug|release)/(?:deps/)?[A-Za-z0-9._-]+\)"),
    re.compile(rf"Doc-tests {PACKAGE}"),
)


def log_paths(log):
    return {
        "combined": log,
        "stdout": pathlib.Path(str(log) + ".stdout"),
        "stderr": pathlib.Path(str(log) + ".stderr"),
        "meta": pathlib.Path(str(log) + ".meta.json"),
        "exit": pathlib.Path(str(log) + ".exit"),
    }


def create_bundle(requested_log):
    temporary_root = None
    if requested_log:
        requested = pathlib.Path(requested_log)
        if not requested.is_absolute():
            requested = pathlib.Path.cwd() / requested
        if not requested.name:
            raise OSError("QUIET_COMMAND_LOG must name a file")
        log = requested.parent.resolve(strict=True) / requested.name
    else:
        temporary_root = pathlib.Path(
            tempfile.mkdtemp(prefix="quiet-command-", dir=os.environ.get("TMPDIR") or None)
        )
        os.chmod(temporary_root, 0o700)
        log = temporary_root / "combined.log"

    paths = log_paths(log)
    handles = {}
    created = []
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
    flags |= getattr(os, "O_NOFOLLOW", 0)
    try:
        for name, path in paths.items():
            fd = os.open(str(path), flags, 0o600)
            os.fchmod(fd, 0o600)
            metadata = os.fstat(fd)
            created.append((path, metadata.st_dev, metadata.st_ino))
            handles[name] = os.fdopen(fd, "wb", buffering=0)
    except Exception:
        for handle in handles.values():
            handle.close()
        for path, device, inode in created:
            try:
                current = path.lstat()
                if current.st_dev == device and current.st_ino == inode:
                    path.unlink()
            except OSError:
                pass
        if temporary_root is not None:
            try:
                temporary_root.rmdir()
            except OSError:
                pass
        raise
    return paths, handles, temporary_root


def write_json(handle, value):
    data = (json.dumps(value, ensure_ascii=True, indent=2, sort_keys=True) + "\n").encode("utf-8")
    handle.seek(0)
    handle.truncate()
    handle.write(data)
    handle.flush()
    os.fsync(handle.fileno())


def shell_status(return_code):
    if return_code >= 0:
        return return_code
    return min(255, 128 - return_code)


def capture(argv, handles):
    child_env = os.environ.copy()
    child_env.pop("QUIET_COMMAND_LOG", None)
    try:
        child = subprocess.Popen(argv, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=child_env)
    except OSError as error:
        message = ("quiet-command: could not execute command: " + str(error) + "\n").encode(
            "utf-8", "backslashreplace"
        )
        handles["stderr"].write(message)
        handles["combined"].write(message)
        return 127, False

    selector = selectors.DefaultSelector()
    streams = {"stdout": child.stdout, "stderr": child.stderr}
    try:
        for name, stream in streams.items():
            os.set_blocking(stream.fileno(), False)
            selector.register(stream, selectors.EVENT_READ, name)
        while selector.get_map():
            try:
                events = selector.select()
            except InterruptedError:
                continue
            for key, _ in events:
                try:
                    data = os.read(key.fileobj.fileno(), 65536)
                except BlockingIOError:
                    continue
                if not data:
                    selector.unregister(key.fileobj)
                    key.fileobj.close()
                    continue
                handles[key.data].write(data)
                handles["combined"].write(data)
        return child.wait(), True
    except BaseException:
        if child.poll() is None:
            child.terminate()
        child.wait()
        raise
    finally:
        selector.close()


def sha256_file(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def source_identity():
    helper = pathlib.Path(__file__).resolve()
    repository = helper.parent.parent
    identity = {
        "helperPath": str(helper),
        "helperSha256": sha256_file(helper),
        "git": {"state": "unknown", "root": str(repository)},
        "candidateSha": os.environ.get("CANDIDATE_SHA") or None,
    }
    try:
        def git(*arguments):
            return subprocess.run(
                ["git", "-C", str(repository), *arguments],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )

        top = git("rev-parse", "--show-toplevel")
        head = git("rev-parse", "--verify", "HEAD")
        status = git("status", "--porcelain=v1", "--untracked-files=all", "-z")
        diff = git("diff", "--binary", "HEAD", "--")
        untracked = git("ls-files", "--others", "--exclude-standard", "-z")
        if any(result.returncode != 0 for result in (top, head, status, diff, untracked)):
            identity["git"]["reason"] = "Git source identity unavailable"
            return identity

        root = pathlib.Path(os.fsdecode(top.stdout.rstrip(b"\n"))).resolve()
        dirty = hashlib.sha256()
        dirty.update(b"status\0" + status.stdout)
        dirty.update(b"diff\0" + diff.stdout)
        for raw_path in untracked.stdout.split(b"\0"):
            if not raw_path:
                continue
            path = root / os.fsdecode(raw_path)
            dirty.update(b"untracked\0" + raw_path + b"\0")
            try:
                metadata = path.lstat()
                dirty.update(str(metadata.st_mode & 0o7777).encode("ascii") + b"\0")
                if path.is_symlink():
                    dirty.update(b"symlink\0" + os.fsencode(os.readlink(path)))
                elif path.is_file():
                    with path.open("rb") as stream:
                        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                            dirty.update(chunk)
                else:
                    dirty.update(b"non-regular\0")
            except OSError:
                dirty.update(b"unavailable\0")

        head_text = head.stdout.decode("ascii", "strict").strip()
        identity["git"] = {
            "state": "verified",
            "root": str(root),
            "head": head_text,
            "dirtyInputFingerprint": dirty.hexdigest(),
        }
        candidate = identity["candidateSha"]
        if candidate:
            identity["candidateMatchesHead"] = candidate == head_text
    except (OSError, UnicodeError, ValueError):
        identity["git"]["reason"] = "Git source identity unavailable"
    return identity


def file_bytes(path):
    with path.open("rb") as stream:
        return stream.read()


def cargo_test(argv):
    return bool(argv) and pathlib.Path(argv[0]).name in ("cargo", "cargo.exe") and len(argv) > 1 and argv[1] == "test"


def parse_cargo_output(raw):
    try:
        lines = raw.decode("utf-8").splitlines(keepends=True)
    except UnicodeDecodeError:
        return None, b"", "invalid UTF-8"

    suites = []
    details = []
    expected = None
    observed = collections.Counter()
    uncertain = None

    for line in lines:
        body = line.rstrip("\r\n")
        match = RUNNING.fullmatch(body)
        if match:
            if expected is not None:
                uncertain = "a test suite began before the prior suite completed"
                break
            expected = int(match.group(1))
            observed = collections.Counter()
            continue

        match = TEST.fullmatch(body)
        if match:
            if expected is None:
                uncertain = "a test record appeared outside a recognized test suite"
                break
            observed[match.group(2)] += 1
            continue

        match = RESULT.fullmatch(body)
        if match:
            if expected is None:
                uncertain = "a result summary appeared without a recognized test suite"
                break
            values = [int(match.group(index)) for index in range(2, 7)]
            passed, failed, ignored, measured, filtered = values
            actual = observed["ok"] + observed["FAILED"] + observed["ignored"]
            if actual != expected or passed != observed["ok"] or failed != observed["FAILED"] or ignored != observed["ignored"]:
                uncertain = "test records did not match the reported suite counts"
                break
            if expected != passed + failed + ignored + measured:
                uncertain = "running count did not match the recognized result counts"
                break
            if match.group(1) != "ok" or failed:
                uncertain = "the Cargo result summary reported failure"
                break
            suites.append(
                {"passed": passed, "failed": failed, "ignored": ignored, "measured": measured, "filtered": filtered}
            )
            expected = None
            observed = collections.Counter()
            continue

        if body.startswith("running ") or body.startswith("test result:") or body.startswith("test "):
            uncertain = "unrecognized Cargo test record or summary"
            break
        if not body or any(pattern.fullmatch(body.lstrip()) for pattern in NOISE):
            continue
        details.append(line.encode("utf-8"))

    if uncertain:
        return None, b"", uncertain
    if expected is not None:
        return None, b"", "Cargo output ended before a test result summary"
    if not suites:
        return None, b"", "no recognized Cargo test result summary"

    counts = {key: sum(suite[key] for suite in suites) for key in ("passed", "failed", "ignored", "measured", "filtered")}
    if counts["passed"] == 0:
        return None, b"", "zero successful tests were reported"
    return {"suites": len(suites), **counts}, b"".join(details), None


def clipped_utf8(data, limit, prefix):
    if len(data) <= limit:
        return data
    if prefix:
        chunk = data[:limit]
        while chunk:
            try:
                chunk.decode("utf-8")
                break
            except UnicodeDecodeError as error:
                chunk = chunk[: error.start]
        return chunk
    chunk = data[-limit:]
    while chunk:
        try:
            chunk.decode("utf-8")
            break
        except UnicodeDecodeError as error:
            chunk = chunk[error.end :]
    return chunk


def paths_text(paths):
    return (
        "raw combined log: " + shlex.quote(str(paths["combined"])) + "\n"
        + "raw stdout: " + shlex.quote(str(paths["stdout"])) + "\n"
        + "raw stderr: " + shlex.quote(str(paths["stderr"])) + "\n"
        + "source metadata: " + shlex.quote(str(paths["meta"])) + "\n"
    )


def output_projection(label, paths, counts, details):
    summary = (
        "ok: " + label + " (Cargo test confirmed: " + str(counts["passed"]) + " passed, "
        + str(counts["failed"]) + " failed, " + str(counts["ignored"]) + " ignored, "
        + str(counts["filtered"]) + " filtered, " + str(counts["suites"]) + " suites)\n"
        + paths_text(paths)
    ).encode("utf-8")
    if not details:
        sys.stdout.buffer.write(summary)
        return len(summary)

    header = summary + b"additional output retained verbatim:\n"
    available = max(0, MAX_MODEL_BYTES - len(header) - 100)
    if len(details) <= available:
        rendered = details
    else:
        first_size = available // 2
        last_size = available - first_size
        first = clipped_utf8(details, first_size, True)
        last = clipped_utf8(details, last_size, False)
        omitted = max(0, len(details) - len(first) - len(last))
        rendered = first + ("\n[omitted " + str(omitted) + " bytes of additional output; use the bounded read command below]\n").encode("utf-8") + last
    read_hint = (
        "bounded detail read: python3 scripts/quiet-command.py read "
        + shlex.quote(str(paths["combined"]))
        + " --stream combined --offset 0 --limit 8192\n"
    ).encode("utf-8")
    output = header + rendered + (b"\n" if not rendered.endswith(b"\n") else b"") + read_hint
    if len(output) > MAX_MODEL_BYTES:
        output = output[:MAX_MODEL_BYTES]
    sys.stdout.buffer.write(output)
    return len(output)


def fallback_projection(label, paths, raw, reason, status):
    digest = sha256_file(paths["combined"])
    header = (
        "unverified: " + label + "; Cargo output was not projected (" + reason + "); child exit=" + str(status) + "\n"
        + "No test success is inferred. Read the protected original with the bounded command below.\n"
        + paths_text(paths)
        + "combined bytes: " + str(len(raw)) + " sha256: " + digest + "\n"
        + "bounded detail read: python3 scripts/quiet-command.py read "
        + shlex.quote(str(paths["combined"]))
        + " --stream combined --offset 0 --limit 8192\n"
    ).encode("utf-8")
    limit = max(0, MAX_MODEL_BYTES - len(header) - 100)
    try:
        raw.decode("utf-8")
        if len(raw) <= limit:
            excerpt = raw
        else:
            first = clipped_utf8(raw, limit // 2, True)
            last = clipped_utf8(raw, limit - len(first), False)
            omitted = len(raw) - len(first) - len(last)
            excerpt = first + ("\n[omitted " + str(omitted) + " bytes; protected raw file is complete]\n").encode() + last
    except UnicodeDecodeError:
        encoded = base64.b64encode(raw[: min(4096, max(0, limit // 2))])
        excerpt = b"raw prefix (base64):\n" + encoded + b"\n"
    output = header + b"raw excerpt:\n" + excerpt
    if len(output) > MAX_MODEL_BYTES:
        output = output[:MAX_MODEL_BYTES]
    sys.stdout.buffer.write(output)
    return len(output)


def failure_output(label, paths, status, show_all=False):
    raw = file_bytes(paths["combined"])
    excerpt = raw if show_all else raw[-MAX_FAILURE_BYTES:]
    header = (
        "failed (" + str(status) + "): " + label + "; full output is retained\n"
        + paths_text(paths)
        + ("complete combined output:\n" if show_all else "combined tail (" + str(len(excerpt)) + " bytes):\n")
    ).encode("utf-8")
    sys.stderr.buffer.write(header + excerpt + (b"\n" if excerpt and not excerpt.endswith(b"\n") else b""))


def run(label, argv):
    if not argv:
        sys.stderr.write("quiet-command: COMMAND is required\n")
        return 2
    requested_log = os.environ.get("QUIET_COMMAND_LOG", "")
    paths, handles, temporary_root = create_bundle(requested_log)
    cwd = os.getcwd()
    metadata = {
        "version": 1,
        "state": "running",
        "label": label,
        "argv": argv,
        "argvBytesHex": [os.fsencode(value).hex() for value in argv],
        "cwd": cwd,
        "cwdBytesHex": os.fsencode(cwd).hex(),
        "producerSource": source_identity(),
        "paths": {key: str(value) for key, value in paths.items()},
        "childReturnCode": None,
        "shellExitCode": None,
        "launched": None,
    }
    write_json(handles["meta"], metadata)
    try:
        return_code, launched = capture(argv, handles)
        command_status = shell_status(return_code)
        handles["exit"].write((str(command_status) + "\n").encode("ascii"))
        handles["exit"].flush()
        os.fsync(handles["exit"].fileno())
        for name in ("combined", "stdout", "stderr"):
            handles[name].flush()
            os.fsync(handles[name].fileno())
            handles[name].close()
        for name in ("meta", "exit"):
            handles[name].flush()
        combined = file_bytes(paths["combined"])
        metadata.update(
            {
                "state": "complete",
                "childReturnCode": return_code if launched else None,
                "shellExitCode": command_status,
                "launched": launched,
                "streams": {
                    name: {"bytes": paths[name].stat().st_size, "sha256": sha256_file(paths[name])}
                    for name in ("combined", "stdout", "stderr")
                },
            }
        )

        if command_status != 0:
            metadata["projection"] = {"state": "not_projected", "reason": "nonzero child exit"}
            write_json(handles["meta"], metadata)
            failure_output(label, paths, command_status, show_all=temporary_root is not None and not cargo_test(argv))
            return command_status

        if cargo_test(argv):
            counts, details, reason = parse_cargo_output(combined)
            if reason:
                metadata["projection"] = {"state": "unverified", "reason": reason}
                write_json(handles["meta"], metadata)
                fallback_projection(label, paths, combined, reason, command_status)
            else:
                metadata["projection"] = {"state": "projected", "counts": counts, "additionalOutputBytes": len(details)}
                write_json(handles["meta"], metadata)
                output_projection(label, paths, counts, details)
            return command_status

        metadata["projection"] = {"state": "not_applicable"}
        write_json(handles["meta"], metadata)
        if temporary_root is not None:
            for path in paths.values():
                path.unlink()
            temporary_root.rmdir()
        sys.stdout.write("ok: " + label + "\n")
        return command_status
    finally:
        for handle in handles.values():
            try:
                handle.close()
            except OSError:
                pass


def read_detail(args):
    maximum = MAX_DETAIL_BYTES
    if args.limit < 1 or args.limit > maximum:
        sys.stderr.write("quiet-command: --limit must be between 1 and " + str(maximum) + " bytes\n")
        return 2
    if args.offset < 0:
        sys.stderr.write("quiet-command: --offset must be nonnegative\n")
        return 2
    path = pathlib.Path(args.path)
    if args.stream == "stdout":
        path = pathlib.Path(str(path) + ".stdout")
    elif args.stream == "stderr":
        path = pathlib.Path(str(path) + ".stderr")
    try:
        size = path.stat().st_size
        with path.open("rb") as stream:
            stream.seek(args.offset)
            data = stream.read(args.limit)
    except OSError as error:
        sys.stderr.write("quiet-command: cannot read retained output: " + str(error) + "\n")
        return 1
    sys.stdout.buffer.write(data)
    next_offset = args.offset + len(data)
    sys.stderr.write(
        "stream=" + args.stream + " bytes=" + str(len(data)) + " offset=" + str(args.offset)
        + " total=" + str(size) + " nextOffset=" + str(next_offset) + " complete="
        + ("true" if next_offset >= size else "false") + "\n"
    )
    return 0


def main(argv):
    if len(argv) >= 2 and argv[1] == "run":
        if len(argv) < 4:
            sys.stderr.write("usage: quiet-command.py run LABEL COMMAND [ARG ...]\n")
            return 2
        label = argv[2]
        if not re.fullmatch(r"[A-Za-z0-9._-]+", label):
            sys.stderr.write("quiet-command label must be portable\n")
            return 2
        try:
            return run(label, argv[3:])
        except (OSError, ValueError) as error:
            sys.stderr.write("quiet-command: " + str(error) + "\n")
            return 1
    if len(argv) >= 2 and argv[1] == "read":
        parser = argparse.ArgumentParser(prog="quiet-command.py read")
        parser.add_argument("path", help="combined log path reported by quiet-command")
        parser.add_argument("--stream", choices=("combined", "stdout", "stderr"), default="combined")
        parser.add_argument("--offset", type=int, default=0)
        parser.add_argument("--limit", type=int, default=8192)
        try:
            return read_detail(parser.parse_args(argv[2:]))
        except SystemExit as error:
            return int(error.code)
    sys.stderr.write(
        "usage: quiet-command.py run LABEL COMMAND [ARG ...] | read LOG [--stream combined|stdout|stderr] [--offset N] [--limit N]\n"
    )
    return 2


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
