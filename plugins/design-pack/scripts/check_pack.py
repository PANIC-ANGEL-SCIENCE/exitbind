#!/usr/bin/env python3
"""Validate this skills-only package, not design quality or host compatibility."""
import argparse
import json
import re
import shutil
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
REFS = ('briefing roles lifecycle direction reference-analysis visual-methods aesthetic-construction typography preview-evidence first-pass elements color interface interface-craft interface-source-notes diagrams communication-design '
        'physical-design motion-sound spatial validation memory exitbind sources').split()
EXPECTED = {
    'README.md', 'LICENSE', 'VALIDATION.md', 'EVALUATION.md', 'plugin.json',
    '.claude-plugin/plugin.json', '.codex-plugin/plugin.json',
    'scripts/check_pack.py', 'scripts/package.py',
    'skills/design-pack/SKILL.md',
    'skills/design-pack/references/source-records.json',
    'skills/design-pack/references/web-source-records.json',
    'skills/design-pack/references/aesthetic-source-records.json',
    'skills/design-pack/references/interface-source-records.json',
    'skills/mockup-image/SKILL.md',
    'skills/mockup-image/references/image-production.md',
    'skills/mockup-image/templates/image-brief.md',
    *(f'skills/design-pack/references/{name}.md' for name in REFS),
    *(f'skills/design-pack/templates/{name}.md' for name in ('brief', 'project-profile', 'handoff')),
}
REPOS = set('Leonxlnx/taste-skill emilkowalski/skills pbakaus/impeccable '
    'nextlevelbuilder/ui-ux-pro-max-skill cathrynlavery/diagram-design Caho1/open-claude-ui '
    'storytold/craft-anything penpot/penpot heygen-com/hyperframes remotion-dev/remotion '
    'howseen-ai/claude-motion-design Leonxlnx/cinetic Vincentwei1021/video-shotcraft '
    'lemomo-ai/lemo-opuscar yihui-dev/awesome-opus5-5-videos Leonxlnx/earshot '
    'ahujasid/mcp-for-blender img2threejs/img2threejs'.split())

WEB_URLS = set(('https://www.dark.design/ https://variant.com/ https://www.happyhues.co/ '
                'https://haikei.app/ https://shadergradient.co/ https://grainient.supply/ '
                'https://reactbits.dev/').split())

def check_web_records(records, notes):
    errors = []
    if not isinstance(records, list) or len(records) != 7:
        return ['public source coverage mismatch']
    if any(not isinstance(r, dict) for r in records):
        return ['invalid public source record']
    urls = [r.get('url') for r in records]
    if any(not isinstance(url, str) for url in urls) or set(urls) != WEB_URLS:
        errors.append('public source coverage mismatch')
    for record in records:
        for field in ('id', 'url', 'inspected_on', 'evidence_type', 'observation',
                      'transfer', 'rights', 'limitations'):
            value = record.get(field)
            if not isinstance(value, str) or not value.strip():
                errors.append(f'public source missing field: {field}')
        # This release contains text evidence, not locally rendered evidence.
        if record.get('evidence_type') != 'primary-text-extraction':
            errors.append('public source evidence type drift')
        if record.get('local_render_verified') is not False:
            errors.append('public source rendered claim drift')
        url = record.get('url')
        if isinstance(url, str) and url not in notes:
            errors.append('public source without lineage note')
    return errors


