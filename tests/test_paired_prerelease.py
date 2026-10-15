"""No-network release-channel, admission and real Git-object integration probes."""
import base64
import contextlib
import datetime
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, ROOT/path)
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


PAIR = module('paired', 'scripts/paired-prerelease.py')
CHANNEL = module('channel', 'scripts/release-channel.py')
PREPARE = module('prepare', 'scripts/prepare-prerelease.py')
CURRENT = CHANNEL.package_version(ROOT)
CORE = CURRENT.split('-', 1)[0].split('+', 1)[0]
PARTS = [int(x) for x in CORE.split('.')]
NEXT = '.'.join(map(str, PARTS[:2]+[PARTS[2]+1]))+'-rc.1'
CONFIG = {'branches': ['R01/task-one', 'dot01/task-two'], 'version': NEXT}
A, B, BASE, CANDIDATE = [x*40 for x in ['a', 'b', 'c', 'd']]


def run(root, *args, data=None):
    p = subprocess.run(['git', '-C', str(root), *args], input=data, capture_output=True)
    if p.returncode:
        raise AssertionError(p.stderr.decode())
    return p.stdout


class FakeAPI:
    repo = 'fixture/repo'
    repo_id = 1

    def __init__(self):
        self.refs = {'main': BASE, CONFIG['branches'][0]: A, CONFIG['branches'][1]: B}
        self.tags = {}
        self.statuses = {}
        self.calls = []
        self.marker = {'protocol': 1, 'base': BASE, 'branches': CONFIG['branches'], 'sources': [A, B], 'version': NEXT}
        self.conclusion = 'success'
        self.extra_run = None
        self.fail_dispatch = False

    def get(self, route, optional=False):
        from urllib.parse import unquote, parse_qs, urlsplit
        if route.startswith('git/ref/heads/'):
            sha = self.refs.get(unquote(route[len('git/ref/heads/'):]))
            if sha is None and optional:
                return None
            return {'object': {'sha': sha, 'type': 'commit'}}
        if route.startswith('git/ref/tags/'):
            sha = self.tags.get(route[len('git/ref/tags/'):])
            return None if sha is None else {'object': {'sha': sha, 'type': 'commit'}}
        if route.startswith('actions/workflows/ci.yml/runs?'):
            args = parse_qs(urlsplit(route).query)
            value = {'head_sha': args['head_sha'][0], 'head_branch': args['branch'][0], 'event': 'push',
                     'repository': {'id': 1}, 'run_number': 1, 'run_attempt': 1,
                     'status': 'completed', 'conclusion': self.conclusion, 'path': '.github/workflows/ci.yml'}
            return {'workflow_runs': [value]+([dict(value, **self.extra_run)] if self.extra_run else [])}
        if route.startswith('contents/'):
            return {'content': base64.b64encode(json.dumps(self.marker).encode()).decode()}
        if route.startswith('commits/') and '/statuses?' in route:
            sha = route.split('/')[1]
            return [{'context': PAIR.CONTEXT, 'state': state} for state in reversed(self.statuses.get(sha, []))]
        raise AssertionError(route)

    def post(self, route, body):
        self.calls.append((route, body))
        if route.startswith('statuses/'):
            self.statuses.setdefault(route.split('/')[1], []).append(body['state'])
            return body
        if route == 'git/refs':
            ref = body['ref']
            if ref.startswith('refs/heads/'):
                self.refs[ref[len('refs/heads/'):]] = body['sha']
            else:
                tag = ref[len('refs/tags/'):]
                if tag in self.tags:
                    raise AssertionError('Tag overwrite')
                self.tags[tag] = body['sha']
            return body
        if route.endswith('/dispatches'):
            if self.fail_dispatch:
                raise RuntimeError('Unknown dispatch response')
            return None
        raise AssertionError(route)


