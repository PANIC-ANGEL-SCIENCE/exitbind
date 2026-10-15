#!/usr/bin/env python3
"""Integrate one explicitly configured pair; never update main or overwrite a tag."""
import argparse
import base64
import datetime
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import urllib.error
import urllib.parse
import urllib.request

CONTEXT = 'exitbind/paired-candidate'
MARKER = '.github/paired-candidate.json'
PROTECTED = ['.github/', 'scripts/paired-prerelease.py', 'scripts/prepare-prerelease.py',
             'scripts/release-channel.py', 'scripts/ci-local.sh', 'scripts/ci-selected-history.sh',
             'scripts/publish-release-assets.sh', 'scripts/package-release.sh',
             'scripts/check-release-refs.sh', 'scripts/published-install-smoke.sh',
             'scripts/check-public-history.sh',
             'scripts/installer-smoke.sh', 'scripts/verify-binary.py', 'install.sh',
             'tooling/readme/ci_scope.py']
SHA = re.compile('[0-9a-f]{40}')
VERSION = re.compile(r'0\.(0|[1-9][0-9]*)\.([1-9][0-9]*)-(alpha|beta|rc)\.([1-9][0-9]*)')


def encode(value):
    return urllib.parse.quote(value, safe='')


def pair_config(raw):
    if not raw.strip():
        return None
    value = json.loads(raw)
    if set(value) != {'branches', 'version'}:
        raise ValueError('Pair needs exactly branches and version')
    branches = value['branches']
    if not isinstance(branches, list) or len(branches) != 2 or len(set(branches)) != 2:
        raise ValueError('Choose exactly two distinct task branches')
    for branch in branches:
        if not isinstance(branch, str) or branch in ['main', 'readme-badges'] or branch.startswith(('refs/', 'integration/', 'automation/')):
            raise ValueError('Only explicit task branches may be paired')
        if not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._/-]{0,150}', branch) or '..' in branch or branch.endswith(('/', '.', '.lock')) or '//' in branch or '/.' in branch:
            raise ValueError('Invalid task branch')
    version = value['version']
    match = VERSION.fullmatch(version) if isinstance(version, str) else None
    if not match or not 10 <= int(match[2]) <= 99:
        raise ValueError('Choose one explicit candidate in the compatible pre-1.0 series')
    return {'branches': branches, 'version': version}


class Client:
    def __init__(self):
        self.repo = os.environ['GITHUB_REPOSITORY']
        if not re.fullmatch('[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', self.repo):
            raise ValueError('Invalid repository identity')
        self.repo_id = int(os.environ['GITHUB_REPOSITORY_ID'])
        self.token = os.environ['GH_TOKEN']
        self.sequence = 0
        self.evidence = Path(os.environ.get('PAIR_EVIDENCE', 'pair-evidence'))
        self.evidence.mkdir(exist_ok=True)
        self.verify()

    def request(self, method, route, body=None, optional=False):
        if method != 'GET':
            self.verify()
        data = None if body is None else json.dumps(body).encode()
        request = urllib.request.Request('https://api.github.com/repos/'+self.repo+('/'+route if route else ''),
                    data=data, method=method, headers={'Authorization': 'Bearer '+self.token,
                    'Accept': 'application/vnd.github+json', 'X-GitHub-Api-Version': '2026-03-10',
                    'Content-Type': 'application/json'})
        self.sequence += 1
        capture = self.evidence/str(self.sequence)
        record = {'method': method, 'route': route, 'request': body, 'status': 'unknown', 'response': None}
        capture.with_suffix('.json').write_text(json.dumps(record))
        try:
            with urllib.request.urlopen(request, timeout=60) as response:
                raw = response.read()
                status = response.status
        except urllib.error.HTTPError as error:
            raw = error.read()
            status = error.code
            record.update(status=status, response=raw.decode(errors='replace'))
            capture.with_suffix('.json').write_text(json.dumps(record))
            if optional and method == 'GET' and status == 404:
                return None
            raise RuntimeError('GitHub operation failed; inspect retained evidence, do not replay uncertain writes') from error
        except (urllib.error.URLError, TimeoutError) as error:
            record['errorClass'] = type(error).__name__
            capture.with_suffix('.json').write_text(json.dumps(record))
            raise RuntimeError('Unknown operation outcome; inspect provider state before any further effect') from error
        record.update(status=status, response=raw.decode(errors='replace'))
        capture.with_suffix('.json').write_text(json.dumps(record))
        return json.loads(raw) if raw else None

    def verify(self):
        value = self.request('GET', '')
        if (value['id'], value['full_name'], value['default_branch']) != (self.repo_id, self.repo, 'main'):
            raise ValueError('Repository identity/default branch changed')

    def get(self, route, optional=False):
        return self.request('GET', route, optional=optional)

    def post(self, route, body):
        return self.request('POST', route, body)


