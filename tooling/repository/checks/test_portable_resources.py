#!/usr/bin/env python3
"""Exercise the portable rollout metrics and checkout-bound target resources."""
from __future__ import annotations

import datetime
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import tracemalloc
import unittest

ROOT = Path(__file__).resolve().parents[3]
RESOURCES = ROOT / "skills/exitbind/resources"


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError("portable resource cannot be loaded")
    module = importlib.util.module_from_spec(spec)
    previous = sys.dont_write_bytecode
    sys.dont_write_bytecode = True
    try:
        spec.loader.exec_module(module)
    finally:
        sys.dont_write_bytecode = previous
    return module


METRICS_PATH = RESOURCES / "analysis/rollout-metrics.py"
metrics = load("portable_rollout_metrics", METRICS_PATH)
selector = load("portable_bind_target", RESOURCES / "verification/bind-target.py")


def record(kind, payload=None, timestamp=None):
    item = {"type": kind, "payload": payload}
    if timestamp is not None:
        item["timestamp"] = timestamp
    return json.dumps(item).encode() + b"\n"


def command(identity, exit_code=0, text="fixture secret", timestamp=None):
    return record("event_msg", {"type": "item_completed", "item": {
        "type": "CommandExecution", "id": identity, "exit_code": exit_code,
        "command": text, "stdout": "fixture secret"}}, timestamp)