class PairTests(unittest.TestCase):
    def setUp(self):
        self.environment = patch.dict(os.environ, {'GITHUB_RUN_ID': '123', 'PAIR_AUTHOR_NAME': 'Fixture',
                                                   'PAIR_AUTHOR_EMAIL': 'fixture@example.invalid'})
        self.environment.start()
        self.addCleanup(self.environment.stop)

    def test_pair_requires_exactly_two_explicit_task_refs_and_one_candidate(self):
        self.assertIsNone(PAIR.pair_config(''))
        self.assertEqual(PAIR.pair_config(json.dumps(CONFIG)), CONFIG)
        bad = [dict(CONFIG, branches=['main', 'dot01/task']), dict(CONFIG, branches=['one', 'one']),
               dict(CONFIG, branches=['one']), dict(CONFIG, branches=['one', 'two', 'three']),
               dict(CONFIG, branches=['integration/pair-x', 'two']), dict(CONFIG, branches=['one/../two', 'three']),
               dict(CONFIG, branches=['one;echo', 'two']), dict(CONFIG, branches=['one/.git', 'two']),
               dict(CONFIG, version=CORE), dict(CONFIG, version=NEXT+'+build'),
               dict(CONFIG, version=NEXT.replace('-rc.1', '-rc.01')), dict(CONFIG, enabled=True)]
        for value in bad:
            with self.subTest(value=value), self.assertRaises((ValueError, TypeError)):
                PAIR.pair_config(json.dumps(value))

    def test_current_ci_rejects_older_green_when_latest_failed_or_pending(self):
        api = FakeAPI()
        self.assertTrue(PAIR.current_ci(api, CONFIG['branches'][0], A))
        for change in [{'run_number': 2, 'conclusion': 'failure'}, {'run_number': 2, 'status': 'in_progress', 'conclusion': None},
                       {'run_number': 2, 'path': '.github/workflows/other.yml'}]:
            api.extra_run = change
            self.assertFalse(PAIR.current_ci(api, CONFIG['branches'][0], A))

    def plan(self, api):
        with patch.object(PAIR, 'git', return_value=(BASE+'\n').encode()), patch.object(PAIR, 'build_candidate', return_value=CANDIDATE):
            return PAIR.plan(api, ROOT, json.dumps(CONFIG))

    def test_unconfigured_pair_and_failed_source_ci_make_no_writes(self):
        api = FakeAPI()
        self.assertFalse(PAIR.plan(api, ROOT, '')['ready'])
        api.conclusion = 'failure'
        self.assertFalse(self.plan(api)['ready'])
        self.assertEqual(api.calls, [])

    def test_plan_creates_only_integration_ref_and_one_pending_admission(self):
        api = FakeAPI()
        value = self.plan(api)
        self.assertTrue(value['ready'])
        self.assertEqual(value['sha'], CANDIDATE)
        self.assertEqual(api.refs['main'], BASE)
        self.assertEqual(api.statuses[CANDIDATE], ['pending'])
        self.assertFalse(self.plan(api)['ready'])
        self.assertEqual(len(api.calls), 2)

    def test_reserved_tag_never_increments_or_rewrites_version(self):
        api = FakeAPI()
        api.tags['v'+NEXT] = A
        self.assertFalse(self.plan(api)['ready'])
        self.assertEqual(api.calls, [])

    def test_different_reserved_pair_marker_requires_recovery(self):
        api = FakeAPI()
        api.refs['integration/pair-'+NEXT] = CANDIDATE
        api.marker['sources'] = [B, A]
        with self.assertRaises(ValueError):
            self.plan(api)
        self.assertEqual(api.calls, [])

    def finish(self, api, result='success'):
        api.refs['integration/pair-'+NEXT] = CANDIDATE
        api.statuses[CANDIDATE] = ['pending']
        return PAIR.finish(api, json.dumps(CONFIG), CANDIDATE, 'integration/pair-'+NEXT, result)

    def test_failed_combined_check_never_creates_a_tag(self):
        api = FakeAPI()
        self.finish(api, 'failure')
        self.assertEqual(api.tags, {})
        self.assertEqual(api.statuses[CANDIDATE], ['pending', 'failure'])

    def test_source_or_main_move_holds_publication(self):
        for branch in ['main', *CONFIG['branches']]:
            api = FakeAPI()
            api.refs[branch] = 'e'*40
            with self.subTest(branch=branch), self.assertRaises(ValueError):
                self.finish(api)
            self.assertEqual(api.calls, [])

    def test_success_creates_one_tag_and_explicit_allowed_dispatch(self):
        api = FakeAPI()
        self.finish(api)
        self.assertEqual(api.tags, {'v'+NEXT: CANDIDATE})
        self.assertEqual(api.refs['main'], BASE)
        self.assertEqual(api.calls[-1], ('actions/workflows/release.yml/dispatches', {'ref': 'v'+NEXT, 'inputs': {'mode': 'release'}}))
        with self.assertRaises(ValueError):
            PAIR.finish(api, json.dumps(CONFIG), CANDIDATE, 'integration/pair-'+NEXT, 'success')

    def test_unknown_dispatch_is_retained_without_a_replay(self):
        api = FakeAPI()
        api.fail_dispatch = True
        with self.assertRaises(RuntimeError):
            self.finish(api)
        before = list(api.calls)
        self.assertFalse(self.plan(api)['ready'])
        self.assertEqual(api.calls, before)