def head(api, branch):
    value = api.get('git/ref/heads/'+encode(branch))
    sha = value['object']['sha']
    if value['object']['type'] != 'commit' or not SHA.fullmatch(sha):
        raise ValueError('Expected exact branch commit')
    return sha


def current_ci(api, branch, sha):
    runs = api.get('actions/workflows/ci.yml/runs?branch='+encode(branch)+'&head_sha='+sha+'&per_page=100')
    selected = [r for r in runs['workflow_runs'] if r['head_sha'] == sha and r['head_branch'] == branch
                and r['event'] in ['push', 'workflow_dispatch'] and r['repository']['id'] == api.repo_id]
    if not selected:
        return False
    latest = max(selected, key=lambda r: (r['run_number'], r['run_attempt']))
    return latest['status'] == 'completed' and latest['conclusion'] == 'success' and latest['path'] == '.github/workflows/ci.yml'


def latest_status(api, sha):
    statuses = api.get('commits/'+sha+'/statuses?per_page=100')
    for status in statuses:
        if status['context'] == CONTEXT:
            return status['state']
    return None


def git(root, *args, env=None, data=None):
    command = ['git', '-C', str(root), *args]
    result = subprocess.run(command, input=data, env=env, capture_output=True)
    if result.returncode:
        raise ValueError('Local integration failed; conflicts require task-branch repair')
    return result.stdout


def upload_objects(api, root, base, sources, candidate):
    """Upload only newly generated public objects, retaining canonical commit attribution."""
    objects = git(root, 'rev-list', '--objects', candidate, '^'+base, *['^'+s for s in sources]).decode().splitlines()
    objects = {line.split(' ', 1)[0] for line in objects}
    types = {oid: git(root, 'cat-file', '-t', oid).decode().strip() for oid in objects}
    for oid, kind in types.items():
        if kind == 'blob':
            result = api.post('git/blobs', {'content': base64.b64encode(git(root, 'cat-file', 'blob', oid)).decode(), 'encoding': 'base64'})
            if result['sha'] != oid:
                raise ValueError('Generated blob identity mismatch')
    done = set()

    def tree(oid):
        if oid not in objects or oid in done:
            return
        entries = []
        for record in git(root, 'ls-tree', '-z', oid).split(b'\0'):
            if not record:
                continue
            metadata, name = record.split(b'\t', 1)
            mode, kind, child = metadata.decode().split(' ')
            if kind == 'tree':
                tree(child)
            entries.append({'path': name.decode(), 'mode': mode.zfill(6), 'type': kind, 'sha': child})
        if api.post('git/trees', {'tree': entries})['sha'] != oid:
            raise ValueError('Generated tree identity mismatch')
        done.add(oid)

    for oid, kind in types.items():
        if kind == 'tree':
            tree(oid)
    for oid in git(root, 'rev-list', '--reverse', candidate, '^'+base, *['^'+s for s in sources]).decode().splitlines():
        raw = git(root, 'cat-file', 'commit', oid).decode()
        header, message = raw.split('\n\n', 1)
        fields = header.splitlines()
        author = os.environ['PAIR_AUTHOR_NAME']
        email = os.environ['PAIR_AUTHOR_EMAIL']

        def identity(prefix):
            line = next(line[len(prefix)+1:] for line in fields if line.startswith(prefix+' '))
            match = re.fullmatch(re.escape(author)+' <'+re.escape(email)+r'> ([0-9]+) ([+-][0-9]{4})', line)
            if not match or match[2] != '+0000':
                raise ValueError('Unexpected generated commit identity')
            date = datetime.datetime.fromtimestamp(int(match[1]), datetime.timezone.utc).isoformat()
            return {'name': author, 'email': email, 'date': date}
        body = {'message': message, 'tree': fields[0].split(' ')[1],
                'parents': [line.split(' ')[1] for line in fields if line.startswith('parent ')],
                'author': identity('author'), 'committer': identity('committer')}
        if api.post('git/commits', body)['sha'] != oid:
            raise ValueError('Generated commit identity mismatch')


