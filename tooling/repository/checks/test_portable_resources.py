#!/usr/bin/env python3
"""Exercise the portable rollout metrics and checkout-bound target resources."""
from __future__ import annotations

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


def record(kind, payload=None):
    return json.dumps({"type": kind, "payload": payload}).encode() + b"\n"


def command(identity, exit_code=0):
    return record("event_msg", {"type": "item_completed", "item": {
        "type": "CommandExecution", "id": identity, "exit_code": exit_code,
        "command": "fixture secret", "stdout": "fixture secret"}})


class RolloutMetricsTests(unittest.TestCase):
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

    def test_deduplication_is_limited_to_the_declared_window(self):
        result = metrics.analyze(io.BytesIO(command("a") + command("b") + command("c") + command("a")), window=2)
        self.assertEqual(result["command_events"], 4)
        self.assertEqual(result["command_replays_in_window"], 0)
        self.assertEqual(result["dedupe_window"], 2)

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
        self.assertEqual(result["command_events"], 50000)
        self.assertLess(peak, 2_000_000)

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
            self.assertEqual(json.loads(good.stdout)["records"], 1)
            self.assertNotIn(str(data), good.stdout)


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
