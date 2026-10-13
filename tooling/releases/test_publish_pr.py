import json
import os
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

import publish_pr as publisher
import test_sync_docs


class DocumentationPR(unittest.TestCase):
    def setUp(self):
        self.fixture = test_sync_docs.DocumentationVersions()
        self.fixture.setUp()
        self.addCleanup(self.fixture.doCleanups)
        self.root = self.fixture.root
        self.previous_cwd = Path.cwd()
        os.chdir(self.root)
        self.addCleanup(os.chdir, self.previous_cwd)
        self.environment = patch.dict(os.environ, {
            "GITHUB_REPOSITORY": publisher.REPOSITORY,
            "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1"})
        self.environment.start()
        self.addCleanup(self.environment.stop)
        self.actual_git = publisher.git
        self.actual_run = publisher.run
        self.actual_subprocess_run = subprocess.run
        self.actual_git("init", "-b", "main")
        self.actual_git("config", "user.name", "Fixture")
        self.actual_git("config", "user.email", "fixture@example.invalid")
        self.actual_git("add", ".")
        self.actual_git("commit", "-m", "fixture")
        self.base = self.actual_git("rev-parse", "HEAD")
        self.remote = Path(self.fixture.temp.name) / "remote.git"
        self.actual_git("init", "--bare", str(self.remote))
        self.actual_git("remote", "add", "origin", str(self.remote))
        self.actual_git("push", "origin", "main")
        self.opened = []
        self.creates = 0
        self.pushes = 0
        self.create_outcome = "success"
        self.before_push = None

        def git(*args):
            if args == ("remote", "get-url", "origin"):
                return f"https://github.com/{publisher.REPOSITORY}.git"
            if args[0] == "push":
                self.assertNotIn("--force", args)
                self.assertTrue(args[-1].endswith(":" + publisher.REF))
                self.pushes += 1
                if self.before_push:
                    self.before_push()
            return self.actual_git(*args)

        def run(*args):
            if args[0] == "gh":
                self.assertEqual(args[1:3], ("pr", "list"))
                for item in self.opened:
                    item["headRefOid"] = self.branch_head()
                return json.dumps(self.opened)
            return self.actual_run(*args)

        def execute(args, **kwargs):
            if args[0] != "gh":
                return self.actual_subprocess_run(args, **kwargs)
            self.assertEqual(args[1:3], ["pr", "create"])
            self.assertIn("--draft", args)
            self.assertIn("--body-file", args)
            self.creates += 1
            if self.create_outcome != "rejected":
                self.opened.append(dict(number=1, baseRefName="main", headRefName=publisher.BRANCH,
                                        headRefOid=self.branch_head(), url="https://github.com/example/pr/1"))
            return subprocess.CompletedProcess(args, 0 if self.create_outcome == "success" else 1)

        for target, replacement in (("git", git), ("run", run)):
            handle = patch.object(publisher, target, replacement)
            handle.start()
            self.addCleanup(handle.stop)
        handle = patch.object(publisher.subprocess, "run", execute)
        handle.start()
        self.addCleanup(handle.stop)

    def branch_head(self):
        rows = self.actual_git("ls-remote", "origin", publisher.REF).splitlines()
        return rows[0].split()[0] if rows else None

    def fresh_render(self, version=None):
        self.actual_git("reset", "--hard", self.base)
        self.fixture.render(version or self.fixture.next)

    def test_create_repeat_and_next_version_reuse_one_pr_without_main_mutation(self):
        self.fresh_render()
        publisher.publish()
        first = self.branch_head()
        self.fresh_render()
        publisher.publish()
        self.assertEqual(self.branch_head(), first)
        self.assertEqual((self.pushes, self.creates), (1, 1))
        next_version = self.fixture.next.rsplit(".", 1)[0] + ".99"
        self.fresh_render(next_version)
        publisher.publish()
        self.assertNotEqual(self.branch_head(), first)
        self.assertTrue(publisher.ancestor(first, self.branch_head()))
        self.assertEqual((self.pushes, self.creates), (2, 1))
        self.assertEqual(publisher.remote_head("refs/heads/main"), self.base)

    def test_concurrent_main_changes_are_preserved_in_branch_parents_and_tree(self):
        self.fresh_render()
        publisher.publish()
        first = self.branch_head()
        self.actual_git("reset", "--hard", self.base)
        (self.root / "concurrent.txt").write_text("Owner's independent change\n")
        self.actual_git("add", "concurrent.txt")
        self.actual_git("commit", "-m", "concurrent main")
        self.base = self.actual_git("rev-parse", "HEAD")
        self.actual_git("push", "origin", "main")
        self.fresh_render()
        publisher.publish()
        head = self.branch_head()
        self.assertTrue(publisher.ancestor(first, head))
        self.assertTrue(publisher.ancestor(self.base, head))
        self.assertEqual(publisher.blob(head, "concurrent.txt"), "Owner's independent change\n")
        self.assertEqual(self.creates, 1)

    def test_foreign_base_or_owner_edits_are_held_without_a_push(self):
        self.fresh_render()
        self.opened = [dict(number=1, baseRefName="fix/fr45-residuals", headRefName=publisher.BRANCH)]
        with self.assertRaisesRegex(ValueError, "another base"):
            publisher.publish()
        self.assertEqual(self.pushes, 0)
        self.opened = []
        self.actual_git("reset", "--hard", self.base)
        (self.root / "README.md").write_text((self.root / "README.md").read_text() + "Owner text\n")
        self.actual_git("add", "README.md")
        self.actual_git("commit", "-m", "owner edit")
        self.actual_git("push", "origin", "HEAD:" + publisher.REF)
        self.fresh_render()
        with self.assertRaisesRegex(ValueError, "non-version"):
            publisher.publish()
        self.assertEqual(self.pushes, 0)

    def test_non_version_worktree_changes_cannot_be_published(self):
        self.fresh_render()
        (self.root / "README.md").write_text((self.root / "README.md").read_text() + "Unexpected text\n")
        with self.assertRaisesRegex(ValueError, "non-version"):
            publisher.publish()
        self.assertEqual(self.pushes, 0)

    def test_rejected_push_and_uncertain_pr_creation_do_not_blindly_retry(self):
        hook = self.remote / "hooks/pre-receive"
        hook.write_text("#!/bin/sh\necho fixture-protection >&2\nexit 1\n")
        hook.chmod(0o755)
        self.fresh_render()
        with self.assertRaises(subprocess.CalledProcessError):
            publisher.publish()
        self.assertEqual(self.creates, 0)
        hook.unlink()
        self.fresh_render()
        self.create_outcome = "uncertain-success"
        publisher.publish()
        self.assertEqual(self.creates, 1)

    def test_concurrent_branch_push_is_not_overwritten(self):
        self.fresh_render()
        def advance():
            tree = self.actual_git("rev-parse", self.base + "^{tree}")
            self.racing_head = self.actual_git("commit-tree", tree, "-p", self.base, "-m", "concurrent")
            self.actual_git("push", "origin", self.racing_head + ":" + publisher.REF)
        self.before_push = advance
        with self.assertRaises(subprocess.CalledProcessError):
            publisher.publish()
        self.assertEqual(self.branch_head(), self.racing_head)
        self.assertEqual(self.creates, 0)

    def test_merged_branch_allows_a_later_release_and_failed_pr_retains_branch(self):
        self.fresh_render()
        publisher.publish()
        self.base = self.branch_head()
        self.actual_git("push", "origin", self.base + ":refs/heads/main")
        self.opened = []
        self.fresh_render(self.fixture.next.rsplit(".", 1)[0] + ".99")
        self.create_outcome = "rejected"
        with self.assertRaisesRegex(ValueError, "PR creation failed"):
            publisher.publish()
        self.assertTrue(publisher.ancestor(self.base, self.branch_head()))
        self.assertEqual(self.creates, 2)

    def test_squash_merged_implementation_is_reused_only_after_incorporation(self):
        (self.root / "implementation.py").write_text("raise RuntimeError('must never execute')\n")
        self.actual_git("add", "implementation.py")
        self.actual_git("commit", "-m", "automation implementation")
        implementation = self.actual_git("rev-parse", "HEAD")
        self.actual_git("push", "origin", implementation + ":" + publisher.REF)
        tree = self.actual_git("rev-parse", implementation + "^{tree}")
        self.base = self.actual_git("commit-tree", tree, "-p", self.base, "-m", "squash reviewed implementation")
        self.actual_git("push", "origin", self.base + ":refs/heads/main")
        self.fresh_render()
        publisher.publish()
        self.assertTrue(publisher.ancestor(implementation, self.branch_head()))
        self.assertTrue(publisher.ancestor(self.base, self.branch_head()))
        self.assertEqual(publisher.blob(self.branch_head(), "implementation.py"),
                         "raise RuntimeError('must never execute')\n")

    def test_unchanged_main_does_not_create_branch_or_pr(self):
        publisher.publish()
        self.assertEqual((self.pushes, self.creates), (0, 0))

    def test_workflow_runs_trusted_main_with_only_job_scoped_write_permissions(self):
        workflow = (test_sync_docs.ROOT / ".github/workflows/release.yml").read_text()
        before_jobs = workflow.split("jobs:", 1)[0]
        self.assertIn("permissions:\n  contents: read", before_jobs)
        self.assertNotIn("pull_request", before_jobs)
        job = workflow.split("  documentation:\n", 1)[1].split("  installed:\n", 1)[0]
        self.assertIn("ref: main", job)
        self.assertIn("contents: write", job)
        self.assertIn("pull-requests: write", job)
        self.assertNotIn("ref: automation/", job)
        self.assertNotIn("secrets.", job)


if __name__ == "__main__":
    unittest.main()
