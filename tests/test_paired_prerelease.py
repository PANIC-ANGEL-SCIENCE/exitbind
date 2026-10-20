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


PAIR = module('paired', 'tooling/repository/release/paired-prerelease.py')
CHANNEL = module('channel', 'tooling/repository/release/release-channel.py')
PREPARE = module('prepare', 'tooling/repository/release/prepare-prerelease.py')
CURRENT = CHANNEL.package_version(ROOT)
CORE = CURRENT.split('-', 1)[0].split('+', 1)[0]
PARTS = [int(x) for x in CORE.split('.')]
NEXT = '.'.join(map(str, PARTS[:2]+[PARTS[2]+1]))+'-rc.1'
CONFIG = {'branches': ['R01/task-one', 'dot01/task-two'], 'version': NEXT}
A, B, BASE, CANDIDATE = [x*40 for x in ['a', 'b', 'c', 'd']]


def run(root, *args, data=None, env=None):
    p = subprocess.run(['git', '-C', str(root), *args], input=data, env=env, capture_output=True)
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


    def test_candidate_inventory_cannot_make_trusted_preparation_rewrite_release_control(self):
        # Review reproduction: a task branch adds a `sources` rule for the protected
        # installer's retired Soulmate guard; trusted preparation must refuse it.
        guard = 'if test "$surface" = soulmate && test "$version" = v0.25.0; then'
        for kind in ['sources', 'documents']:
            with self.subTest(kind=kind), tempfile.TemporaryDirectory(prefix='pair-inventory-') as directory:
                root = Path(directory)/'source'
                tracked = subprocess.run(['git', '-C', str(ROOT), 'ls-files', '-z'], capture_output=True, check=True)
                for name in filter(None, tracked.stdout.decode().split('\0')):
                    (root/name).parent.mkdir(parents=True, exist_ok=True)
                    shutil.copy2(ROOT/name, root/name)
                inventory_path = root/'tooling/repository/release/doc-versions.json'
                inventory = json.loads(inventory_path.read_text())
                if kind == 'sources':
                    inventory['sources']['install.sh'].append(
                        {'current': guard.replace('v0.25.0', 'v{version}'), 'count': 1})
                else:
                    (root/'.github/notes.md').write_text('Released as v'+CURRENT+'.\n')
                    inventory['documents']['.github/notes.md'] = [{'current': 'Released as v{version}.', 'count': 1}]
                inventory_path.write_text(json.dumps(inventory, indent=2)+'\n')
                run(root, 'init', '-q')
                run(root, 'add', '--all')
                before = {str(p.relative_to(root)): p.read_bytes() for p in root.rglob('*') if p.is_file() and '.git' not in p.parts}
                with self.assertRaises(ValueError):
                    PREPARE.prepare(root, NEXT)
                after = {str(p.relative_to(root)): p.read_bytes() for p in root.rglob('*') if p.is_file() and '.git' not in p.parts}
                self.assertEqual(before, after)
                self.assertIn(guard, (root/'install.sh').read_text())


