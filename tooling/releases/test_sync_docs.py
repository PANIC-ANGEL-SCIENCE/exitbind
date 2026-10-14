import copy
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import tomllib
import unittest

from sync_docs import INVENTORY, POLICY, REPOSITORY, TARGETS, plan, published_tag, verify_release

ROOT = Path(__file__).resolve().parents[2]


def release(tag):
    return dict(tag_name=tag, draft=False, prerelease=False,
                published_at="2026-10-13T06:06:29Z",
                html_url=f"https://github.com/{REPOSITORY}/releases/tag/{tag}",
                assets=[dict(name=f"exitbind-{target}{suffix}", state="uploaded", size=10)
                        for target in TARGETS for suffix in ("", ".tar.gz", ".tar.gz.sha256")])


class DocumentationVersions(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "checkout"
        self.root.mkdir()
        for source in ROOT.rglob("*"):
            if ".git" not in source.relative_to(ROOT).parts and source.is_file() and (
                    source.suffix == ".md" or source.relative_to(ROOT).as_posix() in
                    (INVENTORY, "Cargo.toml", "tooling/releases/sync_docs.py")):
                target = self.root / source.relative_to(ROOT)
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(source, target)
        self.stable = published_tag(self.root)
        numbers = list(map(int, self.stable[1:].split(".")))
        numbers[-1] += 1
        self.next = "v" + ".".join(map(str, numbers))

    def render(self, stable=None):
        changes = plan(self.root, stable)
        for name, text in changes.items():
            (self.root / name).write_text(text)
        return changes

    def snapshot(self):
        return {str(p.relative_to(self.root)): p.read_bytes()
                for p in self.root.rglob("*") if p.is_file() and ".git" not in p.relative_to(self.root).parts}

    def cli(self, *args, env=None):
        return subprocess.run([sys.executable, str(ROOT / "tooling/releases/sync_docs.py"),
                               "--root", str(self.root), *args],
                              text=True, capture_output=True, env=env)

    def test_real_checkout_is_current(self):
        self.assertEqual(plan(ROOT), {})

    def test_next_stable_transition_is_complete_and_repeatable(self):
        before = self.snapshot()
        candidate = (self.root / "Cargo.toml").read_bytes()
        self.render(verify_release(release(self.next), self.stable))
        self.assertEqual(published_tag(self.root), self.next)
        self.assertEqual(plan(self.root), {})
        self.assertEqual(self.render(), {})
        self.assertEqual((self.root / "Cargo.toml").read_bytes(), candidate)
        inventory = json.loads((self.root / INVENTORY).read_text())
        for name, rules in inventory["files"].items():
            text = (self.root / name).read_text()
            for rule in rules:
                if "pinned" in rule:
                    self.assertIn(rule["pinned"], text)
                elif "{stable}" in rule["template"]:
                    self.assertIn(rule["template"].replace("{stable}", self.next[1:]), text)
        for name in inventory["archive"]:
            self.assertEqual((self.root / name).read_bytes(), before[name])
        for name in before:
            if name.startswith("docs/maintenance/"):
                self.assertEqual((self.root / name).read_bytes(), before[name])

    def test_candidate_bump_never_promotes_stable_installation(self):
        cargo = self.root / "Cargo.toml"
        candidate = tomllib.loads(cargo.read_text())["package"]["version"]
        next_candidate = "99.98.97"
        cargo.write_text(cargo.read_text().replace(f'version = "{candidate}"',
                                                 f'version = "{next_candidate}"', 1))
        readme = (self.root / "README.md").read_bytes()
        changes = self.render()
        self.assertEqual(set(changes), {POLICY})
        self.assertEqual(published_tag(self.root), self.stable)
        self.assertEqual((self.root / "README.md").read_bytes(), readme)
        self.assertIn(f"Source candidate tag: v{next_candidate}", (self.root / POLICY).read_text())

    def test_stale_plain_and_tagged_references_fail_and_repair(self):
        for name in ("README.md", "docs/project-lessons.md", "docs/project-capabilities.md"):
            path = self.root / name
            path.write_text(path.read_text().replace(self.stable[1:], "0.0.1"))
        before = self.snapshot()
        result = self.cli()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("stale documentation", result.stderr)
        self.assertEqual(before, self.snapshot())
        self.assertEqual(self.cli("--write").returncode, 0)
        self.assertEqual(plan(self.root), {})

    def test_readme_only_gate_rejects_drift_before_rust_checks(self):
        helper = self.root / "tooling/readme/check-existing.sh"
        helper.parent.mkdir(parents=True)
        shutil.copyfile(ROOT / "tooling/readme/check-existing.sh", helper)
        readme = self.root / "README.md"
        readme.write_text(readme.read_text().replace(self.stable[1:], "0.0.1"))
        result = subprocess.run(["sh", str(helper)], text=True, capture_output=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("stale documentation versions", result.stderr)

    def test_new_language_and_guide_references_require_inventory(self):
        for name in ("README.ko.md", "README.ja.md", "guides/install.rst", "guide.mdx", "guide.adoc"):
            with self.subTest(name=name):
                path = self.root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                text = f"Install {self.stable} using the published installer.\n"
                path.write_text(text)
                with self.assertRaisesRegex(ValueError, "unclassified"):
                    plan(self.root)
                inventory_path = self.root / INVENTORY
                inventory = json.loads(inventory_path.read_text())
                inventory["files"][name] = [{"template": "Install v{stable} using the published installer.", "count": 1}]
                inventory_path.write_text(json.dumps(inventory))
                self.render(self.next)
                self.assertEqual(path.read_text(), text.replace(self.stable, self.next))

    def test_removed_duplicate_and_unknown_references_refuse_all_writes(self):
        path = self.root / "README.md"
        original = path.read_text()
        command = next(line for line in original.splitlines() if line.startswith("curl -fsSL"))
        for changed in (original.replace(command, ""), original + command + "\n",
                        original + f"Install {self.stable} here too.\n"):
            path.write_text(changed)
            before = self.snapshot()
            result = self.cli("--write")
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(before, self.snapshot())

    def test_pinned_history_cannot_be_silently_reclassified(self):
        path = self.root / "docs/legacy-compatibility.md"
        path.write_text(path.read_text().replace("v0.25.0", self.stable))
        with self.assertRaisesRegex(ValueError, "unclassified"):
            plan(self.root)

    def test_crlf_and_unrelated_bytes_survive_version_updates(self):
        path = self.root / "REFERENCE.md"
        original = path.read_bytes().replace(b"\n", b"\r\n")
        path.write_bytes(original)
        rendered = plan(self.root, self.next)["REFERENCE.md"]
        self.assertEqual(rendered.encode(), original.replace(self.stable[1:].encode(), self.next[1:].encode()))

    def test_literal_braces_in_document_examples_are_preserved(self):
        name = "docs/json-example.md"
        template = '{"version": "v{stable}", "name": "example"}'
        (self.root / name).write_text(template.replace("{stable}", self.stable[1:]) + "\n")
        inventory_path = self.root / INVENTORY
        inventory = json.loads(inventory_path.read_text())
        inventory["files"][name] = [{"template": template, "count": 1}]
        inventory_path.write_text(json.dumps(inventory))
        self.assertEqual(plan(self.root, self.next)[name], template.replace("{stable}", self.next[1:]) + "\n")

    def test_release_requires_stable_publication_and_complete_assets(self):
        valid = release(self.next)
        variants = [dict(draft=True), dict(prerelease=True), dict(published_at=None),
                    dict(tag_name=self.next + "-rc.1"), dict(tag_name="v0.031.13"),
                    dict(html_url="https://github.com/example/other/releases/tag/" + self.next),
                    dict(assets=valid["assets"][:-1])]
        for change in variants:
            with self.subTest(change=change):
                with self.assertRaises(ValueError):
                    verify_release(dict(valid, **change), self.stable)
        with self.assertRaisesRegex(ValueError, "backwards"):
            verify_release(release(self.stable), self.next)
        self.assertEqual(verify_release(release(self.stable), self.stable), self.stable)
        bad = copy.deepcopy(valid)
        bad["assets"][0]["state"] = "new"
        with self.assertRaises(ValueError):
            verify_release(bad, self.stable)

    def test_verified_github_command_and_api_failure(self):
        bin_dir = Path(self.temp.name) / "bin"
        bin_dir.mkdir()
        gh = bin_dir / "gh"
        gh.write_text('#!/bin/sh\ntest "$*" = "api repos/PANIC-ANGEL-SCIENCE/exitbind/releases/latest" || exit 99\ncat "$RELEASE_JSON"\n')
        gh.chmod(0o755)
        payload = Path(self.temp.name) / "release.json"
        payload.write_text(json.dumps(release(self.next)))
        env = dict(os.environ, PATH=str(bin_dir) + os.pathsep + os.environ["PATH"], RELEASE_JSON=str(payload))
        self.assertEqual(self.cli("--write", "--from-github", env=env).returncode, 0)
        self.assertEqual(published_tag(self.root), self.next)
        before = self.snapshot()
        gh.write_text("#!/bin/sh\nexit 1\n")
        self.assertNotEqual(self.cli("--write", "--from-github", env=env).returncode, 0)
        self.assertEqual(before, self.snapshot())


    def test_release_preparation_is_repeatable_and_does_not_publish(self):
        for directory in ("scripts", "plugins", "systems.soulmate", "schema", "src", "examples"):
            shutil.copytree(ROOT / directory, self.root / directory, dirs_exist_ok=True)
        for name in ("Cargo.lock", "install.sh", "plugin.json", "tooling/releases/prepare_docs.sh"):
            shutil.copyfile(ROOT / name, self.root / name)
        self.env = dict(os.environ, GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM="1",
                        GITHUB_REPOSITORY=REPOSITORY)

        def git(*args, cwd=None):
            result = subprocess.run(["git", *args], cwd=cwd or self.root, env=self.env,
                                    text=True, capture_output=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            return result.stdout.strip()

        git("init", "-b", "main")
        git("config", "user.name", "Fixture")
        git("config", "user.email", "fixture@example.invalid")
        git("add", ".")
        git("commit", "-m", "fixture")
        remote = Path(self.temp.name) / "remote.git"
        git("init", "--bare", str(remote))
        git("remote", "add", "origin", str(remote))
        git("push", "origin", "main")
        bin_dir = Path(self.temp.name) / "bin"
        bin_dir.mkdir()
        gh = bin_dir / "gh"
        gh.write_text('#!/bin/sh\ncat "$RELEASE_JSON"\n')
        gh.chmod(0o755)
        payload = Path(self.temp.name) / "release.json"
        payload.write_text(json.dumps(release(self.next)))
        self.env.update(PATH=str(bin_dir) + os.pathsep + self.env["PATH"], RELEASE_JSON=str(payload))
        patch = Path(self.temp.name) / "docs.patch"

        def prepare(*args):
            return subprocess.run(["sh", "tooling/releases/prepare_docs.sh", *args],
                                  cwd=self.root, env=self.env, text=True, capture_output=True)

        head = git("rev-parse", "HEAD")
        # Candidate preparation repairs drift in a dirty working tree without GitHub.
        policy = self.root / POLICY
        policy.write_text(policy.read_text().replace("Source candidate tag: " + self.stable,
                                                    "Source candidate tag: v0.0.1"))
        gh.write_text("#!/bin/sh\nexit 99\n")
        candidate = prepare()
        self.assertEqual(candidate.returncode, 0, candidate.stdout + candidate.stderr)
        self.assertEqual(git("status", "--porcelain"), "")
        self.assertEqual(published_tag(self.root), self.stable)
        gh.write_text('#!/bin/sh\ncat "$RELEASE_JSON"\n')
        first = prepare("--published")
        self.assertEqual(first.returncode, 0, first.stdout + first.stderr)
        self.assertEqual(head, git("rev-parse", "refs/heads/main", cwd=remote))
        self.assertEqual(head, git("rev-parse", "HEAD"))
        patch.write_text(git("diff", "--binary") + "\n")
        self.assertIn(self.next, patch.read_text())
        before_refusal = self.snapshot()
        self.assertNotEqual(prepare("--published").returncode, 0)
        self.assertEqual(before_refusal, self.snapshot())
        expected = self.snapshot()
        patch_bytes = patch.read_bytes()
        git("restore", ".")
        git("apply", "--check", str(patch))
        git("apply", str(patch))
        for name in json.loads((self.root / INVENTORY).read_text())["files"]:
            self.assertEqual((self.root / name).read_bytes(), expected[name])
        git("restore", ".")
        second = prepare("--published")
        self.assertEqual(second.returncode, 0, second.stdout + second.stderr)
        self.assertEqual((git("diff", "--binary") + "\n").encode(), patch_bytes)
        self.assertEqual(head, git("rev-parse", "refs/heads/main", cwd=remote))
        self.assertEqual(head, git("rev-parse", "HEAD"))


if __name__ == "__main__":
    unittest.main()
