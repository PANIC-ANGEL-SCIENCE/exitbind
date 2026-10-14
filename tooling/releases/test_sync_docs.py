import copy
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import tomllib
import unittest

from sync_docs import INVENTORY, POLICY, REPOSITORY, TARGETS, plan, published_tag

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
                    (INVENTORY, "Cargo.toml", "tooling/releases/sync_docs.py",
                     "scripts/check-installation-pointer.py")):
                target = self.root / source.relative_to(ROOT)
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(source, target)
        self.stable = published_tag(self.root)
        numbers = list(map(int, self.stable[1:].split(".")))
        numbers[-1] += 1
        self.next = "v" + ".".join(map(str, numbers))
        # Publication fixtures remain valid after the checkout itself is promoted.
        changelog = self.root / "CHANGELOG.md"
        pending = "## Unreleased — " + self.next[1:] + " source candidate"
        if pending not in changelog.read_text().splitlines():
            with changelog.open("a") as stream:
                stream.write("\n" + pending + "\n\n- Fixture candidate.\n")


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
        self.render(self.next)
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
                    self.assertIn(rule["template"].replace("{stable}", self.next[1:]).replace(
                        "{candidate}", tomllib.loads(candidate.decode())["package"]["version"]), text)
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
        helper.parent.mkdir(parents=True, exist_ok=True)
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

    def selected(self, tag=None, metadata=None, producer="a" * 40):
        tag = tag or self.next
        payload = release(tag) if metadata is None else metadata
        if metadata is None:
            payload["producer_commit"] = producer
            for asset in payload["assets"]:
                asset["digest"] = "sha256:" + "b" * 64
        path = Path(self.temp.name) / "selected.json"
        path.write_text(json.dumps(payload))
        return self.cli("--write", "--published", "--metadata", str(path),
                        "--expected-version", tag, "--expected-producer", producer)

    def test_selected_release_promotes_only_matching_heading_and_is_repeatable(self):
        changelog = self.root / "CHANGELOG.md"
        before = changelog.read_bytes()
        self.assertIn(("## Unreleased — " + self.next[1:] + " source candidate").encode(), before)
        result = self.selected()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        expected = before.replace(("## Unreleased — " + self.next[1:] + " source candidate").encode(),
                                  ("## " + self.next[1:]).encode(), 1)
        self.assertEqual(changelog.read_bytes(), expected)
        self.assertEqual(published_tag(self.root), self.next)
        after = self.snapshot()
        self.assertEqual(self.selected().returncode, 0)
        self.assertEqual(self.snapshot(), after)

    def test_selected_older_stable_is_valid_and_preserves_newer_candidate(self):
        before = self.snapshot()
        result = self.selected(self.stable)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(self.snapshot(), before)
        # Explicit stable selection may move back, without relabeling history.
        self.assertEqual(self.selected().returncode, 0)
        changelog = (self.root / "CHANGELOG.md").read_bytes()
        result = self.selected(self.stable)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(published_tag(self.root), self.stable)
        self.assertEqual((self.root / "CHANGELOG.md").read_bytes(), changelog)

    def test_promotion_preserves_newer_candidate_and_crlf_history(self):
        path = self.root / "CHANGELOG.md"
        newer = "## Unreleased — 99.98.97 source candidate\n\n- New work.\n\n"
        before = path.read_bytes().replace(b"## Unreleased", newer.encode() + b"## Unreleased", 1)
        before = before.replace(b"\n", b"\r\n")
        path.write_bytes(before)
        cargo = self.root / "Cargo.toml"
        candidate = tomllib.loads(cargo.read_text())["package"]["version"]
        cargo.write_text(cargo.read_text().replace(f'version = "{candidate}"', 'version = "99.98.97"', 1))
        result = self.selected()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(path.read_bytes(), before.replace(
            ("## Unreleased — " + self.next[1:] + " source candidate").encode(),
            ("## " + self.next[1:]).encode(), 1))
        self.assertIn("Source candidate tag: v99.98.97", (self.root / POLICY).read_text())

    def test_unverified_metadata_refuses_all_document_and_changelog_writes(self):
        valid = release(self.next)
        valid["producer_commit"] = "a" * 40
        for asset in valid["assets"]:
            asset["digest"] = "sha256:" + "b" * 64
        variants = [dict(draft=True), dict(prerelease=True), dict(published_at=None),
                    dict(tag_name=self.stable), dict(producer_commit="c" * 40),
                    dict(assets=valid["assets"][:-1]),
                    dict(assets=valid["assets"] + [valid["assets"][0]]),
                    dict(assets=valid["assets"] + [dict(name="extra", digest="sha256:" + "b" * 64)])]
        bad = copy.deepcopy(valid["assets"])
        bad[0]["digest"] = "unknown"
        variants.append(dict(assets=bad))
        before = self.snapshot()
        for variant in variants:
            with self.subTest(variant=variant):
                result = self.selected(metadata=dict(valid, **variant))
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(before, self.snapshot())

    def test_missing_readback_stays_unavailable_without_writes(self):
        before = self.snapshot()
        result = self.cli("--write", "--published", "--metadata", str(self.root / "missing.json"),
                          "--expected-version", self.next, "--expected-producer", "a" * 40)
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertIn("verification unavailable", result.stderr)
        self.assertEqual(before, self.snapshot())

    def test_ambiguous_or_missing_selected_heading_refuses_all_writes(self):
        path = self.root / "CHANGELOG.md"
        original = path.read_text()
        heading = "## Unreleased — " + self.next[1:] + " source candidate"
        for content in (original + "\n" + heading + "\n", original.replace(heading, "## Pending"),
                        original + "\n## " + self.next[1:] + "\n"):
            path.write_text(content)
            before = self.snapshot()
            result = self.selected()
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("unambiguous changelog", result.stderr)
            self.assertEqual(before, self.snapshot())

    def test_ci_rejects_selected_stable_with_unreleased_heading_without_relabeling(self):
        # Simulate a manual pointer-only update that forgot publication status.
        self.render(self.next)
        before = self.snapshot()
        result = self.cli()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Unreleased heading", result.stderr)
        self.assertEqual(before, self.snapshot())
        self.assertEqual(self.selected().returncode, 0)
        self.assertEqual(self.cli().returncode, 0)

    def test_preparation_wrapper_uses_caller_metadata_path_and_checks_result(self):
        for directory in ("scripts", "plugins", "systems.soulmate", "schema", "src", "examples"):
            shutil.copytree(ROOT / directory, self.root / directory, dirs_exist_ok=True)
        for name in ("Cargo.lock", "install.sh", "plugin.json", "tooling/releases/prepare_docs.sh"):
            shutil.copyfile(ROOT / name, self.root / name)
        helper = self.root / "tooling/releases/prepare_docs.sh"
        payload = release(self.next)
        payload["producer_commit"] = "a" * 40
        for asset in payload["assets"]:
            asset["digest"] = "sha256:" + "b" * 64
        (Path(self.temp.name) / "readback.json").write_text(json.dumps(payload))
        result = subprocess.run(["sh", str(helper)], cwd=self.temp.name, text=True, capture_output=True)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(published_tag(self.root), self.stable)
        result = subprocess.run(["sh", str(helper), "--published", "--metadata", "readback.json",
                                 "--expected-version", self.next, "--expected-producer", "a" * 40],
                                cwd=self.temp.name, text=True, capture_output=True)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(published_tag(self.root), self.next)
        self.assertNotIn("## Unreleased — " + self.next[1:] + " source candidate",
                         (self.root / "CHANGELOG.md").read_text())

    def test_plain_candidate_sync_does_not_promote_unreleased_heading(self):
        path = self.root / "CHANGELOG.md"
        before = path.read_bytes()
        self.assertEqual(self.cli("--write").returncode, 0)
        self.assertEqual(path.read_bytes(), before)
        for args in (("--write", "--published"), ("--expected-version", self.next),
                     ("--write", "--from-github")):
            self.assertNotEqual(self.cli(*args).returncode, 0)
        self.assertEqual(path.read_bytes(), before)


if __name__ == "__main__":
    unittest.main()