class ClientEvidenceTests(unittest.TestCase):
    def test_unknown_write_retains_attempt_without_token_or_retry(self):
        import urllib.error
        class Response:
            status = 200
            def __enter__(self): return self
            def __exit__(self, *args): pass
            def read(self): return json.dumps({'id': 1, 'full_name': 'fixture/repo', 'default_branch': 'main'}).encode()
        calls = []
        def request(value, timeout):
            calls.append(value.method)
            if value.method == 'POST':
                raise urllib.error.URLError('fixture connection lost')
            return Response()
        with tempfile.TemporaryDirectory(prefix='pair-api-') as directory:
            with patch.dict(os.environ, {'GITHUB_REPOSITORY': 'fixture/repo', 'GITHUB_REPOSITORY_ID': '1',
                                        'GH_TOKEN': 'fixture-private-token', 'PAIR_EVIDENCE': directory}), patch.object(PAIR.urllib.request, 'urlopen', side_effect=request):
                client = PAIR.Client()
                with self.assertRaises(RuntimeError):
                    client.post('git/refs', {'ref': 'refs/tags/v'+NEXT, 'sha': CANDIDATE})
            records = [json.loads(p.read_text()) for p in Path(directory).glob('*.json')]
            writes = [x for x in records if x['method'] == 'POST']
            self.assertEqual(len(writes), 1)
            self.assertEqual(writes[0]['status'], 'unknown')
            self.assertEqual(writes[0]['request']['sha'], CANDIDATE)
            self.assertNotIn('fixture-private-token', json.dumps(records))
            self.assertEqual(calls, ['GET', 'GET', 'POST'])


class ChannelTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='release-channel-')
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)

    def version(self, value):
        (self.root/'Cargo.toml').write_text('[package]\nname = "fixture"\nversion = "'+value+'"\n')

    def test_prerelease_is_linux_only_and_stable_has_both_macs(self):
        for value, full in [(CORE, True), (NEXT, False), (CORE+'+build-with-hyphen', True)]:
            self.version(value)
            choice = CHANNEL.channel(self.root, 'refs/tags/v'+value)
            targets = choice['targets'].split()
            self.assertEqual(len(targets), 3 if full else 1)
            self.assertEqual(len(json.loads(choice['installed'])['include']), 3 if full else 1)

    def test_readiness_checks_all_platforms_without_requiring_a_tag(self):
        self.version(NEXT)
        choice = CHANNEL.channel(self.root, 'refs/heads/integration/pair-'+NEXT, 'stable-readiness')
        self.assertEqual(choice['readiness_only'], 'true')
        self.assertEqual(len(choice['targets'].split()), 3)

    def test_wrong_release_tag_and_wrong_readiness_ref_refuse(self):
        self.version(NEXT)
        for ref, mode in [('refs/heads/main', 'release'), ('refs/tags/v'+CORE, 'release'),
                          ('refs/heads/main', 'stable-readiness'), ('refs/tags/v'+NEXT, 'stable-readiness'),
                          ('refs/tags/v'+NEXT, 'unknown')]:
            with self.subTest(ref=ref, mode=mode), self.assertRaises(ValueError):
                CHANNEL.channel(self.root, ref, mode)


class MetadataSafetyTests(unittest.TestCase):
    def test_invalid_metadata_and_symlinks_refuse_before_any_write(self):
        for kind in ['shape', 'symlink']:
            with self.subTest(kind=kind), tempfile.TemporaryDirectory(prefix='pair-metadata-') as directory:
                root = Path(directory)/'source'
                shutil.copytree(ROOT, root, ignore=shutil.ignore_patterns('.git', '__pycache__'))
                target = root/'plugin.json'
                if kind == 'shape':
                    target.write_text('{}')
                else:
                    outside = Path(directory)/'outside.json'
                    outside.write_bytes(target.read_bytes())
                    target.unlink()
                    target.symlink_to(outside)
                before = {str(p.relative_to(root)): p.read_bytes() for p in root.rglob('*') if p.is_file()}
                with self.assertRaises(ValueError):
                    PREPARE.prepare(root, NEXT)
                after = {str(p.relative_to(root)): p.read_bytes() for p in root.rglob('*') if p.is_file()}
                self.assertEqual(before, after)