class RolloutMetricsTests(unittest.TestCase):
    def test_custom_tool_call_only_reports_unknown_command_coverage(self):
        data = record("response_item", {"type": "custom_tool_call", "name": "private-tool",
                      "call_id": "private-call-id", "input": "tools.exec_command({cmd: 'printf private-input'})"})
        result = metrics.analyze(io.BytesIO(data))
        self.assertEqual(result["unclassified_tool_records"], 1)
        self.assertEqual(result["command_events"], 0)
        for field in ("dedupe_complete", "hourly_complete", "command_patterns_complete"):
            self.assertFalse(result[field], field)
        encoded = json.dumps(result)
        for private in ("private-tool", "private-call-id", "private-input", "exec_command"):
            self.assertNotIn(private, encoded)
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "private-rollout.jsonl"
            path.write_bytes(data)
            observed = subprocess.run([sys.executable, str(METRICS_PATH), str(path)],
                                      capture_output=True, text=True, timeout=3, check=False)
            self.assertEqual(observed.returncode, 0)
            self.assertEqual(json.loads(observed.stdout), dict(result, snapshot_bytes=len(data),
                                                               input_metadata_changed=False))
            for private in (str(path), "private-tool", "private-call-id", "private-input", "exec_command"):
                self.assertNotIn(private, observed.stdout + observed.stderr)

    def test_unclassified_tool_records_do_not_hide_observed_command(self):
        data = (record("response_item", {"type": "function_call", "arguments": "private-function-args"})
                + record("response_item", {"type": "function_call_output", "output": "private-result"})
                + record("response_item", {"type": "future_tool_call", "input": "private-future"})
                + command("visible-only-as-count", text="git status", timestamp="2026-11-03T12:00:00Z"))
        result = metrics.analyze(io.BytesIO(data))
        self.assertEqual(result["unclassified_tool_records"], 3)
        self.assertEqual(result["command_events"], 1)
        self.assertEqual(result["commands_per_hour"], {"2026-11-03T12:00Z": 1})
        self.assertEqual(result["command_families"]["version_control"], 1)
        for field in ("dedupe_complete", "hourly_complete", "command_patterns_complete"):
            self.assertFalse(result[field], field)
        encoded = json.dumps(result)
        for private in ("private-function-args", "private-result", "private-future", "visible-only-as-count"):
            self.assertNotIn(private, encoded)

    def test_aggregates_without_disclosing_content_or_summing_tokens(self):
        data = (record("session_meta", {"text": "fixture secret"}) + command("item-1", 3)
                + command("item-1", 3) + record("token_usage_record", {"input_tokens": 12345})
                + record("token_usage_record", {"input_tokens": 23456}) + record("compacted"))
        result = metrics.analyze(io.BytesIO(data))
        self.assertEqual(result["records"], 6)
        self.assertEqual(result["command_events"], 1)
        self.assertEqual(result["command_replays_in_window"], 1)
        self.assertEqual(result["command_nonzero_exit"], 1)
        self.assertIsNone(result["token_totals"])
        encoded = json.dumps(result)
        for omitted in ("fixture secret", "item-1", "12345", "23456"):
            self.assertNotIn(omitted, encoded)

    def test_deduplication_retains_exact_ids_and_reports_cap_overflow(self):
        result = metrics.analyze(io.BytesIO(command("a") + command("b") + command("c") + command("a")), window=2)
        self.assertEqual(result["command_event_records"], 4)
        self.assertEqual(result["command_events"], 2)
        self.assertEqual(result["command_replays_in_window"], 1)
        self.assertEqual(result["command_id_overflow"], 1)
        self.assertFalse(result["dedupe_complete"])
        self.assertEqual(result["dedupe_window"], 2)

    def test_hour_and_pattern_aggregates_distinguish_replays_from_repeats(self):
        data = (record("compacted")
                + command("first-id", text="git status --private-example", timestamp="2026-11-03T13:45:12+01:00")
                + command("second-id", text="git status --private-example", timestamp="2026-11-03T12:55:00Z")
                + command("first-id", text="git status --private-example", timestamp="2026-11-03T12:59:00Z")
                + command("third-id", text="rg private-example", timestamp="2026-11-03T13:05:00Z"))
        result = metrics.analyze(io.BytesIO(data))
        self.assertEqual(result["record_types"]["compacted"], 1)
        self.assertEqual(result["command_event_records"], 4)
        self.assertEqual(result["command_events"], 3)
        self.assertEqual(result["command_replays_in_window"], 1)
        self.assertEqual(result["commands_per_hour"], {"2026-11-03T12:00Z": 2, "2026-11-03T13:00Z": 1})
        self.assertEqual(result["command_families"]["version_control"], 2)
        self.assertEqual(result["command_families"]["search"], 1)
        self.assertEqual(result["repeated_command_patterns"], 1)
        self.assertEqual(result["repeated_command_excess"], 1)
        self.assertTrue(result["dedupe_complete"])
        self.assertTrue(result["hourly_complete"])
        self.assertTrue(result["command_patterns_complete"])
        self.assertIsNone(result["command_duration_seconds"])
        self.assertIsNone(result["token_totals"])
        encoded = json.dumps(result)
        for private in ("first-id", "second-id", "third-id", "private-example", "git status", "rg private"):
            self.assertNotIn(private, encoded)

    def test_missing_invalid_or_overlong_identity_and_command_remain_untracked(self):
        data = (command(None) + command("") + command("x" * 257) + command("\ud800")
                + command("valid-1", text="s" * 1025)
                + command("valid-2", text="\ud800") + command("valid-3", text=None))
        result = metrics.analyze(io.BytesIO(data))
        self.assertEqual(result["command_event_records"], 7)
        self.assertEqual(result["command_events"], 3)
        self.assertEqual(result["command_missing_id"], 2)
        self.assertEqual(result["command_invalid_id"], 2)
        self.assertEqual(result["command_text_untracked"], 3)
        self.assertFalse(result["dedupe_complete"])
        self.assertFalse(result["command_patterns_complete"])
        self.assertNotIn("valid-1", json.dumps(result))

    def test_hour_limit_and_invalid_timestamps_are_reported(self):
        start = datetime.datetime(2026, 1, 1, tzinfo=datetime.timezone.utc)
        data = b"".join(command(str(i), text="git status", timestamp=(start + datetime.timedelta(hours=i)).isoformat())
                        for i in range(metrics.MAX_HOURS + 1))
        data += command("missing-time", timestamp="private malformed timestamp")
        data += command("naive-time", timestamp="2026-01-01T12:00:00")
        result = metrics.analyze(io.BytesIO(data))
        self.assertEqual(len(result["commands_per_hour"]), metrics.MAX_HOURS)
        self.assertEqual(result["command_hour_overflow"], 1)
        self.assertEqual(result["command_hour_missing"], 2)
        self.assertFalse(result["hourly_complete"])
        self.assertNotIn("private malformed timestamp", json.dumps(result))

    def test_snapshot_limit_reads_the_first_record_and_no_later_bytes(self):
        first = command("first", text="cargo test", timestamp="2026-11-03T12:00:00Z")
        source = io.BytesIO(first + command("second", text="git status"))
        result = metrics.analyze(source, max_bytes=len(first))
        self.assertEqual(result["records"], 1)
        self.assertEqual(result["command_events"], 1)
        self.assertEqual(result["bytes_read"], len(first))
        self.assertEqual(source.tell(), len(first))

    def test_invalid_and_oversized_records_do_not_consume_the_next_record(self):
        data = b"x" * 300 + b"\ninvalid\n" + record("compacted")
        result = metrics.analyze(io.BytesIO(data), limit=64)
        self.assertEqual((result["oversized"], result["malformed"], result["records"]), (1, 1, 1))
        self.assertEqual(result["record_types"]["compacted"], 1)
        self.assertEqual(result["bytes_read"], len(data))

    def test_untrusted_types_and_missing_ids_are_not_exposed(self):
        result = metrics.analyze(io.BytesIO(record("fixture secret", {"id": "fixture secret"}) + command(None)))
        self.assertEqual(result["other_records"], 1)
        self.assertEqual(result["command_missing_id"], 1)
        self.assertNotIn("fixture secret", json.dumps(result))

    def test_memory_is_bounded_for_many_distinct_records(self):
        class RepeatingStream:
            remaining = 50000
            def readline(self, limit):
                if not self.remaining:
                    return b""
                self.remaining -= 1
                return command(str(self.remaining))
        tracemalloc.start()
        try:
            result = metrics.analyze(RepeatingStream(), window=16)
            _, peak = tracemalloc.get_traced_memory()
        finally:
            tracemalloc.stop()
        self.assertEqual(result["command_event_records"], 50000)
        self.assertEqual(result["command_events"], 16)
        self.assertEqual(result["command_id_overflow"], 49984)
        self.assertFalse(result["dedupe_complete"])
        self.assertLess(peak, 2_000_000)

    def test_maximum_retained_id_and_command_sizes_stay_bounded(self):
        class FullWindowStream:
            count = 0
            def readline(self, limit):
                if self.count == metrics.MAX_WINDOW:
                    return b""
                identity = f"{self.count:04d}" + "i" * (metrics.MAX_ID_BYTES - 4)
                text = f"{self.count:04d}" + "c" * (metrics.MAX_COMMAND_BYTES - 4)
                self.count += 1
                return command(identity, text=text)
        tracemalloc.start()
        try:
            result = metrics.analyze(FullWindowStream())
            _, peak = tracemalloc.get_traced_memory()
        finally:
            tracemalloc.stop()
        self.assertEqual(result["command_events"], metrics.MAX_WINDOW)
        self.assertTrue(result["dedupe_complete"])
        self.assertTrue(result["command_patterns_complete"])
        self.assertLess(peak, 16_000_000)

    @unittest.skipUnless(hasattr(os, "mkfifo"), "FIFO check requires a POSIX host")
    def test_cli_rejects_symlink_and_pipe_without_echoing_paths(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            data = root / "input.jsonl"
            data.write_bytes(record("compacted"))
            link = root / "link"
            link.symlink_to(data)
            pipe = root / "fifo"
            os.mkfifo(pipe)
            for source in (link, pipe):
                result = subprocess.run([sys.executable, str(METRICS_PATH), str(source)], capture_output=True,
                                        text=True, timeout=3, check=False)
                self.assertEqual(result.returncode, 2)
                self.assertNotIn(str(source), result.stderr)
            good = subprocess.run([sys.executable, str(METRICS_PATH), str(data)], capture_output=True,
                                  text=True, timeout=3, check=False)
            self.assertEqual(good.returncode, 0)
            observation = json.loads(good.stdout)
            self.assertEqual(observation["records"], 1)
            self.assertEqual(observation["snapshot_bytes"], data.stat().st_size)
            self.assertFalse(observation["input_metadata_changed"])
            self.assertNotIn(str(data), good.stdout)
            invalid = subprocess.run([sys.executable, str(METRICS_PATH), str(data),
                                      "--dedupe-window", "private-option-value"],
                                     capture_output=True, text=True, timeout=3, check=False)
            self.assertEqual(invalid.returncode, 2)
            self.assertNotIn(str(data), invalid.stderr)
            self.assertNotIn("private-option-value", invalid.stderr)


class BindTargetTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.checkout = self.root / "checkout"
        self.checkout.mkdir()
        (self.checkout / "Cargo.toml").write_text('[package]\nname = "fixture"\nversion = "0.1.0"\n')
        self.cache = self.root / "cache"
        self.cache.mkdir()
        digest = hashlib.sha256(os.fsencode(str(self.checkout.resolve()))).hexdigest()
        self.target = self.cache / ("cargo-" + digest)

    def test_empty_target_binds_idempotently_and_preserves_artifacts(self):
        first = selector.select(self.checkout, self.cache)
        self.assertEqual(first["target"], str(self.target))
        (self.target / "artifact").write_text("owned")
        self.assertEqual(selector.select(self.checkout, self.cache), first)
        self.assertEqual((self.target / "artifact").read_text(), "owned")

    def test_unmarked_nonempty_target_is_never_adopted(self):
        self.target.mkdir()
        (self.target / "foreign").write_text("keep")
        with self.assertRaises((OSError, ValueError)):
            selector.select(self.checkout, self.cache)
        self.assertEqual((self.target / "foreign").read_text(), "keep")
        self.assertFalse((self.target / selector.MARKER).exists())

    def test_foreign_malformed_and_duplicate_markers_are_refused(self):
        self.target.mkdir()
        marker = self.target / selector.MARKER
        for content in ('{}', '[]', '{"version":1,"version":1}', '{"version":true,"checkout":"x"}'):
            marker.write_text(content)
            with self.subTest(content=content), self.assertRaises((OSError, ValueError)):
                selector.select(self.checkout, self.cache)

    def test_symlink_target_marker_and_manifest_are_refused(self):
        outside = self.root / "outside"
        outside.mkdir()
        self.target.symlink_to(outside, target_is_directory=True)
        with self.assertRaises((OSError, ValueError)):
            selector.select(self.checkout, self.cache)
        self.target.unlink()
        self.target.mkdir()
        (self.target / selector.MARKER).symlink_to(self.checkout / "Cargo.toml")
        with self.assertRaises((OSError, ValueError)):
            selector.select(self.checkout, self.cache)
        (self.checkout / "Cargo.toml").unlink()
        (self.checkout / "Cargo.toml").symlink_to(outside / "Cargo.toml")
        with self.assertRaises((OSError, ValueError)):
            selector.select(self.checkout, self.cache)

    def test_distinct_checkout_uses_distinct_owned_target(self):
        first = selector.select(self.checkout, self.cache)
        other = self.root / "other"
        other.mkdir()
        (other / "Cargo.toml").write_text('[workspace]\n')
        second = selector.select(other, self.cache)
        self.assertNotEqual(first["target"], second["target"])


if __name__ == "__main__":
    unittest.main()
