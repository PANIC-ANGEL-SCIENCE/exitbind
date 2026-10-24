#!/usr/bin/env python3
"""Prepare coherent metadata for one explicitly chosen candidate version."""
import argparse
import importlib.util
from pathlib import Path


def load_sibling(name, file_name):
    # Siblings come from this (trusted) tooling directory, never from the candidate.
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(file_name))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def load_channel():
    return load_sibling('release_channel', 'release-channel.py')


def release_line(version):
    """The first line of a CHANGELOG entry: its release tag, true before and after publish."""
    return ('Release: https://github.com/PANIC-ANGEL-SCIENCE/exitbind/releases/tag/v'+version
            + " \u2014 the tag sits on this entry's release commit, and the page lists its assets"
            ' once they are published.')


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
    # The version inventory rewrites the lock file, manifests, schemas, the
    # installer default, fixtures and every current documentation mention. It
    # validates the whole inventory before returning, so nothing is written yet.
    # Release-source rules come from this trusted checkout; candidate document
    # rules may not touch the paths paired integration protects.
    trusted_root = Path(__file__).resolve().parents[3]
    protected = load_sibling('paired_prerelease', 'paired-prerelease.py').PROTECTED
    changes.update(load_sibling('sync_doc_versions', 'sync-doc-versions.py').plan(
        root, version, trusted_root=trusted_root, protected=protected))
    changelog = safe_file('CHANGELOG.md').read_text()
    if not changelog.startswith('# Changelog\n\n'):
        raise ValueError('Unexpected changelog heading')
    # The entry's first line names its tag, as check-release-refs.sh requires:
    # it stays true after the tag is published, so the entry is never rewritten.
    changes['CHANGELOG.md'] = ('# Changelog\n\n## '+version+'\n\n'+release_line(version)+'\n\n'
                               'Paired prerelease candidate; Linux validation and assets only.\n\n'
                               + changelog[len('# Changelog\n\n'):])
    # All shapes are checked before the first write; the version-free install
    # command and the release history stay intact.
    for name, text in changes.items():
        (root/name).write_text(text)
    return sorted(changes)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('version')
    parser.add_argument('--root', default='.')
    args = parser.parse_args()
    prepare(args.root, args.version)
