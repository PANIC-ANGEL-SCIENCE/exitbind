"""Attempt and cross-workflow failure evidence for source-proof reuse."""
import copy
import unittest

import test_source_proof as support
from test_source_proof import (
    PROOF, SOURCE, OTHER, FakeProvider, current, find, job, run, source_proof, source_steps,
)


class SourceProofFailureTests(unittest.TestCase):
    def setUp(self):
        support.SourceProofTests.setUp(self)

    def test_reusable_caller_requires_its_tested_source_not_the_caller_head(self):
        api = FakeProvider()
        caller = run(5, "paired-prerelease.yml", OTHER, "main", "workflow_run")
        api.add_run(caller, [job("check / exit", source_steps())])
        source_proof(api, caller, self.current)
        api.statuses[SOURCE] = [
            {"context": "exitbind/paired-candidate", "state": "success", "created_at": "2026-11-03T00:00:06Z", "target_url": None},
            {"context": "exitbind/paired-candidate", "state": "pending", "created_at": "2026-11-03T00:00:05Z", "target_url": "https://github.com/fixture/repository/actions/runs/5"},
        ]
        self.assertEqual(find(api, self.current)["source"], SOURCE)
        unrelated = run(6, "paired-prerelease.yml", OTHER, "main", "workflow_run")
        unrelated["conclusion"] = "failure"
        api.add_run(unrelated, [])
        api.statuses[OTHER] = [{"context": "exitbind/paired-candidate", "state": "failure", "created_at": "2026-11-03T00:00:07Z", "target_url": None},
                               {"context": "exitbind/paired-candidate", "state": "pending", "created_at": "2026-11-03T00:00:06Z", "target_url": "https://github.com/fixture/repository/actions/runs/6"}]
        self.assertEqual(find(api, self.current)["runId"], 5)
        same_source = run(7, "paired-prerelease.yml", OTHER, "main", "workflow_run")
        same_source["conclusion"] = "failure"
        api.add_run(same_source, [])
        api.statuses[SOURCE] = [
            {"context": "exitbind/paired-candidate", "state": "failure", "created_at": "2026-11-03T00:00:08Z", "target_url": None},
            {"context": "exitbind/paired-candidate", "state": "pending", "created_at": "2026-11-03T00:00:07Z", "target_url": "https://github.com/fixture/repository/actions/runs/7"},
        ] + api.statuses[SOURCE]
        self.assertIsNone(find(api, self.current))
        self.assertIsNone(find(api, dict(self.current, source=OTHER)))


    def test_consumer_retry_never_reuses_an_older_origin(self):
        self.assertIsNotNone(find(self.api, self.current))
        self.assertIsNone(find(self.api, dict(self.current, attempt=2)))


    def test_newer_same_source_release_failure_or_pending_blocks_ci_origin(self):
        for state in ("failure", "pending"):
            with self.subTest(state=state):
                api = copy.deepcopy(self.api)
                later = run(2, "release.yml", SOURCE, "integration/pair-next", "workflow_dispatch")
                if state == "pending":
                    later.update(status="in_progress", conclusion=None)
                else:
                    later["conclusion"] = "failure"
                api.add_run(later, [])
                self.assertIsNone(find(api, self.current))
        unrelated = run(2, "release.yml", OTHER, "integration/pair-other", "workflow_dispatch")
        unrelated["conclusion"] = "failure"
        self.api.add_run(unrelated, [])
        self.assertEqual(find(self.api, self.current)["runId"], 1)
        old_failure = run(2, "release.yml", SOURCE, "integration/pair-old", "workflow_dispatch")
        old_failure["conclusion"] = "failure"
        self.api.add_run(old_failure, [])
        self.run["created_at"] = self.run["updated_at"] = "2026-11-03T00:00:03Z"
        self.assertEqual(find(self.api, self.current)["runId"], 1)
        old_failure["updated_at"] = "2026-11-03T00:00:04Z"
        self.assertIsNone(find(self.api, self.current))


    def test_malformed_same_source_workflow_evidence_refuses(self):
        for case in ("missing-owner", "missing-time", "missing-sha", "missing-conclusion"):
            with self.subTest(case=case):
                api = copy.deepcopy(self.api)
                later = run(2, "release.yml", SOURCE, "integration/pair-next", "workflow_dispatch")
                if case == "missing-owner":
                    later["conclusion"] = "failure"
                    del later["head_repository"]
                elif case == "missing-time":
                    del later["updated_at"]
                elif case == "missing-sha":
                    del later["head_sha"]
                else:
                    del later["conclusion"]
                api.add_run(later, [])
                self.assertIsNone(find(api, self.current))


    def test_source_scoped_paired_status_fence_excludes_only_own_pending(self):
        def status(state, second, run_id=None):
            return {"id": second, "context": "exitbind/paired-candidate", "state": state,
                    "created_at": f"2026-11-03T00:00:{second:02d}Z",
                    "target_url": None if run_id is None else
                                  f"https://github.com/fixture/repository/actions/runs/{run_id}"}
        for state in ("failure", "pending"):
            with self.subTest(state=state):
                self.api.statuses[SOURCE] = [status(state, 3, 2 if state == "pending" else None)]
                self.assertIsNone(find(self.api, self.current))
        self.api.statuses[SOURCE] = [status("pending", 4, self.current["runId"]),
                                     status("failure", 3), status("pending", 2, 2)]
        self.assertIsNone(find(self.api, self.current))
        self.api.statuses[SOURCE] = [status("pending", 4, self.current["runId"])]
        self.assertEqual(find(self.api, self.current)["runId"], 1)
        self.api.statuses[SOURCE] = [status("success", 3), status("pending", 2, 2)]
        self.assertEqual(find(self.api, self.current)["runId"], 1)
        self.api.statuses[SOURCE] = [status("failure", 4, self.current["runId"])]
        self.assertIsNone(find(self.api, self.current))
        self.api.statuses[SOURCE] = [status("pending", 3, self.current["runId"]), status("pending", 2, 2)]
        self.assertIsNone(find(self.api, self.current))
        foreign = status("pending", 3, self.current["runId"])
        foreign["target_url"] = "https://github.com/other/repository/actions/runs/99"
        self.api.statuses[SOURCE] = [foreign]
        self.assertIsNone(find(self.api, self.current))
        self.api.statuses[SOURCE] = [status("failure", 0)]
        self.assertEqual(find(self.api, self.current)["runId"], 1)
        self.api.statuses[SOURCE] = [status("success", 0)] * 100
        self.assertIsNone(find(self.api, self.current))
        self.api.statuses.clear()
        self.api.statuses[OTHER] = [status("failure", 3)]
        self.assertEqual(find(self.api, self.current)["runId"], 1)


    def test_status_lookup_failure_never_authorizes_reuse(self):
        original = self.api.get
        def unavailable(path):
            if path.startswith("commits/"):
                raise OSError("unavailable")
            return original(path)
        self.api.get = unavailable
        self.assertIsNone(find(self.api, self.current))


    def test_release_requires_full_readiness_and_native_matching_platform(self):
        api = FakeProvider()
        ready = run(7, "release.yml", SOURCE, "integration/pair-test", "workflow_dispatch")
        names = ["channel", "linux", "macos-aarch64-apple-darwin", "macos-x86_64-apple-darwin", "windows-wsl"]
        mac = current("macOS", "ARM64")
        all_jobs = [job(name) for name in names]
        all_jobs[2] = job(names[2], source_steps("native", linux=False))
        all_jobs.append(job("stable-readiness-result", ["Require every platform before recording stable readiness", "Upload stable readiness"]))
        api.add_run(ready, all_jobs)
        source_proof(api, ready, mac, "native")
        api.add_artifact(ready, f"stable-readiness-{SOURCE}-1", "stable-readiness.json", {
            "source": SOURCE, "runId": "7", "runAttempt": "1", "ref": "refs/heads/integration/pair-test",
            "allNativePlatformsAndWsl": True, "published": False, "mainChanged": False})
        self.assertIsNotNone(find(api, mac, "release"))
        self.assertEqual(PROOF.decision(find(api, mac, "release"), mac)["ci_package"], "false")
        self.assertIsNone(find(api, mac, "ci"))
        self.assertIsNone(find(api, current("macOS", "X64"), "release"))
        newer_ci = run(8)
        newer_ci.update(status="in_progress", conclusion=None)
        api.add_run(newer_ci, [])
        self.assertIsNone(find(api, mac, "release"))
        api.runs["ci.yml"].pop()
        api.job_list[7][4]["conclusion"] = "failure"
        self.assertIsNone(find(api, mac, "release"))