def build_candidate(api, trusted, base, config, sources, marker, expected_tree=None, reserved_candidate=None):
    """No candidate program executes in this privileged stage."""
    with tempfile.TemporaryDirectory(prefix='paired-integration-') as directory:
        root = Path(directory)
        git(root, 'init', '-q')
        git(root, 'remote', 'add', 'origin', 'https://github.com/'+api.repo+'.git')
        if expected_tree is not None and not SHA.fullmatch(reserved_candidate or ''):
            raise ValueError('Reserved reconstruction requires an exact candidate commit')
        reserved = [reserved_candidate] if expected_tree is not None else []
        git(root, 'fetch', '--no-tags', 'origin', base, *sources, *reserved)
        for ancestor in [base, *sources] if reserved else []:
            git(root, 'merge-base', '--is-ancestor', ancestor, reserved_candidate)
        git(root, 'checkout', '--detach', base)
        env = {k: v for k, v in os.environ.items() if 'TOKEN' not in k and k not in ['GITHUB_OUTPUT', 'GITHUB_ENV']}
        env.update(GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL=os.devnull,
                   GIT_AUTHOR_NAME=os.environ['PAIR_AUTHOR_NAME'], GIT_COMMITTER_NAME=os.environ['PAIR_AUTHOR_NAME'],
                   GIT_AUTHOR_EMAIL=os.environ['PAIR_AUTHOR_EMAIL'], GIT_COMMITTER_EMAIL=os.environ['PAIR_AUTHOR_EMAIL'],
                   TZ='UTC')
        for source in sources:
            if subprocess.run(['git', '-C', str(root), 'merge-base', '--is-ancestor', source, base]).returncode == 0:
                raise ValueError('A selected task is already in main; do not recycle completed work')
            paths = git(root, 'diff', '--name-only', base+'...'+source).decode().splitlines()
            if any(any(p == guard or p.startswith(guard) for guard in PROTECTED) for p in paths):
                raise ValueError('Release-control changes need separate reviewed main delivery')
            git(root, 'merge', '--no-ff', '--no-edit', source, env=env)
        spec = importlib.util.spec_from_file_location('prepare_candidate', trusted/'scripts/prepare-prerelease.py')
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        module.prepare(root, config['version'])
        (root/MARKER).write_text(json.dumps(marker, sort_keys=True, indent=2)+'\n')
        # The checker comes from trusted main; it reads candidate data without executing it.
        subprocess.run(['sh', str(trusted/'scripts/check-release-refs.sh')], cwd=root, env=env, check=True)
        git(root, 'add', '--all', env=env)
        git(root, 'commit', '-m', 'Prepare paired prerelease '+config['version'], env=env)
        candidate = git(root, 'rev-parse', 'HEAD').decode().strip()
        if expected_tree is not None:
            if git(root, 'rev-parse', 'HEAD^{tree}').decode().strip() != expected_tree:
                raise ValueError('Reserved integration tree differs from the exact trusted reconstruction')
            return candidate
        upload_objects(api, root, base, sources, candidate)
        return candidate