# Exact source identities and evidence categories for this bounded release.
AESTHETIC_SOURCES = {
    'foundry': ('reported-rendered', 'snapshot', 'https://cms-font-discovery-41302026.lovable.app/'),
    'maison': ('reported-rendered', 'snapshot', 'https://scandi-haven-shop.lovable.app/product/arc-pendant-light'),
    'prompt-frame': ('reported-rendered', 'snapshot', 'https://lovable-prompt-frame.lovable.app/work'),
    'void': ('reported-rendered', 'partial', 'https://void-storefront-design.lovable.app/product/oversize-shirt'),
    'expense': ('reported-access-failure', 'failed', 'https://expense-it.lovable.app/login'),
    'local-preview-failures': ('reported-access-failure', 'failed', 'https://graphic-designer-05.lovable.app/'),
    'landing-claims': ('reported-primary-text', 'text-only', 'https://lovable.dev/templates/websites/portfolio/graphic-designer-5'),
    'taste-type': ('reported-primary-text', 'text-only', 'https://github.com/Leonxlnx/taste-skill/blob/main/skills/taste-skill/library/type.json'),
    'emil-type': ('reported-primary-text', 'text-only', 'https://github.com/emilkowalski/skills/blob/main/ui-cheatsheet.md'),
    'impeccable-methods': ('reported-primary-text', 'text-only', 'https://github.com/pbakaus/impeccable/blob/main/.agents/skills/impeccable/reference/typeset.md'),
    'impeccable-gallery': ('reported-rendered', 'snapshot', 'https://impeccable.style/gallery/'),
    'taste-web': ('reported-rendered', 'snapshot', 'https://www.tasteskill.dev/'),
    'seven-site-revisit': ('reported-primary-text', 'text-only', 'https://www.happyhues.co/'),
    'typography-drafts': ('reported-primary-text', 'text-only', 'https://www.w3.org/TR/klreq/'),
    'font-metadata': ('reported-primary-text', 'text-only', 'https://github.com/google/fonts/blob/main/ofl/notosanskr/METADATA.pb'),
}

def check_aesthetic_records(records, notes):
    errors = []
    if not isinstance(records, list) or any(not isinstance(r, dict) for r in records):
        return ['invalid aesthetic records']
    ids = [r.get('id') for r in records]
    if any(not isinstance(i, str) for i in ids):
        return ['invalid aesthetic identity']
    if len(ids) != len(AESTHETIC_SOURCES) or set(ids) != set(AESTHETIC_SOURCES):
        errors.append('aesthetic source coverage mismatch')
    for r in records:
        for field in ('id', 'inspected_on', 'reporter', 'observations', 'author_claims',
                      'transfer_hypotheses', 'limitations', 'rights'):
            if not isinstance(r.get(field), str) or not r[field].strip():
                errors.append(f'aesthetic missing field: {field}')
        urls = r.get('source_urls')
        if not isinstance(urls, list) or not urls or any(
                not isinstance(u, str) or not u.startswith('https://') for u in urls):
            errors.append('invalid aesthetic URLs')
            urls = []
        if len(urls) != len(set(urls)):
            errors.append('duplicate aesthetic URL')
        expected = AESTHETIC_SOURCES.get(r.get('id'))
        if expected and (r.get('evidence_type'), r.get('access_status'), urls[0] if urls else None) != expected:
            errors.append('aesthetic evidence identity drift')
        if r.get('local_render_verified') is not False:
            errors.append('aesthetic local render claim drift')
        if r.get('interaction_tested') is not False:
            errors.append('aesthetic interaction claim drift')
        # Unknown viewport must remain explicit, never filled from failed local attempts.
        expected_view = 'Requested 1440x1000; no usable render' if r.get('id') == 'local-preview-failures' else None
        if 'viewport' not in r or r['viewport'] != expected_view:
            errors.append('aesthetic viewport provenance drift')
        if any(u not in notes for u in urls):
            errors.append('aesthetic source without lineage note')
    return errors


INTERFACE_COMMIT = '60bf140e43c98b248e6594487384ec967e89d98a'
INTERFACE_SKILLS = set(('better-interface better-ui better-layout better-typography '
                        'better-colors better-writing build-design break state-machine').split())

