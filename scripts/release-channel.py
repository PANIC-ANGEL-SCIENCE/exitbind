#!/usr/bin/env python3
"""Select release targets from the checked-out package, never the tag spelling alone."""
import argparse
import json
import os
from pathlib import Path
import re

LINUX = 'x86_64-unknown-linux-gnu'
MACS = ['aarch64-apple-darwin', 'x86_64-apple-darwin']
VERSION = re.compile(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?')


def package_version(root):
    text = (Path(root) / 'Cargo.toml').read_text()
    package = text.split('[package]', 1)[1].split('\n[', 1)[0]
    values = re.findall(r'^version = "([^"]+)"$', package, re.M)
    if len(values) != 1 or not VERSION.fullmatch(values[0]):
        raise ValueError('Expected one valid package version')
    version = values[0]
    pre = VERSION.fullmatch(version)[4]
    if pre and any(x.isdigit() and len(x) > 1 and x[0] == '0' for x in pre.split('.')):
        raise ValueError('Numeric prerelease identifiers cannot have leading zeroes')
    return version


def channel(root, ref, mode='release'):
    version = package_version(root)
    if mode not in ['release', 'stable-readiness']:
        raise ValueError('Unknown release mode')
    if mode == 'release' and ref != 'refs/tags/v' + version:
        raise ValueError('Release requires the exact package-version tag')
    if mode == 'stable-readiness' and not ref.startswith('refs/heads/integration/pair-'):
        raise ValueError('Stable readiness requires a dedicated integration branch')
    prerelease = VERSION.fullmatch(version)[4] is not None
    full = not prerelease or mode == 'stable-readiness'
    targets = [LINUX] + (MACS if full else [])
    matrix = [{'runner': 'ubuntu-latest', 'host_os': 'Linux', 'host_arch': 'x86_64', 'target': LINUX}]
    if full:
        matrix += [
            {'runner': 'macos-15', 'host_os': 'Darwin', 'host_arch': 'arm64', 'target': MACS[0]},
            {'runner': 'macos-15-intel', 'host_os': 'Darwin', 'host_arch': 'x86_64', 'target': MACS[1]},
        ]
    return {'prerelease': str(prerelease).lower(), 'full_platforms': str(full).lower(),
            'readiness_only': str(mode == 'stable-readiness').lower(),
            'targets': ' '.join(targets), 'installed': json.dumps({'include': matrix}, separators=(',', ':'))}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', default='.')
    parser.add_argument('--mode', default='release')
    args = parser.parse_args()
    result = channel(args.root, os.environ['GITHUB_REF'], args.mode)
    with open(os.environ['GITHUB_OUTPUT'], 'a') as output:
        for key, value in result.items():
            print(f'{key}={value}', file=output)