def plan(api, trusted, raw):
    config = pair_config(raw)
    if config is None:
        return {'ready': False, 'state': 'idle: no future pair selected'}
    tag = 'v'+config['version']
    existing_tag = api.get('git/ref/tags/'+tag, optional=True)
    if existing_tag is not None:
        return {'ready': False, 'state': 'immutable candidate tag already exists; no automatic version increment'}
    base = head(api, 'main')
    if git(trusted, 'rev-parse', 'HEAD').decode().strip() != base:
        raise ValueError('Trusted workflow source must be the current main commit')
    sources = [head(api, b) for b in config['branches']]
    if not all(current_ci(api, b, sha) for b, sha in zip(config['branches'], sources)):
        return {'ready': False, 'state': 'waiting for both exact task heads to pass CI'}
    marker = {'protocol': 1, 'base': base, 'branches': config['branches'], 'sources': sources, 'version': config['version']}
    branch = 'integration/pair-'+config['version']
    existing = api.get('git/ref/heads/'+encode(branch), optional=True)
    if existing:
        candidate = existing['object']['sha']
        if existing['object']['type'] != 'commit' or not SHA.fullmatch(candidate):
            raise ValueError('Reserved integration ref must identify an exact commit')
        item = api.get('contents/'+MARKER+'?ref='+candidate)
        if json.loads(base64.b64decode(item['content'])) != marker:
            raise ValueError('Reserved integration branch has different inputs; explicit recovery is required')
        if latest_status(api, candidate) is not None:
            return {'ready': False, 'state': 'candidate already admitted; do not automatically retry'}
        tree = api.get('git/commits/'+candidate)['tree']['sha']
        build_candidate(api, trusted, base, config, sources, marker,
                        expected_tree=tree, reserved_candidate=candidate)
    else:
        candidate = build_candidate(api, trusted, base, config, sources, marker)
        if head(api, 'main') != base or [head(api, b) for b in config['branches']] != sources:
            raise ValueError('Inputs moved during integration; no branch or tag published')
        api.post('git/refs', {'ref': 'refs/heads/'+branch, 'sha': candidate})
        if head(api, branch) != candidate:
            raise ValueError('Integration branch readback mismatch')
    status = latest_status(api, candidate)
    if status is not None:
        return {'ready': False, 'state': 'candidate already admitted: '+status+'; do not automatically retry an unknown or failed check'}
    api.post('statuses/'+candidate, {'state': 'pending', 'context': CONTEXT,
             'description': 'Admitted one combined Linux check',
             'target_url': 'https://github.com/'+api.repo+'/actions/runs/'+os.environ['GITHUB_RUN_ID']})
    return {'ready': True, 'state': 'combined Linux check admitted', 'sha': candidate, 'base': base, 'branch': branch, 'tag': tag}


def finish(api, raw, candidate, branch, result):
    config = pair_config(raw)
    if config is None or branch != 'integration/pair-'+config['version'] or not SHA.fullmatch(candidate):
        raise ValueError('Invalid candidate completion')
    if head(api, branch) != candidate:
        raise ValueError('Candidate moved after its combined check')
    item = api.get('contents/'+MARKER+'?ref='+candidate)
    marker = json.loads(base64.b64decode(item['content']))
    if marker['branches'] != config['branches'] or marker['version'] != config['version']:
        raise ValueError('Pair changed after check admission')
    if head(api, 'main') != marker['base'] or [head(api, b) for b in config['branches']] != marker['sources']:
        raise ValueError('A task or main changed during combined CI; publication held')
    if latest_status(api, candidate) != 'pending':
        raise ValueError('No current one-shot combined-check admission')
    state = 'success' if result == 'success' else 'failure'
    api.post('statuses/'+candidate, {'state': state, 'context': CONTEXT, 'description': 'Combined Linux check '+state})
    if state != 'success':
        return {'state': 'Combined check failed; no tag or release created'}
    tag = 'v'+config['version']
    if api.get('git/ref/tags/'+tag, optional=True) is not None:
        raise ValueError('Immutable version already reserved; do not replay publication')
    api.post('git/refs', {'ref': 'refs/tags/'+tag, 'sha': candidate})
    ref = api.get('git/ref/tags/'+tag)
    if ref['object']['sha'] != candidate or ref['object']['type'] != 'commit':
        raise ValueError('Candidate tag readback mismatch')
    # GITHUB_TOKEN push events do not trigger workflows; use the declared allowed event once.
    api.post('actions/workflows/release.yml/dispatches', {'ref': tag, 'inputs': {'mode': 'release'}})
    return {'state': 'One Linux prerelease requested; inspect the release workflow for publication/install results', 'tag': tag, 'sha': candidate}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=['plan', 'finish'])
    parser.add_argument('--trusted-root', default='.')
    args = parser.parse_args()
    api = Client()
    raw = os.environ.get('PAIR_CONFIG', '')
    if args.operation == 'plan':
        result = plan(api, Path(args.trusted_root).resolve(), raw)
        with open(os.environ['GITHUB_OUTPUT'], 'a') as output:
            for key, value in result.items():
                print(key+'='+str(value).lower() if isinstance(value, bool) else key+'='+value, file=output)
    else:
        result = finish(api, raw, os.environ['PAIR_SHA'], os.environ['PAIR_BRANCH'], os.environ['PAIR_CHECK_RESULT'])
    print(json.dumps(result))
