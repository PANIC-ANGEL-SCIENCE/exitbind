import unittest
import xml.etree.ElementTree as ET
from pathlib import Path
from badges import ci_status, release_status, license_status, render
from ci_scope import readme_only


class Badges(unittest.TestCase):
    def run_data(self, **updates):
        d = dict(id=1, head_sha="a" * 40, head_branch="main", event="push", path=".github/workflows/ci.yml", status="completed", conclusion="success")
        d.update(updates)
        return d

    def test_exact_subject(self):
        for change in (dict(head_sha="b" * 40), dict(head_branch="feature/x"), dict(event="pull_request"), dict(path=".github/workflows/other.yml")):
            self.assertEqual(ci_status([self.run_data(**change)], "a" * 40)["label"], "unknown")

    def test_current_pending_not_old_green(self):
        self.assertEqual(ci_status([self.run_data(), self.run_data(id=2, status="queued")], "a" * 40)["label"], "pending")

    def test_conclusions(self):
        for conclusion, label in (("success", "passing"), ("failure", "failing"), ("cancelled", "cancelled"), (None, "unknown")):
            self.assertEqual(ci_status([self.run_data(conclusion=conclusion)], "a" * 40)["label"], label)

    def test_draft_prerelease_not_stable(self):
        base = dict(id=1, tag_name="v0.31.11", published_at="2026-10-12", draft=False, prerelease=False)
        self.assertEqual(release_status([base])["label"], "v0.31.11")
        for key in ("draft", "prerelease"):
            self.assertEqual(release_status([dict(base, **{key: True})])["label"], "none")

    def test_license(self):
        for data, value in (({}, "unknown"), ({"license": None}, "unknown"), ({"license": {"spdx_id": "NOASSERTION"}}, "unknown"), ({"license": {"spdx_id": "MIT"}}, "MIT")):
            self.assertEqual(license_status(data)["label"], value)

    def test_xml_escaping(self):
        template = (Path(__file__).resolve().parents[4] / "assets/readme/black-glass.svg.template").read_text()
        svg = render(template, "Release", "<&\"", "untrusted <script>")
        node = ET.fromstring(svg)
        self.assertEqual(node.attrib["width"], "80")
        self.assertNotIn("<script>", svg)

    def test_compact_values_preserve_text(self):
        template = (Path(__file__).resolve().parents[4] / "assets/readme/black-glass.svg.template").read_text()
        for value in ("unknown", "running", "failing", "cancelled", "failed start", "action req.", "v1234.5678.901", "GPL-3.0-or-later", "WWWWWWWWWWWWWWWW"):
            root = ET.fromstring(render(template, "License", value, "checked"))
            self.assertEqual((root.attrib["width"], root.attrib["height"]), ("80", "36"))
            lines = [n for n in root.iter() if n.attrib.get("class") == "value"]
            self.assertEqual("".join(n.text or "" for n in lines), value)
            self.assertLessEqual(len(lines), 2)
            self.assertTrue(all(len(n.text or "") <= 10 for n in lines))
            self.assertTrue(all(int(n.attrib["font-size"]) >= 10 for n in lines))

    def test_narrow_scope(self):
        self.assertTrue(readme_only(["README.md"]))
        for paths in ([], ["README.md", "src/main.rs"], ["tooling/repository/ci/readme/badges.py"], ["assets/readme/x.svg"], [".github/workflows/ci.yml"], ["docs/README.md"]):
            self.assertFalse(readme_only(paths))

class ScopeIntegration(unittest.TestCase):
    def test_git_changes_fail_closed(self):
        import os
        import subprocess
        import tempfile
        from ci_scope import scope
        with tempfile.TemporaryDirectory() as directory:
            old = os.getcwd()
            try:
                os.chdir(directory)
                def git(*args):
                    return subprocess.check_output(['git', *args], stderr=subprocess.DEVNULL).decode().strip()
                git('init'); git('config', 'user.name', 'fixture'); git('config', 'user.email', 'fixture@example.invalid')
                Path('README.md').write_text('before'); git('add', '.'); git('commit', '-m', 'base')
                base = git('rev-parse', 'HEAD')
                Path('README.md').write_text('after'); git('add', '.'); git('commit', '-m', 'readme')
                self.assertEqual(scope(base, 'HEAD'), 'readme')
                self.assertEqual(scope('0'*40, 'HEAD'), 'full')
                self.assertEqual(scope('bad', 'HEAD'), 'full')
                Path('code.rs').write_text('code'); git('add', '.'); git('commit', '-m', 'mixed')
                self.assertEqual(scope(base, 'HEAD'), 'full')
                git('rm', 'code.rs'); Path('README.md').unlink(); Path('README.md').symlink_to('missing')
                git('add', '.'); git('commit', '-m', 'link')
                self.assertEqual(scope(base, 'HEAD'), 'full')
            finally:
                os.chdir(old)


if __name__ == "__main__":
    unittest.main()
