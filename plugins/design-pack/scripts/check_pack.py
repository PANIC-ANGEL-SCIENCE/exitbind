#!/usr/bin/env python3
"""Validate package structure, never aesthetic quality or host loading."""
import argparse
import json
import math
from pathlib import Path
import re
import shutil
import tempfile
from urllib.parse import unquote

ROOT = Path(__file__).resolve().parents[1]
REFS = ('aesthetic-construction brand-system briefing color communication-design '
        'design-judgment diagrams direction discovery elements exitbind first-pass '
        'interface-craft interface lifecycle memory methods motion-sound physical-design '
        'reference-analysis reference-roles roles screen-production portfolio-presentation spatial typography validation visual-methods').split()
ENTRY = 'skills/design-pack/SKILL.md'
JUDGMENT = 'skills/design-pack/references/design-judgment.md'
OVERLAY = 'skills/design-pack/templates/private-reference-overlay.json'
SCREEN_RUN = 'skills/design-pack/templates/screen-run.json'
IMAGE_RUN = 'skills/mockup-image/templates/image-run.json'
OUTCOME = 'skills/design-pack/templates/outcome-review.json'
MANIFESTS = ('plugin.json', '.claude-plugin/plugin.json', '.codex-plugin/plugin.json')
EXPECTED = {
    'README.md', 'LICENSE', 'VALIDATION.md', 'EVALUATION.md', *MANIFESTS,
    'scripts/check_image_run.py', 'scripts/test_check_image_run.py', IMAGE_RUN,
    'scripts/test_age_casting_guidance.py', 'scripts/test_attitude_guidance.py',
    'scripts/test_expression_guidance.py',
    'scripts/test_capture_styling_guidance.py',
    'scripts/test_mirror_selfie_guidance.py',
    'scripts/check_outcome.py', 'scripts/test_check_outcome.py', OUTCOME,
    'skills/mockup-image/references/generation-contract.md',
    'skills/mockup-image/references/model-direction.md',
    'skills/mockup-image/references/capture-styling.md',
    'skills/mockup-image/references/mirror-selfie.md',
    'scripts/check_pack.py', 'scripts/package.py', 'scripts/check_run.py', 'scripts/test_check_run.py', ENTRY, JUDGMENT, OVERLAY,
    'skills/design-pack/templates/screen-run.json',
    'skills/mockup-image/SKILL.md', 'skills/mockup-image/references/image-production.md',
    'skills/mockup-image/templates/image-brief.md',
    *(f'skills/design-pack/references/{name}.md' for name in REFS),
    *(f'skills/design-pack/templates/{name}.md' for name in
      ('brief', 'project-profile', 'handoff', 'brand-system', 'reference-session', 'presentation-brief')),
}
LINK = re.compile(r'\[([^\]]*)\]\(([^)]*)\)')
REMOTE = re.compile(r'(?:https?|ftp)://[^\s<>"\']+', re.I)
UNSUPPORTED_LINK = re.compile(
    r'^\s*\[[^\]]+\]:|\[[^\]]*\]\s*\[[^\]]*\]|<\s*/?\s*a(?:\s|>)|<!--|-->',
    re.I | re.M,
)


class DuplicateJSONKey(ValueError):
    pass


