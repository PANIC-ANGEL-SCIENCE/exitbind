#!/usr/bin/env python3
# exitbind-managed-publication:v1
"""Stream aggregate rollout metrics without retaining or printing event content."""
import argparse
from collections import OrderedDict
import hashlib
import json
import os
import stat
import sys

MAX_RECORD = 4 * 1024 * 1024
MAX_WINDOW = 4096
TYPES = frozenset(("session_meta", "turn_context", "response_item", "event_msg", "compacted", "token_usage_record"))


def analyze(stream, limit=MAX_RECORD, window=MAX_WINDOW):
    counts = {kind: 0 for kind in sorted(TYPES)}
    result = {"bytes_read": 0, "records": 0, "malformed": 0, "oversized": 0,
              "other_records": 0, "record_types": counts, "command_events": 0,
              "command_nonzero_exit": 0, "command_replays_in_window": 0,
              "command_missing_id": 0, "dedupe_window": window,
              "token_totals": None}
    seen = OrderedDict()
    while True:
        raw = stream.readline(limit + 1)
        if not raw:
            break
        result["bytes_read"] += len(raw)
        if len(raw) > limit:
            result["oversized"] += 1
            while not raw.endswith(b"\n"):
                raw = stream.readline(limit + 1)
                if not raw:
                    break
                result["bytes_read"] += len(raw)
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
        event_id = command.get("id")
        if isinstance(event_id, str) and event_id:
            key = hashlib.sha256(event_id.encode("utf-8")).digest()
            if key in seen:
                result["command_replays_in_window"] += 1
                seen.move_to_end(key)
                continue
            seen[key] = None
            if len(seen) > window:
                seen.popitem(last=False)
        else:
            result["command_missing_id"] += 1
        result["command_events"] += 1
        exit_code = command.get("exit_code")
        if isinstance(exit_code, int) and not isinstance(exit_code, bool) and exit_code != 0:
            result["command_nonzero_exit"] += 1
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("rollout", help="Explicit path to one JSONL rollout")
    parser.add_argument("--record-limit", type=int, default=MAX_RECORD,
                        help="Maximum bytes in one record (64 through 4194304)")
    parser.add_argument("--dedupe-window", type=int, default=MAX_WINDOW,
                        help="Recent command IDs to recognize (1 through 4096)")
    args = parser.parse_args()
    if not 64 <= args.record_limit <= MAX_RECORD or not 1 <= args.dedupe_window <= MAX_WINDOW:
        parser.error("record limit or dedupe window outside supported bounds")
    if not hasattr(os, "O_NOFOLLOW"):
        print("rollout metrics: secure file opening is unavailable", file=sys.stderr)
        return 2
    try:
        fd = os.open(args.rollout, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
        with os.fdopen(fd, "rb") as stream:
            if not stat.S_ISREG(os.fstat(stream.fileno()).st_mode):
                raise OSError("not a regular file")
            result = analyze(stream, args.record_limit, args.dedupe_window)
    except OSError:
        print("rollout metrics: cannot open regular input file", file=sys.stderr)
        return 2
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