def check_interface_record(record, notes):
    if not isinstance(record, dict):
        return ['invalid interface record']
    errors = []
    if (record.get('repository') != 'jakubkrehel/skills' or
            record.get('url') != 'https://github.com/jakubkrehel/skills' or
            record.get('commit') != INTERFACE_COMMIT):
        errors.append('interface source identity drift')
    if record.get('evidence_type') != 'reported-primary-text':
        errors.append('interface evidence type drift')
    if record.get('readme_skill_count') != 14:
        errors.append('interface scope count drift')
    for flag in ('local_render_verified', 'upstream_executed'):
        if record.get(flag) is not False:
            errors.append(f'interface unsupported claim: {flag}')
    for field in ('inspected_on', 'observation', 'transfer', 'limitations',
                  'excluded_prescriptions', 'image_methods_scope'):
        if not isinstance(record.get(field), str) or not record[field].strip():
            errors.append(f'interface missing field: {field}')
    if record.get('image_methods_origin') != 'original-pack-synthesis':
        errors.append('interface image attribution drift')
    license = record.get('license')
    if not isinstance(license, dict):
        errors.append('invalid interface license')
    else:
        if (license.get('spdx') != 'MIT' or license.get('url') !=
                f'https://github.com/jakubkrehel/skills/blob/{INTERFACE_COMMIT}/LICENSE'):
            errors.append('interface license provenance drift')
        if not isinstance(license.get('scope'), str) or not license['scope'].strip():
            errors.append('interface missing license scope')
    skills = record.get('inspected_skills')
    if not isinstance(skills, list) or any(not isinstance(v, dict) for v in skills):
        return errors + ['invalid interface skills']
    names = [v.get('name') for v in skills]
    if any(not isinstance(n, str) for n in names):
        return errors + ['invalid interface skill name']
    if len(names) != 9 or set(names) != INTERFACE_SKILLS:
        errors.append('interface inspected scope drift')
    for item in skills:
        url = f'https://github.com/jakubkrehel/skills/blob/{INTERFACE_COMMIT}/skills/{item.get("name")}/SKILL.md'
        if item.get('url') != url:
            errors.append('interface skill source drift')
        if not isinstance(item.get('sha256'), str) or not re.fullmatch('[0-9a-f]{64}', item['sha256']):
            errors.append('invalid interface source digest')
        if url not in notes:
            errors.append('interface source without lineage note')
    return errors

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
        if manifests[0]['name'] != 'design-pack' or manifests[0]['version'] != '0.4.0':
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
        image_skill = (root/'skills/mockup-image/SKILL.md').read_text()
        if not image_skill.startswith('---\nname: mockup-image\ndescription: ') or '\n---\n' not in image_skill[4:]:
            errors.append('invalid image skill frontmatter')
        if len(image_skill.splitlines()) > 100:
            errors.append('image entrypoint too large')
        for link in ('../design-pack/SKILL.md', 'references/image-production.md',
                     'templates/image-brief.md', '../design-pack/references/first-pass.md'):
            if f']({link})' not in image_skill:
                errors.append(f'unrouted image dependency: {link}')
        if '](../mockup-image/SKILL.md)' not in skill:
            errors.append('unrouted image skill')
        interface_record = json.loads((root/'skills/design-pack/references/interface-source-records.json').read_text())
        interface_notes = (root/'skills/design-pack/references/interface-source-notes.md').read_text()
        errors.extend(check_interface_record(interface_record, interface_notes))
        records = json.loads((root/'skills/design-pack/references/source-records.json').read_text())
        if len(records) != 18 or {r['repository'] for r in records} != REPOS:
            errors.append('source coverage mismatch')
        notes = (root/'skills/design-pack/references/sources.md').read_text()
        web_records = json.loads((root/'skills/design-pack/references/web-source-records.json').read_text())
        errors.extend(check_web_records(web_records, notes))
        aesthetic = json.loads((root/'skills/design-pack/references/aesthetic-source-records.json').read_text())
        focused_notes = notes + (root/'skills/design-pack/references/preview-evidence.md').read_text() + (root/'skills/design-pack/references/typography.md').read_text()
        errors.extend(check_aesthetic_records(aesthetic, focused_notes))
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
    interface_path = 'skills/design-pack/references/interface-source-records.json'
    interface_record = json.loads((ROOT/interface_path).read_text())
    interface_notes = (ROOT/'skills/design-pack/references/interface-source-notes.md').read_text()
    alternate = json.loads(json.dumps(interface_record))
    alternate['transfer'] = 'Use a different contextual composition while preserving evidence limits.'
    if check_interface_record(interface_record, interface_notes) or check_interface_record(alternate, interface_notes):
        raise RuntimeError('positive interface provenance rejected')
    print('PASS positive pinned interface scope and contextual transfer')
    changes = {
        'interface-unpinned': (lambda r: r.update(commit='main'), 'interface source identity drift'),
        'interface-all-skills-claim': (lambda r: r['inspected_skills'].append(dict(r['inspected_skills'][0])), 'interface inspected scope drift'),
        'interface-missing-skill': (lambda r: r['inspected_skills'].pop(), 'interface inspected scope drift'),
        'interface-wrong-path': (lambda r: r['inspected_skills'][0].update(url='https://example.com/SKILL.md'), 'interface skill source drift'),
        'interface-bad-digest': (lambda r: r['inspected_skills'][0].update(sha256='unknown'), 'invalid interface source digest'),
        'interface-render-claim': (lambda r: r.update(local_render_verified=True), 'interface unsupported claim: local_render_verified'),
        'interface-execution-claim': (lambda r: r.update(upstream_executed=True), 'interface unsupported claim: upstream_executed'),
        'interface-missing-limit': (lambda r: r.pop('limitations'), 'interface missing field: limitations'),
        'interface-image-misattribution': (lambda r: r.update(image_methods_origin='upstream-photography'), 'interface image attribution drift'),
        'interface-license-scope': (lambda r: r['license'].pop('scope'), 'interface missing license scope'),
    }
    for name, (change, expected) in changes.items():
        with tempfile.TemporaryDirectory(prefix='design-pack-test-') as tmp:
            p = Path(tmp)/'pack'
            shutil.copytree(ROOT, p, ignore=shutil.ignore_patterns('__pycache__'))
            record = json.loads((p/interface_path).read_text())
            change(record)
            (p/interface_path).write_text(json.dumps(record))
            if expected not in check(p):
                raise RuntimeError(f'negative test missed expected reason: {name}')
            print(f'PASS reject {name}')
    route_cases = [
        ('skills/design-pack/SKILL.md', '../mockup-image/SKILL.md', 'unrouted image skill'),
        ('skills/mockup-image/SKILL.md', '../design-pack/references/first-pass.md', 'unrouted image dependency: ../design-pack/references/first-pass.md'),
        ('skills/mockup-image/SKILL.md', 'references/image-production.md', 'unrouted image dependency: references/image-production.md'),
        ('skills/mockup-image/SKILL.md', 'templates/image-brief.md', 'unrouted image dependency: templates/image-brief.md'),
        ('skills/design-pack/SKILL.md', 'references/interface-craft.md', 'unrouted reference: interface-craft'),
        ('skills/design-pack/SKILL.md', 'references/interface-source-notes.md', 'unrouted reference: interface-source-notes'),
    ]
    for path, link, expected in route_cases:
        with tempfile.TemporaryDirectory(prefix='design-pack-test-') as tmp:
            p = Path(tmp)/'pack'
            shutil.copytree(ROOT, p, ignore=shutil.ignore_patterns('__pycache__'))
            target = p/path
            target.write_text(target.read_text().replace(link, '#removed-route'))
            if expected not in check(p):
                raise RuntimeError(f'negative route test missed expected reason: {link}')
            print(f'PASS reject route {link}')

    # Positive coverage is explicit: reported snapshots, partial/failed access,
    # and source text must all coexist without being flattened to local renders.
    records = json.loads((ROOT/'skills/design-pack/references/aesthetic-source-records.json').read_text())
    notes = '\n'.join((ROOT/f'skills/design-pack/references/{n}.md').read_text()
                      for n in ('sources', 'preview-evidence', 'typography'))
    if check_aesthetic_records(records, notes):
        raise RuntimeError('positive aesthetic evidence rejected')
    alternative = json.loads(json.dumps(records))
    alternative[0]['transfer_hypotheses'] = 'Consider a different expressive voice; no fixed aesthetic is required.'
    if check_aesthetic_records(alternative, notes):
        raise RuntimeError('checker encoded taste rather than provenance')
    print('PASS positive mixed provenance and alternate aesthetic hypothesis')

    def mutate_aesthetic(p, change):
        path = p/'skills/design-pack/references/aesthetic-source-records.json'
        data = json.loads(path.read_text())
        change(data)
        path.write_text(json.dumps(data))

    focused = {
        'missing-aesthetic-source': (lambda a: a.pop(), 'aesthetic source coverage mismatch'),
        'duplicate-aesthetic-source': (lambda a: a.__setitem__(0, a[1]), 'aesthetic source coverage mismatch'),
        'wrong-prompt-frame': (lambda a: a[2]['source_urls'].__setitem__(0, 'https://jordan-studio.lovable.app/'), 'aesthetic evidence identity drift'),
        'reported-to-local': (lambda a: a[0].update(local_render_verified=True), 'aesthetic local render claim drift'),
        'failed-to-success': (lambda a: a[4].update(access_status='snapshot', evidence_type='reported-rendered'), 'aesthetic evidence identity drift'),
        'partial-to-complete': (lambda a: a[3].update(access_status='snapshot'), 'aesthetic evidence identity drift'),
        'marketing-to-render': (lambda a: a[6].update(evidence_type='reported-rendered'), 'aesthetic evidence identity drift'),
        'invented-interaction': (lambda a: a[1].update(interaction_tested=True), 'aesthetic interaction claim drift'),
        'borrowed-viewport': (lambda a: a[0].update(viewport='1440x1000'), 'aesthetic viewport provenance drift'),
        'missing-focused-limit': (lambda a: a[0].pop('limitations'), 'aesthetic missing field: limitations'),
        'missing-rights': (lambda a: a[0].pop('rights'), 'aesthetic missing field: rights'),
        'missing-transfer-boundary': (lambda a: a[0].pop('transfer_hypotheses'), 'aesthetic missing field: transfer_hypotheses'),
    }
    for name, (change, expected) in focused.items():
        with tempfile.TemporaryDirectory(prefix='design-pack-test-') as tmp:
            p = Path(tmp)/'pack'
            shutil.copytree(ROOT, p, ignore=shutil.ignore_patterns('__pycache__'))
            mutate_aesthetic(p, change)
            if expected not in check(p):
                raise RuntimeError(f'negative test missed expected reason: {name}')
            print(f'PASS reject {name}')
    for ref in ('aesthetic-construction', 'typography', 'preview-evidence'):
        with tempfile.TemporaryDirectory(prefix='design-pack-test-') as tmp:
            p = Path(tmp)/'pack'
            shutil.copytree(ROOT, p, ignore=shutil.ignore_patterns('__pycache__'))
            skill = p/'skills/design-pack/SKILL.md'
            skill.write_text(skill.read_text().replace(f'references/{ref}.md', f'#{ref}'))
            if f'unrouted reference: {ref}' not in check(p):
                raise RuntimeError(f'negative route test failed: {ref}')
            print(f'PASS reject unrouted-{ref}')

    def mutate_web(p, change):
        path = p/'skills/design-pack/references/web-source-records.json'
        records = json.loads(path.read_text())
        change(records)
        path.write_text(json.dumps(records))

    mutations = {
        'broken-link': lambda p: (p/'README.md').write_text('[broken](missing.md)'),
        'manifest-drift': lambda p: (p/'.claude-plugin/plugin.json').write_text('{}'),
        'missing-source': lambda p: (p/'skills/design-pack/references/source-records.json').write_text('[]'),
        'missing-public-source': lambda p: mutate_web(p, lambda records: records.pop()),
        'duplicate-public-source': lambda p: mutate_web(p, lambda records: records.__setitem__(0, records[1])),
        'missing-evidence-limit': lambda p: mutate_web(p, lambda records: records[0].pop('limitations')),
        'unsupported-render-claim': lambda p: mutate_web(p, lambda records: records[0].update(local_render_verified=True)),
        'unrouted-method': lambda p: (p/'skills/design-pack/SKILL.md').write_text(
            (p/'skills/design-pack/SKILL.md').read_text().replace('references/first-pass.md', '#first-pass')),
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
    print(f'PASS {len(EXPECTED)} files, manifests, routes, links, 18 repository, 7 original public and 15 focused evidence records and 9 pinned interface skill sources')
    if args.self_test: self_test()
    return 0

if __name__ == '__main__':
    raise SystemExit(main())