def load_json(content):
    def unique_object(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise DuplicateJSONKey('duplicate JSON object key')
            result[key] = value
        return result
    def invalid_constant(value):
        raise ValueError('non-finite JSON number')
    def finite_float(value):
        result = float(value)
        if not math.isfinite(result):
            raise ValueError('non-finite JSON number')
        return result
    return json.loads(content, object_pairs_hook=unique_object, parse_constant=invalid_constant, parse_float=finite_float)


def valid_skill_metadata(content, name):
    """Check this package's two-field header, not arbitrary YAML frontmatter."""
    match = re.match(r'---\n(.*?)\n---\n', content, re.S)
    if not match:
        return False
    fields = {}
    for line in match.group(1).splitlines():
        key, separator, value = line.partition(':')
        if not separator or key not in ('name', 'description') or key in fields:
            return False
        fields[key] = value.strip()
    if set(fields) != {'name', 'description'} or fields['name'] != name:
        return False
    try:
        description = json.loads(fields['description'])
    except ValueError:
        return False
    return isinstance(description, str) and bool(description.strip())


def check(root):
    root = root.resolve()
    errors = []
    paths = list(root.rglob('*'))
    if any(p.is_symlink() for p in paths):
        return ['symlink in package']
    if {p.relative_to(root).as_posix() for p in paths if p.is_file()} != EXPECTED:
        return ['package inventory mismatch']
    try:
        texts = {rel: (root / rel).read_text(encoding='utf-8') for rel in EXPECTED}
    except (OSError, UnicodeError):
        return ['unreadable or non-UTF-8 package member']
    versions = []
    manifest_data = {}
    for rel in MANIFESTS:
        try:
            data = load_json(texts[rel])
            if not isinstance(data, dict):
                raise ValueError()
            manifest_data[rel] = data
            versions.append(data.get('version'))
            if rel == 'plugin.json' and data.get('$schema') != 'https:' + '/' * 2 + 'agent-plugins.org/schemas/1.0.0/plugin.schema.json':
                errors.append('invalid machine schema locator')
            if data.get('name') != 'design-pack' or data.get('license') != 'MIT':
                errors.append('plugin identity drift')
            if not isinstance(data.get('description'), str) or not data['description'].strip():
                errors.append('missing plugin description')
            if rel != 'plugin.json' and data.get('skills') != './skills/':
                errors.append('plugin skill route drift')
            if any(k in data for k in ('hooks', 'mcpServers', 'agents')):
                errors.append('unexpected executable extension')
        except DuplicateJSONKey:
            errors.append('duplicate JSON key')
        except (ValueError, TypeError):
            errors.append('invalid plugin metadata')
    if len(versions) != 3 or any(not isinstance(v, str) or not re.fullmatch(r'\d+\.\d+\.\d+', v) for v in versions) or len(set(versions)) != 1:
        errors.append('plugin version mismatch')
    if len(manifest_data) == len(MANIFESTS):
        base = manifest_data['plugin.json']
        common = ('name', 'version', 'description', 'author', 'license', 'keywords')
        if any(any(item.get(key) != base.get(key) for key in common)
               for item in manifest_data.values()):
            errors.append('shared manifest metadata mismatch')
        extensions = base.get('extensions')
        openai = extensions.get('com.openai') if isinstance(extensions, dict) else None
        interface = openai.get('interface') if isinstance(openai, dict) else None
        if not isinstance(interface, dict) or interface != manifest_data['.codex-plugin/plugin.json'].get('interface'):
            errors.append('manifest interface mismatch')
        if isinstance(base.get('version'), str) and not texts['README.md'].startswith(
                '# Design Pack ' + base['version'] + ' candidate\n'):
            errors.append('README version mismatch')
    craft_routes = {
        'skills/design-pack/references/screen-production.md':
            ('visual-methods.md', 'interface-craft.md', 'reference-roles.md'),
        'skills/design-pack/references/portfolio-presentation.md':
            ('screen-production.md', 'reference-analysis.md', 'reference-roles.md'),
        'skills/mockup-image/SKILL.md':
            ('references/image-production.md', 'references/generation-contract.md'),
        'skills/mockup-image/references/image-production.md':
            ('generation-contract.md', '../../design-pack/references/reference-roles.md'),
    }
    for source, targets in craft_routes.items():
        for target in targets:
            if '](' + target + ')' not in texts[source]:
                errors.append('missing craft production route')
    for source, target in {
        'skills/mockup-image/SKILL.md': 'references/model-direction.md',
        'skills/mockup-image/references/image-production.md': 'model-direction.md',
        'skills/mockup-image/references/generation-contract.md': 'model-direction.md',
    }.items():
        if '](' + target + ')' not in texts[source]:
            errors.append('missing model direction route')
    for source, target in {
        'skills/mockup-image/SKILL.md': 'references/capture-styling.md',
        'skills/mockup-image/references/image-production.md': 'capture-styling.md',
        'skills/mockup-image/references/generation-contract.md': 'capture-styling.md',
        'skills/mockup-image/references/model-direction.md': 'capture-styling.md',
    }.items():
        if '](' + target + ')' not in texts[source]:
            errors.append('missing capture styling route')
    for source, target in {
        'skills/mockup-image/SKILL.md': 'references/mirror-selfie.md',
        'skills/mockup-image/references/capture-styling.md': 'mirror-selfie.md',
        'skills/mockup-image/references/generation-contract.md': 'mirror-selfie.md',
        'skills/mockup-image/references/model-direction.md': 'mirror-selfie.md',
    }.items():
        if '](' + target + ')' not in texts[source]:
            errors.append('missing mirror selfie route')
    for name in ('design-pack', 'mockup-image'):
        if not valid_skill_metadata(texts[f'skills/{name}/SKILL.md'], name):
            errors.append('invalid skill metadata')
    if '](references/screen-production.md)' not in texts[ENTRY]:
        errors.append('missing direct screen production route')
    presentation_routes = {
        ENTRY: 'references/portfolio-presentation.md',
        'skills/design-pack/references/methods.md': 'portfolio-presentation.md',
        'skills/design-pack/references/reference-analysis.md': 'portfolio-presentation.md',
        'skills/design-pack/references/screen-production.md': 'portfolio-presentation.md',
        'skills/mockup-image/SKILL.md': '../design-pack/references/portfolio-presentation.md',
        'skills/mockup-image/references/generation-contract.md': '../../design-pack/references/portfolio-presentation.md',
        'skills/design-pack/references/portfolio-presentation.md': '../templates/presentation-brief.md',
        'skills/design-pack/references/color.md': 'portfolio-presentation.md',
    }
    for source, target in presentation_routes.items():
        if '](' + target + ')' not in texts[source]:
            errors.append('missing portfolio presentation route')
    for source, target in {
        'skills/design-pack/references/reference-analysis.md': 'reference-roles.md',
        'skills/mockup-image/references/generation-contract.md': '../../design-pack/references/reference-roles.md',
    }.items():
        if '](' + target + ')' not in texts[source]:
            errors.append('missing source-role route')
    try:
        screen_run = load_json(texts[SCREEN_RUN])
        if not isinstance(screen_run, dict) or set(screen_run) != {'schema_version', 'mode', 'maker_id', 'pack_version', 'events', 'event_shapes'} or screen_run.get('schema_version') != 1 or screen_run.get('events') != [] or any(screen_run.get(k) is not None for k in ('mode', 'maker_id', 'pack_version')):
            errors.append('screen run template is populated or invalid')
        shapes = screen_run.get('event_shapes') if isinstance(screen_run, dict) else None
        def blank(value, key=None):
            if key == 'kind':
                return value in ('reference_view', 'direction', 'render', 'review')
            if isinstance(value, dict):
                return all(blank(v, k) for k, v in value.items())
            if isinstance(value, list):
                return all(blank(v) for v in value)
            return value is None
        if not isinstance(shapes, dict) or set(shapes) != {'reference_view', 'direction', 'render', 'review'} or not blank(shapes):
            errors.append('screen run template is populated or invalid')
    except DuplicateJSONKey:
        errors.append('duplicate JSON key')
    except (ValueError, TypeError):
        errors.append('invalid screen run template')
    try:
        image_run = load_json(texts[IMAGE_RUN])
        def image_blank(value, key=None):
            if key == 'kind':
                return value in ('reference_view', 'generation', 'freeze', 'review')
            if isinstance(value, dict):
                return all(image_blank(v, k) for k, v in value.items())
            if isinstance(value, list):
                return all(image_blank(v) for v in value)
            return value is None
        valid_image = (
            isinstance(image_run, dict) and
            set(image_run) == {'schema_version', 'mode', 'maker_id', 'pack_version', 'target_viewport', 'events', 'event_shapes'} and
            type(image_run.get('schema_version')) is int and image_run['schema_version'] == 1 and
            image_run.get('events') == [] and
            image_run.get('target_viewport') == {'width': None, 'height': None} and
            all(image_run.get(k) is None for k in ('mode', 'maker_id', 'pack_version')) and
            isinstance(image_run.get('event_shapes'), dict) and
            set(image_run['event_shapes']) == {'reference_view', 'generation', 'freeze', 'review'} and
            image_blank(image_run['event_shapes']))
        if not valid_image:
            errors.append('image run template is populated or invalid')
    except DuplicateJSONKey:
        errors.append('duplicate JSON key')
    except (ValueError, TypeError):
        errors.append('invalid image run template')
    try:
        outcome = load_json(texts[OUTCOME])
        if outcome != {'schema_version': 1, 'mode': None, 'status': None, 'contract_source': None,
                       'requirements': [], 'artifacts': [], 'views': [], 'checks': [], 'sequence': None} or type(outcome.get('schema_version')) is not int:
            errors.append('outcome review template is populated or invalid')
    except DuplicateJSONKey:
        errors.append('duplicate JSON key')
    except (ValueError, TypeError, AttributeError):
        errors.append('invalid outcome review template')
    sizes = [(root / rel).stat().st_size for rel in (ENTRY, JUDGMENT)]
    if any(size > 4096 for size in sizes) or sum(sizes) > 6144:
        errors.append('compact guidance exceeds selected resource budget')
    if '`' in texts[ENTRY] or re.search(r'(?m)^ {0,3}~{3,}', texts[ENTRY]):
        errors.append('unsupported entrypoint code markup')
    graph = {}
    for rel, content in texts.items():
        inspected = content
        if rel == 'plugin.json':
            try:
                schema = load_json(content).get('$schema')
                if isinstance(schema, str):
                    inspected = inspected.replace(schema, '')
            except (ValueError, AttributeError):
                pass
        if REMOTE.search(inspected):
            errors.append('external locator in reusable content')
        if not rel.endswith('.md'):
            continue
        if UNSUPPORTED_LINK.search(content):
            errors.append('unsupported documentation link form')
        graph[rel] = []
        for label, raw in LINK.findall(content):
            if '\n' in label or '\n' in raw:
                errors.append('unsupported documentation link form')
                continue
            target = unquote(raw.strip().split(' "', 1)[0]).split('#', 1)[0]
            if not target:
                continue
            if re.match(r'^[A-Za-z][\w+.-]*:', target) or target.startswith('/'):
                errors.append('nonlocal documentation route')
                continue
            path = ((root / rel).parent / target).resolve()
            if not path.is_relative_to(root):
                errors.append('escaped documentation route')
            elif path.relative_to(root).as_posix() not in EXPECTED or not path.is_file():
                errors.append('broken documentation route')
            else:
                graph[rel].append(path.relative_to(root).as_posix())
    if 'skills/design-pack/references/discovery.md' not in graph.get(ENTRY, []):
        errors.append('missing required entrypoint discovery route')
    visited, pending = set(), ['README.md']
    while pending:
        rel = pending.pop()
        if rel not in visited:
            visited.add(rel)
            pending.extend(graph.get(rel, []))
    if any(rel.endswith('.md') and rel not in visited for rel in EXPECTED):
        errors.append('unreachable method or template')
    try:
        overlay = load_json(texts[OVERLAY])
        expected = {'schema_version': 1, 'visibility': 'project-private', 'owner_authorization': None,
                    'allowed_recipients': [], 'board_locator': None, 'board_access': 'unresolved', 'items': []}
        if not isinstance(overlay, dict) or any(overlay.get(k) != v for k, v in expected.items()) or set(overlay) != set(expected) | {'item_shape'}:
            errors.append('private template is populated or invalid')
        shape = overlay.get('item_shape') if isinstance(overlay, dict) else None
        if not isinstance(shape, dict) or shape.get('evidence_type') != 'uninspected' or shape.get('rights') != 'unknown' or shape.get('access_status') != 'unresolved' or shape.get('allowed_recipients') != [] or any(v is not None for k, v in shape.items() if k not in ('evidence_type', 'rights', 'access_status', 'allowed_recipients')):
            errors.append('private item template is populated or invalid')
    except DuplicateJSONKey:
        errors.append('duplicate JSON key')
    except (ValueError, TypeError):
        errors.append('invalid private template')
    if any(term not in texts['LICENSE'] for term in ('MIT License', 'Copyright (c) 2026 Design Pack contributors', 'The above copyright notice and this permission notice')):
        errors.append('required license notice missing')
    return sorted(set(errors))


def self_test():
    """Corrupt copies to exercise concrete distribution boundary refusals."""
    def edit(rel, transform):
        def mutation(root):
            path = root / rel
            path.write_text(transform(path.read_text()))
        return mutation

    cases = [
        ('missing mirror selfie contracts', lambda r: (r / 'scripts/test_mirror_selfie_guidance.py').unlink(), 'package inventory mismatch'),
        ('missing mirror selfie method', lambda r: (r / 'skills/mockup-image/references/mirror-selfie.md').unlink(), 'package inventory mismatch'),
        ('mirror entry route', edit('skills/mockup-image/SKILL.md', lambda s: s.replace('](references/mirror-selfie.md)', '](references/image-production.md)')), 'missing mirror selfie route'),
        ('mirror capture route', edit('skills/mockup-image/references/capture-styling.md', lambda s: s.replace('](mirror-selfie.md)', '](generation-contract.md)')), 'missing mirror selfie route'),
        ('mirror generation route', edit('skills/mockup-image/references/generation-contract.md', lambda s: s.replace('](mirror-selfie.md)', '](image-production.md)')), 'missing mirror selfie route'),
        ('mirror model route', edit('skills/mockup-image/references/model-direction.md', lambda s: s.replace('](mirror-selfie.md)', '](image-production.md)')), 'missing mirror selfie route'),
        ('missing capture styling contracts', lambda r: (r / 'scripts/test_capture_styling_guidance.py').unlink(), 'package inventory mismatch'),
        ('missing capture styling method', lambda r: (r / 'skills/mockup-image/references/capture-styling.md').unlink(), 'package inventory mismatch'),
        ('capture entry route', edit('skills/mockup-image/SKILL.md', lambda s: s.replace('](references/capture-styling.md)', '](references/image-production.md)')), 'missing capture styling route'),
        ('capture production route', edit('skills/mockup-image/references/image-production.md', lambda s: s.replace('](capture-styling.md)', '](generation-contract.md)')), 'missing capture styling route'),
        ('capture generation route', edit('skills/mockup-image/references/generation-contract.md', lambda s: s.replace('](capture-styling.md)', '](image-production.md)')), 'missing capture styling route'),
        ('capture model route', edit('skills/mockup-image/references/model-direction.md', lambda s: s.replace('](capture-styling.md)', '](image-production.md)')), 'missing capture styling route'),
        ('missing expression contracts', lambda r: (r / 'scripts/test_expression_guidance.py').unlink(), 'package inventory mismatch'),
        ('missing attitude contracts', lambda r: (r / 'scripts/test_attitude_guidance.py').unlink(), 'package inventory mismatch'),
        ('filled outcome contract', edit(OUTCOME, lambda s: s.replace('"contract_source": null', '"contract_source": "private-task"')), 'outcome review template is populated or invalid'),
        ('filled outcome constraints', edit(OUTCOME, lambda s: s.replace('"requirements": []', '"requirements": [{"id": "private"}]')), 'outcome review template is populated or invalid'),
        ('duplicate outcome key', edit(OUTCOME, lambda s: s.replace('"status": null', '"status": "private", "status": null')), 'duplicate JSON key'),
        ('missing outcome checker', lambda r: (r / 'scripts/check_outcome.py').unlink(), 'package inventory mismatch'),
        ('missing entrypoint discovery route', edit(ENTRY, lambda s: s.replace('[Discovery](references/discovery.md)', 'Discovery')), 'missing required entrypoint discovery route'),
        ('hidden entrypoint discovery route', edit(ENTRY, lambda s: s.replace('[Discovery](references/discovery.md)', '<!-- [Discovery](references/discovery.md) -->')), 'unsupported documentation link form'),
        ('code entrypoint discovery route', edit(ENTRY, lambda s: s.replace('[Discovery](references/discovery.md)', '`[Discovery](references/discovery.md)`')), 'unsupported entrypoint code markup'),
        ('screen route', edit(ENTRY, lambda s: s.replace('](references/screen-production.md)', '](references/methods.md)')), 'missing direct screen production route'),
        ('model entry route', edit('skills/mockup-image/SKILL.md', lambda s: s.replace('](references/model-direction.md)', '](references/image-production.md)')), 'missing model direction route'),
        ('model production route', edit('skills/mockup-image/references/image-production.md', lambda s: s.replace('](model-direction.md)', '](generation-contract.md)')), 'missing model direction route'),
        ('model generation route', edit('skills/mockup-image/references/generation-contract.md', lambda s: s.replace('](model-direction.md)', '](image-production.md)')), 'missing model direction route'),
        ('missing model direction', lambda r: (r / 'skills/mockup-image/references/model-direction.md').unlink(), 'package inventory mismatch'),
        ('portfolio entry route', edit(ENTRY, lambda s: s.replace('](references/portfolio-presentation.md)', '](references/methods.md)')), 'missing portfolio presentation route'),
        ('portfolio generation route', edit('skills/mockup-image/references/generation-contract.md', lambda s: s.replace('](../../design-pack/references/portfolio-presentation.md)', '](image-production.md)')), 'missing portfolio presentation route'),
        ('portfolio brief route', edit('skills/design-pack/references/portfolio-presentation.md', lambda s: s.replace('](../templates/presentation-brief.md)', '](methods.md)')), 'missing portfolio presentation route'),
        ('color presentation route', edit('skills/design-pack/references/color.md', lambda s: s.replace('](portfolio-presentation.md)', '](elements.md)')), 'missing portfolio presentation route'),
        ('README version drift', edit('README.md', lambda s: s.replace('# Design Pack ', '# Design Pack 99.', 1)), 'README version mismatch'),
        ('shared manifest description drift', edit(MANIFESTS[1], lambda s: json.dumps(dict(json.loads(s), description='Different purpose'))), 'shared manifest metadata mismatch'),
        ('invalid manifest extension type', edit(MANIFESTS[0], lambda s: json.dumps(dict(json.loads(s), extensions=[]))), 'manifest interface mismatch'),
        ('overflow exponent manifest data', edit(MANIFESTS[0], lambda s: s[:-2] + ', "extra": 1e999}'), 'invalid plugin metadata'),
        ('nonfinite manifest data', edit(MANIFESTS[0], lambda s: s[:-2] + ', "extra": NaN}'), 'invalid plugin metadata'),
        ('manifest interface drift', edit(MANIFESTS[2], lambda s: json.dumps(dict(json.loads(s), interface={}))), 'manifest interface mismatch'),
        ('screen craft route', edit('skills/design-pack/references/screen-production.md', lambda s: s.replace('](interface-craft.md)', '](interface.md)')), 'missing craft production route'),
        ('image source route', edit('skills/mockup-image/references/image-production.md', lambda s: s.replace('](generation-contract.md)', '](../../design-pack/references/interface.md)')), 'missing craft production route'),
        ('missing presentation template', lambda r: (r / 'skills/design-pack/templates/presentation-brief.md').unlink(), 'package inventory mismatch'),
        ('source role analysis route', edit('skills/design-pack/references/reference-analysis.md', lambda s: s.replace('](reference-roles.md)', '](portfolio-presentation.md)')), 'missing source-role route'),
        ('source role generation route', edit('skills/mockup-image/references/generation-contract.md', lambda s: s.replace('](../../design-pack/references/reference-roles.md)', '](image-production.md)')), 'missing source-role route'),
        ('missing source role method', lambda r: (r / 'skills/design-pack/references/reference-roles.md').unlink(), 'package inventory mismatch'),
        ('filled screen run', edit(SCREEN_RUN, lambda s: s.replace('\"maker_id\": null', '\"maker_id\": \"private-maker\"', 1)), 'screen run template is populated or invalid'),
        ('filled image run', edit(IMAGE_RUN, lambda s: s.replace('"maker_id": null', '"maker_id": "private-maker"', 1)), 'image run template is populated or invalid'),
        ('filled image prompt', edit(IMAGE_RUN, lambda s: s.replace('"prompt_path": null', '"prompt_path": "private-prompt.txt"', 1)), 'image run template is populated or invalid'),
        ('duplicate image key', edit(IMAGE_RUN, lambda s: s.replace('"maker_id": null', '"maker_id": "private", "maker_id": null', 1)), 'duplicate JSON key'),
        ('packaged trial image', lambda r: (r / 'trial.png').write_bytes(b'private'), 'package inventory mismatch'),
        ('missing method', lambda r: (r / JUDGMENT).unlink(), 'package inventory mismatch'),
        ('unlisted data', lambda r: (r / 'private-notes.json').write_text('{}'), 'package inventory mismatch'),
        ('symlink', lambda r: ((r / JUDGMENT).unlink(), (r / JUDGMENT).symlink_to(r / 'README.md')), 'symlink in package'),
        ('invalid manifest', edit(MANIFESTS[0], lambda s: '{'), 'invalid plugin metadata'),
        ('manifest array', edit(MANIFESTS[0], lambda s: '[]'), 'invalid plugin metadata'),
        ('version drift', edit(MANIFESTS[1], lambda s: json.dumps(dict(json.loads(s), version='99.0.0'))), 'plugin version mismatch'),
        ('schema locator', edit(MANIFESTS[0], lambda s: s.replace('agent-plugins.org/schemas/1.0.0/plugin.schema.json', 'example.invalid/private-board')), 'invalid machine schema locator'),
        ('skill route', edit(MANIFESTS[1], lambda s: s.replace('./skills/', './missing/')), 'plugin skill route drift'),
        ('skill metadata', edit(ENTRY, lambda s: s.replace('name: design-pack', 'name: renamed')), 'invalid skill metadata'),
        ('empty single-quoted description', edit(ENTRY, lambda s: re.sub(r'^description:.*$', "description: ''", s, flags=re.M)), 'invalid skill metadata'),
        ('empty double-quoted description', edit(ENTRY, lambda s: re.sub(r'^description:.*$', 'description: ""', s, flags=re.M)), 'invalid skill metadata'),
        ('quoted whitespace description', edit(ENTRY, lambda s: re.sub(r'^description:.*$', 'description: "   "', s, flags=re.M)), 'invalid skill metadata'),
        ('array description', edit(ENTRY, lambda s: re.sub(r'^description:.*$', 'description: []', s, flags=re.M)), 'invalid skill metadata'),
        ('object description', edit(ENTRY, lambda s: re.sub(r'^description:.*$', 'description: {}', s, flags=re.M)), 'invalid skill metadata'),
        ('number description', edit(ENTRY, lambda s: re.sub(r'^description:.*$', 'description: 123', s, flags=re.M)), 'invalid skill metadata'),
        ('broken route', edit(ENTRY, lambda s: s+'\n[missing](missing.md)\n'), 'broken documentation route'),
        ('escaped route', edit(ENTRY, lambda s: s+'\n[escape](../../../outside.md)\n'), 'escaped documentation route'),
        ('multiline inline label', edit('README.md', lambda s: s+'\n[escape\nlabel](../../../outside.md)\n'), 'unsupported documentation link form'),
        ('multiline reference definition and use', edit('README.md', lambda s: s+'\n[escape\nlabel][route\nid]\n\n[route\nid]: ../../../outside.md\n'), 'unsupported documentation link form'),
        ('missing reference-style route', edit('README.md', lambda s: s+'\n[missing][route]\n\n[route]: missing.md\n'), 'unsupported documentation link form'),
        ('escaping reference-style route', edit('README.md', lambda s: s+'\n[escape][route]\n\n[route]: ../../../outside.md\n'), 'unsupported documentation link form'),
        ('collapsed reference route', edit('README.md', lambda s: s+'\n[route][]\n\n[route]: README.md\n'), 'unsupported documentation link form'),
        ('shortcut reference definition', edit('README.md', lambda s: s+'\n[route]\n\n[route]: missing.md\n'), 'unsupported documentation link form'),
        ('missing HTML anchor route', edit('README.md', lambda s: s+'\n<a href="missing.md">missing</a>\n'), 'unsupported documentation link form'),
        ('escaping HTML anchor route', edit('README.md', lambda s: s+'\n<A HREF="../../../outside.md">escape</A>\n'), 'unsupported documentation link form'),
        ('private source', edit('README.md', lambda s: s+'\n'+'https:'+('/'*2)+'example.invalid/private-board\n'), 'external locator in reusable content'),
        ('oversized resource', edit(JUDGMENT, lambda s: s+'x'*4096), 'compact guidance exceeds selected resource budget'),
        ('combined resource envelope', edit(JUDGMENT, lambda s: s+'x'*(6145-(ROOT / ENTRY).stat().st_size-len(s.encode('utf-8')))), 'compact guidance exceeds selected resource budget'),
        ('populated template', edit(OVERLAY, lambda s: s.replace('"items": []', '"items": [{"id": "private"}]')), 'private template is populated or invalid'),
        ('duplicate overlay key', edit(OVERLAY, lambda s: s.replace('"board_locator": null', '"board_locator": "private-source", "board_locator": null')), 'duplicate JSON key'),
        ('duplicate nested overlay key', edit(OVERLAY, lambda s: s.replace('"primary_url_or_local_path": null', '"primary_url_or_local_path": "private-source", "primary_url_or_local_path": null')), 'duplicate JSON key'),
        ('duplicate manifest key', edit(MANIFESTS[1], lambda s: s.replace('"name": "design-pack"', '"name": "unexpected", "name": "design-pack"')), 'duplicate JSON key'),
        ('lost license', edit('LICENSE', lambda s: s.replace('MIT License', '')), 'required license notice missing'),
        ('external tool hook', edit(MANIFESTS[1], lambda s: json.dumps(dict(json.loads(s), hooks={}))), 'unexpected executable extension'),
        ('non-UTF-8', lambda r: (r / 'README.md').write_bytes(bytes([255])), 'unreadable or non-UTF-8 package member'),
    ]
    with tempfile.TemporaryDirectory(prefix='design-pack-check-') as tmp:
        base = Path(tmp)
        for index, (label, mutation, expected) in enumerate(cases):
            candidate = base / str(index)
            shutil.copytree(ROOT, candidate)
            mutation(candidate)
            if expected not in check(candidate):
                raise SystemExit(f'FAIL self-test: {label}')
        description = json.loads(re.search(r'^description: (.*)$', (ROOT / ENTRY).read_text(), re.M).group(1))
        for index, quoted in enumerate((json.dumps(description), json.dumps(description+' "example"'))):
            candidate = base / f'valid-quoted-{index}'
            shutil.copytree(ROOT, candidate)
            edit(ENTRY, lambda s: re.sub(r'^description:.*$', lambda _: 'description: '+quoted, s, flags=re.M))(candidate)
            if check(candidate):
                raise SystemExit('FAIL self-test: valid quoted description')
        candidate = base / 'valid-inline-route'
        shutil.copytree(ROOT, candidate)
        edit('README.md', lambda s: s+'\n[Supported local route](skills/design-pack/SKILL.md)\n')(candidate)
        if check(candidate):
            raise SystemExit('FAIL self-test: valid supported inline route')
    return len(cases)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--self-test', action='store_true')
    args = parser.parse_args()
    errors = check(ROOT)
    if errors:
        raise SystemExit('\n'.join(errors))
    sizes = [(ROOT / rel).stat().st_size for rel in (ENTRY, JUDGMENT)]
    print(f'PASS package: {len(EXPECTED)} files; selected guidance {sum(sizes)}/6144 bytes')
    if args.self_test:
        print(f'PASS structural refusal cases: {self_test()}')


if __name__ == '__main__':
    main()