class RealGitObjectTests(unittest.TestCase):
    def test_privileged_repository_commands_refuse_before_merge_or_api_writes(self):
        class ReachedTrustedPreparation(Exception):
            pass

        controls = ['tooling/repository/release/check-public-history.sh',
                    'tooling/repository/release/publish-release-assets.sh',
                    'tooling/repository/release/paired-prerelease.py',
                    'tooling/repository/release/prepare-prerelease.py',
                    'tooling/repository/release/release-channel.py',
                    'tooling/repository/release/check-release-refs.sh',
                    '.github/workflows/release.yml', 'install.sh',
                    'scripts/paired-prerelease.py', 'scripts/prepare-prerelease.py',
                    'scripts/release-channel.py', 'scripts/ci-local.sh', 'scripts/ci-selected-history.sh',
                    'scripts/publish-release-assets.sh', 'scripts/package-release.sh',
                    'scripts/check-release-refs.sh', 'scripts/published-install-smoke.sh',
                    'scripts/check-public-history.sh', 'scripts/installer-smoke.sh',
                    'scripts/verify-binary.py', 'tooling/readme/ci_scope.py']
        publication = (ROOT/'.github/workflows/release.yml').read_text().split('  publish:\n', 1)[1].split('  installed:\n', 1)[0]
        import re
        commands = re.findall(r'(?:\./|sh )(tooling/repository/release/[a-z0-9-]+\.sh)', publication)
        self.assertEqual(set(commands), {'tooling/repository/release/check-public-history.sh',
                                         'tooling/repository/release/verify-release-evidence.sh',
                                         'tooling/repository/release/publish-release-assets.sh'})
        readiness = (ROOT/'.github/workflows/release.yml').read_text().split(
            '  stable-readiness-result:\n', 1)[1]
        readiness_command = re.search(
            r'(?m)^\s*run:\s*python3\s+(tooling/repository/release/[a-z0-9-]+\.py)\s*$', readiness)
        self.assertIsNotNone(readiness_command)
        workflow_controls = set(commands) | {readiness_command.group(1)}
        controls.extend(sorted(workflow_controls - set(controls)))
        for control in controls:
            with self.subTest(control=control), tempfile.TemporaryDirectory(prefix='pair-control-') as directory:
                root = Path(directory)
                trusted = root/'trusted'
                trusted.mkdir()
                fixture_env = os.environ.copy()
                for name in ('CODEX_THREAD_ID', 'CODEX_SESSION_ID', 'CLAUDE_CODE_SESSION_ID',
                             'EXITBIND_NATIVE_SESSION_ID', 'EXITBIND_SESSION_GOALS_DIR', 'CLAUDE_ENV_FILE'):
                    fixture_env.pop(name, None)
                fixture_env.update(GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL=os.devnull,
                                   GIT_AUTHOR_NAME='Fixture', GIT_AUTHOR_EMAIL='fixture@example.invalid',
                                   GIT_COMMITTER_NAME='Fixture', GIT_COMMITTER_EMAIL='fixture@example.invalid',
                                   PAIR_AUTHOR_NAME='Fixture', PAIR_AUTHOR_EMAIL='fixture@example.invalid')
                run(trusted, 'init', '-q', '-b', 'main', env=fixture_env)
                run(trusted, 'config', 'user.name', 'Fixture', env=fixture_env)
                run(trusted, 'config', 'user.email', 'fixture@example.invalid', env=fixture_env)
                (trusted/'base.txt').write_text('base\n')
                run(trusted, 'add', '--all', env=fixture_env)
                run(trusted, 'commit', '-qm', 'Fixture base', env=fixture_env)
                base = run(trusted, 'rev-parse', 'HEAD', env=fixture_env).decode().strip()
                target = trusted/control
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_text('candidate control must never execute\n')
                run(trusted, 'add', '--all', env=fixture_env)
                run(trusted, 'commit', '-qm', 'Fixture changed control', env=fixture_env)
                changed = run(trusted, 'rev-parse', 'HEAD', env=fixture_env).decode().strip()
                bare = root/'remote.git'
                run(trusted, 'clone', '-q', '--bare', str(trusted), str(bare), env=fixture_env)
                original_git = PAIR.git
                seen = []
                def local_git(path, *args, **kwargs):
                    seen.append(args)
                    kwargs.setdefault('env', fixture_env)
                    return original_git(path, *tuple(str(bare) if x == 'https://github.com/fixture/repo.git' else x for x in args), **kwargs)
                api = FakeAPI()
                with patch.object(PAIR, 'git', side_effect=local_git), patch.dict(os.environ, fixture_env, clear=True), \
                        patch.object(PAIR.importlib.util, 'spec_from_file_location', side_effect=ReachedTrustedPreparation):
                    try:
                        PAIR.build_candidate(api, trusted, base, CONFIG, [changed, changed], {})
                    except ValueError as error:
                        self.assertEqual(str(error), 'Release-control changes need separate reviewed main delivery')
                    except ReachedTrustedPreparation:
                        self.fail(f'{control} reached trusted preparation after merge')
                    else:
                        self.fail(f'{control} completed without the protected refusal')
                self.assertFalse(any(args[0] == 'merge' for args in seen))
                self.assertEqual(api.calls, [])
        for control in workflow_controls:
            self.assertTrue(any(control == guard or control.startswith(guard) for guard in PAIR.PROTECTED), control)

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
                run(bare, 'update-ref', 'refs/heads/reserved-good', candidate)
                probe = GitAPI()
                with self.assertRaises(ValueError):
                    PAIR.build_candidate(probe, trusted, base, CONFIG, heads, marker, expected_tree='0'*40, reserved_candidate=candidate)
                self.assertEqual(probe.writes, 0)
                expected = run(bare, 'rev-parse', candidate+'^{tree}').decode().strip()
                PAIR.build_candidate(probe, trusted, base, CONFIG, heads, marker, expected_tree=expected, reserved_candidate=candidate)
                self.assertEqual(probe.writes, 0)

                class ResumeAPI(FakeAPI):
                    def __init__(self, reserved):
                        super().__init__()
                        self.refs = dict(zip(['main', *CONFIG['branches']], [base, *heads]))
                        self.refs['integration/pair-'+NEXT] = reserved
                        self.marker = marker

                    def get(self, route, optional=False):
                        if route.startswith('git/commits/'):
                            return {'tree': {'sha': expected}}
                        return super().get(route, optional)

                for index, parents in enumerate([[base], [heads[0]], [heads[1]], []]):
                    command = ['-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', 'commit-tree', expected]
                    for parent in parents:
                        command += ['-p', parent]
                    forged = run(bare, *command, data=b'Equal tree without all required ancestry\n').decode().strip()
                    run(bare, 'update-ref', f'refs/heads/reserved-bad-{index}', forged)
                    with self.subTest(parents=parents), self.assertRaises(ValueError):
                        PAIR.build_candidate(probe, trusted, base, CONFIG, heads, marker, expected_tree=expected, reserved_candidate=forged)
                    self.assertEqual(probe.writes, 0)
                    api = ResumeAPI(forged)
                    with self.subTest(admission_parents=parents), self.assertRaises(ValueError):
                        PAIR.plan(api, trusted, json.dumps(CONFIG))
                    self.assertEqual(api.calls, [])
                api = ResumeAPI(candidate)
                with patch.dict(os.environ, {'GITHUB_RUN_ID': 'fixture'}):
                    resumed = PAIR.plan(api, trusted, json.dumps(CONFIG))
                self.assertTrue(resumed['ready'])
                self.assertEqual(resumed['sha'], candidate)
                self.assertEqual(len(api.calls), 1)
                self.assertEqual(api.calls[0][0], 'statuses/'+candidate)
            for source in [base, *heads]:
                p = subprocess.run(['git', '-C', str(bare), 'merge-base', '--is-ancestor', source, candidate])
                self.assertEqual(p.returncode, 0)
            actual = json.loads(run(bare, 'show', candidate+':'+PAIR.MARKER))
            self.assertEqual(actual, marker)
            manifest = run(bare, 'show', candidate+':Cargo.toml').decode()
            self.assertIn('version = "'+NEXT+'"', manifest)
            self.assertEqual(run(bare, 'show', candidate+':README.md'), (trusted/'README.md').read_bytes())
            # The trusted version inventory moved every current mention to the candidate.
            policy = run(bare, 'show', candidate+':docs/release-version-policy.md').decode()
            self.assertIn('Source candidate tag: v'+NEXT+'\n', policy)
            self.assertIn('`v'+NEXT+'` includes', run(bare, 'show', candidate+':REFERENCE.md').decode())
            self.assertIn('version="${EXITBIND_VERSION:-v'+NEXT+'}"', run(bare, 'show', candidate+':install.sh').decode())


if __name__ == '__main__':
    unittest.main()
