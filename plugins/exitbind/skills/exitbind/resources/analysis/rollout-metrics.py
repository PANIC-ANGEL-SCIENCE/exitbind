#!/usr/bin/env python3
# exitbind-managed-publication:v1
"""Stream bounded aggregate rollout metrics without publishing event content."""
import argparse
import datetime
import json
import os
import re
import stat
import sys

MAX_RECORD = 4 * 1024 * 1024
MAX_WINDOW = 4096
MAX_ID_BYTES = 256
MAX_COMMAND_BYTES = 1024
MAX_HOURS = 744
TYPES = frozenset(("session_meta", "turn_context", "response_item", "event_msg", "compacted", "token_usage_record"))
FAMILIES = ("build", "file_read", "governance", "other", "script", "search", "session", "shell", "version_control")


def bounded_text(value, maximum):
    if not isinstance(value, str) or not value or len(value) > maximum:
        return False
    try:
        return len(value.encode("utf-8")) <= maximum
    except UnicodeEncodeError:
        return False


def utc_hour(value):
    if not isinstance(value, str) or not 1 <= len(value) <= 40:
        return None
    try:
        parsed = datetime.datetime.fromisoformat(value[:-1] + "+00:00" if value.endswith("Z") else value)
        if parsed.tzinfo is None or parsed.utcoffset() is None:
            return None
        parsed = parsed.astimezone(datetime.timezone.utc)
        return f"{parsed.year:04}-{parsed.month:02}-{parsed.day:02}T{parsed.hour:02}:00Z"
    except (ValueError, OverflowError):
        return None


def command_family(command):
    match = re.match(r"\s*([^\s;&|]+)", command)
    if not match:
        return "other"
    executable = re.split(r"[/\\]", match.group(1).strip("'\""))[-1]
    if executable in ("cargo", "rustc", "swift", "xcodebuild"):
        return "build"
    if executable in ("cat", "head", "less", "sed", "tail"):
        return "file_read"
    if executable == "exitbind":
        return "governance"
    if executable in ("node", "bun") or re.fullmatch(r"python(?:3(?:\.\d+)?)?", executable):
        return "script"
    if executable in ("find", "grep", "rg"):
        return "search"
    if executable == "tmux":
        return "session"
    if executable in ("bash", "sh", "zsh"):
        return "shell"
    if executable == "git":
        return "version_control"
    return "other"


