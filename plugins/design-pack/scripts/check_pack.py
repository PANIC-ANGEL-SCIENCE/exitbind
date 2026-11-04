#!/usr/bin/env python3
"""Validate this skills-only package, not design quality or host compatibility."""
import argparse
import json
import re
import shutil
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
REFS = ('briefing roles lifecycle direction reference-analysis elements color interface diagrams communication-design '
        'physical-design motion-sound spatial validation memory exitbind sources').split()
EXPECTED = {
    'README.md', 'LICENSE', 'VALIDATION.md', 'EVALUATION.md', 'plugin.json',
    '.claude-plugin/plugin.json', '.codex-plugin/plugin.json',
    'scripts/check_pack.py', 'scripts/package.py',
    'skills/design-pack/SKILL.md',
    'skills/design-pack/references/source-records.json',
    *(f'skills/design-pack/references/{name}.md' for name in REFS),
    *(f'skills/design-pack/templates/{name}.md' for name in ('brief', 'project-profile', 'handoff')),
}
REPOS = set('Leonxlnx/taste-skill emilkowalski/skills pbakaus/impeccable '
    'nextlevelbuilder/ui-ux-pro-max-skill cathrynlavery/diagram-design Caho1/open-claude-ui '
    'storytold/craft-anything penpot/penpot heygen-com/hyperframes remotion-dev/remotion '
    'howseen-ai/claude-motion-design Leonxlnx/cinetic Vincentwei1021/video-shotcraft '
    'lemomo-ai/lemo-opuscar yihui-dev/awesome-opus5-5-videos Leonxlnx/earshot '
    'ahujasid/mcp-for-blender img2threejs/img2threejs'.split())

def check(root):
    errors = []
    files = set()
    for path in root.rglob('*'):
        rel = path.relative_to(root).as_posix()
        if path.is_symlink():
            errors.append(f'symlink: {rel}')
        elif path.is_file():
            files.add(rel)
    if files != EXPECTED:
        errors.append(f'inventory: missing={sorted(EXPECTED-files)}, unexpected={sorted(files-EXPECTED)}')
    for rel in sorted(files & EXPECTED):
        path = root / rel
        try:
            content = path.read_text(encoding='utf-8')
            if not content.strip():
                errors.append(f'empty: {rel}')
            if path.suffix == '.json':
                json.loads(content)
            if path.suffix == '.md':
                for link in re.findall(r'\]\(([^)]+)\)', content):
                    if link.startswith(('https://', 'http://', '#')):
                        continue
                    target = (path.parent / link.split('#')[0]).resolve()
                    if not target.is_relative_to(root.resolve()) or not target.is_file():
                        errors.append(f'local link: {rel} -> {link}')
                for private_marker in ('libfile_', 'Sentinel_', '/workspace/'):
                    if private_marker in content:
                        errors.append(f'private marker in {rel}')
        except (OSError, UnicodeError, ValueError) as exc:
            errors.append(f'{rel}: {exc}')
    try:
        manifests = [json.loads((root/p).read_text()) for p in
            ('plugin.json', '.claude-plugin/plugin.json', '.codex-plugin/plugin.json')]
        for key in ('name', 'version', 'description', 'author', 'license', 'keywords'):
            if any(m.get(key) != manifests[0].get(key) for m in manifests[1:]):
                errors.append(f'manifest mismatch: {key}')
        if manifests[0]['name'] != 'design-pack' or manifests[0]['version'] != '0.1.0':
            errors.append('unexpected identity/version')
        for m in manifests:
            if any(k in m for k in ('hooks','mcpServers','commands','agents','apps')):
                errors.append('unexpected executable/integration manifest surface')
        for m in manifests[1:]:
            if m.get('skills') != './skills/':
                errors.append('wrong skill path')
        if manifests[0]['extensions']['com.openai']['interface'] != manifests[2]['interface']:
            errors.append('interface mismatch')
        skill = (root/'skills/design-pack/SKILL.md').read_text()
        if not skill.startswith('---\nname: design-pack\ndescription: ') or '\n---\n' not in skill[4:]:
            errors.append('invalid skill frontmatter')
        if len(skill.splitlines()) > 160:
            errors.append('entrypoint too large')
        for ref in REFS:
            if f'references/{ref}.md' not in skill:
                errors.append(f'unrouted reference: {ref}')
        records = json.loads((root/'skills/design-pack/references/source-records.json').read_text())
        if len(records) != 18 or {r['repository'] for r in records} != REPOS:
            errors.append('source coverage mismatch')
        notes = (root/'skills/design-pack/references/sources.md').read_text()
        for r in records:
            if r['url'] not in notes:
                errors.append('source without lineage note')
            for field in ('readme_blob_sha','license_blob_sha'):
                if not re.fullmatch('[0-9a-f]{40}', r.get(field,'')):
                    errors.append(f'invalid source evidence: {r["repository"]} {field}')
    except (OSError, ValueError, KeyError, TypeError) as exc:
        errors.append(f'structure: {exc}')
    return errors

def self_test():
    mutations = {
        'broken-link': lambda p: (p/'README.md').write_text('[broken](missing.md)'),
        'manifest-drift': lambda p: (p/'.claude-plugin/plugin.json').write_text('{}'),
        'missing-source': lambda p: (p/'skills/design-pack/references/source-records.json').write_text('[]'),
        'private-extra': lambda p: (p/'private-notes.txt').write_text('private'),
        'symlink': lambda p: (p/'outside').symlink_to('/tmp'),
    }
    for name, mutate in mutations.items():
        with tempfile.TemporaryDirectory(prefix='design-pack-test-') as tmp:
            p = Path(tmp)/'pack'
            shutil.copytree(ROOT,p,ignore=shutil.ignore_patterns('__pycache__'))
            mutate(p)
            if not check(p):
                raise RuntimeError(f'negative test failed: {name}')
            print(f'PASS reject {name}')

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--self-test',action='store_true')
    args=parser.parse_args()
    errors=check(ROOT)
    if errors:
        for error in errors: print('FAIL',error)
        return 1
    print(f'PASS {len(EXPECTED)} files, manifests, routes, links, 18 source records')
    if args.self_test: self_test()
    return 0

if __name__ == '__main__':
    raise SystemExit(main())
