#!/usr/bin/env python3
"""Prepare coherent metadata for one explicitly chosen candidate version."""
import argparse
import importlib.util
from pathlib import Path
import re


def load_channel():
    spec = importlib.util.spec_from_file_location('release_channel', Path(__file__).with_name('release-channel.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def prepare(root, version):
    root = Path(root).resolve()
    channel = load_channel()
    match = channel.VERSION.fullmatch(version)
    if not match or not match[4] or match[5] or match[1] != '0':
        raise ValueError('Choose an explicit pre-1.0 prerelease without build metadata')
    if any(x.isdigit() and len(x) > 1 and x[0] == '0' for x in match[4].split('.')):
        raise ValueError('Invalid prerelease identifier')
    current = channel.package_version(root)
    old = channel.VERSION.fullmatch(current)
    if (match[1], match[2]) != (old[1], old[2]) or not 10 <= int(match[3]) <= 99:
        raise ValueError('A compatibility-series change needs a separate decision')
    if int(match[3]) < int(old[3]) or (not old[4] and int(match[3]) == int(old[3])):
        raise ValueError('Candidate must not precede the current package')
    if current == version:
        return []
    changes = {}

    def safe_file(name):
        path = root/name
        if path.resolve() != path or not path.is_file():
            raise ValueError('Version metadata must be a regular in-tree file: '+name)
        return path

    def change(name, before, after, count=None):
        text = safe_file(name).read_text()
        if before not in text or (count is not None and text.count(before) != count):
            raise ValueError('Unexpected version metadata shape: '+name)
        changes[name] = text.replace(before, after)

    change('Cargo.toml', 'version = "'+current+'"', 'version = "'+version+'"', 1)
    text = safe_file('Cargo.lock').read_text()
    pattern = r'(\[\[package\]\]\nname = "exitbind"\nversion = ")'+re.escape(current)+'"'
    text, count = re.subn(pattern, lambda m: m[1]+version+'"', text)
    if count != 1:
        raise ValueError('Unexpected package lock metadata')
    changes['Cargo.lock'] = text
    for name in ['plugin.json', 'plugins/exitbind/plugin.json', 'plugins/exitbind/.codex-plugin/plugin.json',
                 'plugins/exitbind/.claude-plugin/plugin.json']:
        change(name, '"version": "'+current+'"', '"version": "'+version+'"', 1)
    change('examples/agents.toml', 'ref = "v'+current+'"', 'ref = "v'+version+'"', 1)
    for name in ['schema/exitbind.schema.json', 'schema/exitbind-run-boundary.schema.json',
                 'schema/exitbind-harness-manifest.schema.json']:
        change(name, '/v'+current+'/', '/v'+version+'/')
    change('install.sh', '${EXITBIND_VERSION:-v'+current+'}', '${EXITBIND_VERSION:-v'+version+'}', 1)
    change('tooling/repository/release/check-plugin-distribution.py', 'VERSION = "'+current+'"', 'VERSION = "'+version+'"', 1)
    change('tooling/repository/ci/ci-wsl.sh', 'test "$(exitbind version)" = "'+current+'"',
           'test "$(exitbind version)" = "'+version+'"', 1)
    # Keep the existing current-version ordering fixture coherent with its scanner.
    change('src/distribution/update/version_policy_tests.rs', '"v'+current+'"', '"v'+version+'"')
    policy = safe_file('docs/release-version-policy.md').read_text()
    declaration = 'Source candidate tag: v'+current
    if policy.count(declaration) != 1:
        raise ValueError('Unexpected candidate declaration')
    policy = policy.replace(declaration, 'Source candidate tag: v'+version, 1)
    start = policy.index('\n\n', policy.index('Source candidate tag:'))
    end = policy.index('README install links', start)
    policy = policy[:start]+'\n\nThe source candidate is '+version+'; published stable installation links remain pinned.\n'+policy[end:]
    changes['docs/release-version-policy.md'] = policy
    changelog = safe_file('CHANGELOG.md').read_text()
    if not changelog.startswith('# Changelog\n\n'):
        raise ValueError('Unexpected changelog heading')
    changes['CHANGELOG.md'] = '# Changelog\n\n## '+version+'\n\nPaired prerelease candidate; Linux validation and assets only.\n\n'+changelog[len('# Changelog\n\n'):]
    # All shapes are checked before the first write; published pointers/history stay intact.
    for name, text in changes.items():
        (root/name).write_text(text)
    return sorted(changes)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('version')
    parser.add_argument('--root', default='.')
    args = parser.parse_args()
    prepare(args.root, args.version)