def analyze(stream, limit=MAX_RECORD, window=MAX_WINDOW, max_bytes=None):
    counts = {kind: 0 for kind in sorted(TYPES)}
    result = {"bytes_read": 0, "records": 0, "malformed": 0, "oversized": 0,
              "other_records": 0, "record_types": counts, "command_event_records": 0,
              "command_events": 0,
              "command_nonzero_exit": 0, "command_replays_in_window": 0,
              "command_exit_unknown": 0, "command_missing_id": 0,
              "command_invalid_id": 0, "command_id_overflow": 0,
              "dedupe_window": window, "dedupe_mode": "bounded_exact_set",
              "command_hour_missing": 0, "command_hour_overflow": 0,
              "hour_bin_limit": MAX_HOURS, "commands_per_hour": {},
              "command_text_untracked": 0,
              "command_byte_limit": MAX_COMMAND_BYTES,
              "command_families": {family: 0 for family in FAMILIES},
              "command_duration_seconds": None, "token_totals": None}
    seen = set()
    commands = {}

    def read_chunk():
        size = limit + 1
        if max_bytes is not None:
            size = min(size, max_bytes - result["bytes_read"])
        if size <= 0:
            return b""
        chunk = stream.readline(size)
        result["bytes_read"] += len(chunk)
        return chunk

    while True:
        raw = read_chunk()
        if not raw:
            break
        if len(raw) > limit:
            result["oversized"] += 1
            while not raw.endswith(b"\n"):
                raw = read_chunk()
                if not raw:
                    break
            continue
        try:
            item = json.loads(raw)
        except (ValueError, UnicodeError, RecursionError):
            result["malformed"] += 1
            continue
        if not isinstance(item, dict):
            result["malformed"] += 1
            continue
        result["records"] += 1
        kind = item.get("type")
        if isinstance(kind, str) and kind in TYPES:
            counts[kind] += 1
        else:
            result["other_records"] += 1
        payload = item.get("payload")
        if kind != "event_msg" or not isinstance(payload, dict) or payload.get("type") != "item_completed":
            continue
        command = payload.get("item")
        if not isinstance(command, dict) or (command.get("item_type") or command.get("type")) != "CommandExecution":
            continue
        result["command_event_records"] += 1
        event_id = command.get("id")
        if not isinstance(event_id, str) or not event_id:
            result["command_missing_id"] += 1
            continue
        if not bounded_text(event_id, MAX_ID_BYTES):
            result["command_invalid_id"] += 1
            continue
        if event_id in seen:
            result["command_replays_in_window"] += 1
            continue
        if len(seen) >= window:
            result["command_id_overflow"] += 1
            continue
        seen.add(event_id)
        result["command_events"] += 1
        exit_code = command.get("exit_code")
        if not isinstance(exit_code, int) or isinstance(exit_code, bool):
            result["command_exit_unknown"] += 1
        elif exit_code != 0:
            result["command_nonzero_exit"] += 1
        hour = utc_hour(item.get("timestamp"))
        hours = result["commands_per_hour"]
        if hour is None:
            result["command_hour_missing"] += 1
        elif hour in hours or len(hours) < MAX_HOURS:
            hours[hour] = hours.get(hour, 0) + 1
        else:
            result["command_hour_overflow"] += 1
        text = command.get("command")
        if not bounded_text(text, MAX_COMMAND_BYTES):
            result["command_text_untracked"] += 1
            continue
        result["command_families"][command_family(text)] += 1
        commands[text] = commands.get(text, 0) + 1
    result["dedupe_complete"] = not any(result[key] for key in
        ("malformed", "oversized", "other_records", "command_missing_id", "command_invalid_id", "command_id_overflow"))
    result["hourly_complete"] = result["dedupe_complete"] and not (
        result["command_hour_missing"] or result["command_hour_overflow"])
    result["command_patterns_complete"] = result["dedupe_complete"] and not result["command_text_untracked"]
    result["repeated_command_patterns"] = sum(count > 1 for count in commands.values())
    result["repeated_command_excess"] = sum(count - 1 for count in commands.values() if count > 1)
    return result


class SafeParser(argparse.ArgumentParser):
    def error(self, _message):
        self.exit(2, "rollout metrics: invalid arguments\n")


def main():
    parser = SafeParser(prog="rollout-metrics.py", description=__doc__)
    parser.add_argument("rollout", help="Explicit path to one JSONL rollout")
    parser.add_argument("--record-limit", type=int, default=MAX_RECORD,
                        help="Maximum bytes in one record (64 through 4194304)")
    parser.add_argument("--dedupe-window", type=int, default=MAX_WINDOW,
                        help="Maximum distinct command IDs to track (1 through 4096)")
    args = parser.parse_args()
    if not 64 <= args.record_limit <= MAX_RECORD or not 1 <= args.dedupe_window <= MAX_WINDOW:
        parser.error("record limit or dedupe window outside supported bounds")
    if not hasattr(os, "O_NOFOLLOW"):
        print("rollout metrics: secure file opening is unavailable", file=sys.stderr)
        return 2
    try:
        fd = os.open(args.rollout, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
        with os.fdopen(fd, "rb") as stream:
            initial = os.fstat(stream.fileno())
            if not stat.S_ISREG(initial.st_mode):
                raise OSError("not a regular file")
            result = analyze(stream, args.record_limit, args.dedupe_window, initial.st_size)
            final = os.fstat(stream.fileno())
            result["snapshot_bytes"] = initial.st_size
            result["input_metadata_changed"] = (
                initial.st_size != final.st_size or initial.st_mtime_ns != final.st_mtime_ns
                or result["bytes_read"] != initial.st_size
            )
            if result["input_metadata_changed"]:
                result["dedupe_complete"] = False
                result["hourly_complete"] = False
                result["command_patterns_complete"] = False
    except OSError:
        print("rollout metrics: cannot open regular input file", file=sys.stderr)
        return 2
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
