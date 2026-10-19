#!/usr/bin/env python3
"""Import read-only GitHub readbacks into a local ship report; never call GitHub.

Use retained JSON from existing gh/connector readers. Historical release facts
remain context-only, even when all provider jobs passed.
"""
import argparse
import hashlib
import json
import subprocess
from pathlib import Path

MAX = 2 * 1024 * 1024


def read(path):
    with path.open('rb') as stream:
        raw = stream.read(MAX + 1)
    if len(raw) > MAX:
        raise ValueError('provider readback exceeds 2 MiB')
    return json.loads(raw), hashlib.sha256(raw).hexdigest()


def summary(run, release):
    if not isinstance(run.get('id'), int) or not isinstance(run.get('head_sha'), str):
        raise ValueError('run identity missing')
    status = run.get('status')
    conclusion = run.get('conclusion')
    if status not in ('queued', 'in_progress', 'completed', 'unknown'):
        raise ValueError('unsupported run status')
    if status != 'completed' and conclusion is not None:
        raise ValueError('pending run cannot have a completed conclusion')
    if not isinstance(release.get('id'), int) or not isinstance(release.get('tag_name'), str):
        raise ValueError('release identity missing')
    return {'runId': run['id'], 'headSha': run['head_sha'], 'status': status,
            'conclusion': conclusion, 'releaseId': release['id'],
            'tag': release['tag_name'], 'draft': release.get('draft'),
            'assetCount': len(release.get('assets', [])), 'applicability': 'context_only',
            'candidateAcceptanceCredit': False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path)
    parser.add_argument('--config', type=Path)
    parser.add_argument('--packet')
    parser.add_argument('--run', type=Path)
    parser.add_argument('--release', type=Path)
    parser.add_argument('--self-test', action='store_true')
    args = parser.parse_args()
    if args.self_test:
        release = {'id': 1, 'tag_name': 'v0.1.0'}
        for conclusion in ('success', 'failure', 'cancelled', None):
            v = summary({'id': 2, 'head_sha': 'a'*40, 'status': 'completed', 'conclusion': conclusion}, release)
            assert v['conclusion'] == conclusion and not v['candidateAcceptanceCredit']
        try:
            summary({'id': 2, 'head_sha': 'a'*40, 'status': 'in_progress', 'conclusion': 'success'}, release)
        except ValueError:
            print('provider adapter: completed/failed/pending distinctions passed')
            return
        raise AssertionError('pending success accepted')
    if any(getattr(args, name) is None for name in ('binary', 'config', 'packet', 'run', 'release')):
        parser.error('--binary, --config, --packet, --run and --release are required')
    run, run_digest = read(args.run)
    release, release_digest = read(args.release)
    facts = summary(run, release)
    command = [str(args.binary.resolve()), 'ship', 'status', args.packet,
               '--config', str(args.config.resolve()), '--json']
    status = json.loads(subprocess.check_output(command, timeout=30))
    evidence = json.dumps({'facts': facts, 'runSha256': run_digest,
                           'releaseSha256': release_digest}, separators=(',', ':'))
    identity = hashlib.sha256(evidence.encode()).hexdigest()[:32]
    observation = {'id': 'github-'+identity, 'classification': 'provider',
                   'sourceSha256': status['report']['sourceIdentity'],
                   'provider': 'github-readback-import', 'runId': str(run['id']),
                   'status': run['status'], 'conclusion': run.get('conclusion'),
                   'evidence': evidence,
                   'reason': 'Read-only historical provider facts; context only, no credit for this packet source.'}
    result = subprocess.run([str(args.binary.resolve()), 'ship', 'observe', args.packet,
                             '--config', str(args.config.resolve()), '--apply', '--json'],
                            input=json.dumps(observation).encode(), stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, timeout=30, check=True)
    print(result.stdout.decode(), end='')


if __name__ == '__main__':
    main()