class RealGitObjectTests(unittest.TestCase):
    def test_generated_merge_version_and_git_data_objects_keep_exact_identity(self):
        with tempfile.TemporaryDirectory(prefix='pair-git-') as directory:
            root = Path(directory)
            trusted = root/'trusted'
            shutil.copytree(ROOT, trusted, ignore=shutil.ignore_patterns('.git', '__pycache__'))
            run(trusted, 'init', '-q', '-b', 'main')
            run(trusted, 'config', 'user.name', 'Fixture')
            run(trusted, 'config', 'user.email', 'fixture@example.invalid')
            run(trusted, 'add', '--all')
            run(trusted, 'commit', '-qm', 'Fixture base')
            base = run(trusted, 'rev-parse', 'HEAD').decode().strip()
            heads = []
            for index, branch in enumerate(CONFIG['branches']):
                run(trusted, 'checkout', '-qb', branch, base)
                (trusted/f'tests/pair-{index}.txt').write_text(str(index)+'\n')
                run(trusted, 'add', '--all')
                run(trusted, 'commit', '-qm', 'Fixture task')
                heads.append(run(trusted, 'rev-parse', 'HEAD').decode().strip())
            run(trusted, 'checkout', '-q', 'main')
            bare = root/'remote.git'
            p = subprocess.run(['git', 'clone', '-q', '--bare', str(trusted), str(bare)], capture_output=True)
            self.assertEqual(p.returncode, 0, p.stderr)

            class GitAPI:
                repo = 'fixture/repo'
                writes = 0

                def post(self, route, body):
                    self.writes += 1
                    if route == 'git/blobs':
                        sha = run(bare, 'hash-object', '-w', '--stdin', data=base64.b64decode(body['content']))
                    elif route == 'git/trees':
                        records = [f"{x['mode']} {x['type']} {x['sha']}\t{x['path']}".encode() for x in body['tree']]
                        sha = run(bare, 'mktree', '-z', data=b'\0'.join(records)+b'\0')
                    elif route == 'git/commits':
                        fields = ['tree '+body['tree']]+['parent '+x for x in body['parents']]
                        for key in ['author', 'committer']:
                            value = body[key]
                            timestamp = int(datetime.datetime.fromisoformat(value['date']).timestamp())
                            fields.append(f"{key} {value['name']} <{value['email']}> {timestamp} +0000")
                        raw = '\n'.join(fields)+'\n\n'+body['message']
                        sha = run(bare, 'hash-object', '-w', '-t', 'commit', '--stdin', data=raw.encode())
                    else:
                        raise AssertionError(route)
                    return {'sha': sha.decode().strip()}

            original_git = PAIR.git

            def local_git(path, *args, **kwargs):
                args = tuple(str(bare) if x == 'https://github.com/fixture/repo.git' else x for x in args)
                return original_git(path, *args, **kwargs)

            marker = {'protocol': 1, 'base': base, 'branches': CONFIG['branches'], 'sources': heads, 'version': NEXT}
            with patch.object(PAIR, 'git', side_effect=local_git), patch.dict(os.environ, {
                    'PAIR_AUTHOR_NAME': 'Fixture', 'PAIR_AUTHOR_EMAIL': 'fixture@example.invalid', 'TZ': 'UTC'}):
                candidate = PAIR.build_candidate(GitAPI(), trusted, base, CONFIG, heads, marker)
                probe = GitAPI()
                with self.assertRaises(ValueError):
                    PAIR.build_candidate(probe, trusted, base, CONFIG, heads, marker, expected_tree='0'*40)
                self.assertEqual(probe.writes, 0)
                expected = run(bare, 'rev-parse', candidate+'^{tree}').decode().strip()
                PAIR.build_candidate(probe, trusted, base, CONFIG, heads, marker, expected_tree=expected)
                self.assertEqual(probe.writes, 0)
            for source in [base, *heads]:
                p = subprocess.run(['git', '-C', str(bare), 'merge-base', '--is-ancestor', source, candidate])
                self.assertEqual(p.returncode, 0)
            actual = json.loads(run(bare, 'show', candidate+':'+PAIR.MARKER))
            self.assertEqual(actual, marker)
            manifest = run(bare, 'show', candidate+':Cargo.toml').decode()
            self.assertIn('version = "'+NEXT+'"', manifest)
            self.assertEqual(run(bare, 'show', candidate+':README.md'), (trusted/'README.md').read_bytes())


if __name__ == '__main__':
    unittest.main()
