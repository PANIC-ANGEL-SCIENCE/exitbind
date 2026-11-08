#!/usr/bin/env python3
"""Optional task-scoped readiness gate, not a taste or execution verifier.

Uses local snapshots and declared visual findings. A passed gate means the
record is coherent with its own explicit contract, not that the contract is
complete, the observations are true, or the user accepted the artifact.
No renderer, provider, installation hook or network access is included.
"""
import argparse
import hashlib
import math
from pathlib import Path
import sys

from check_run import EvidenceError, integer, load_json, local_file, nonempty, require, snapshot


def rows(value, label, allow_empty=False):
    require(isinstance(value, list) and (allow_empty or value), label + ' must be a list')
    require(all(isinstance(item, dict) for item in value), label + ' needs objects')
    return value


def indexed(value, label):
    result = {}
    for item in rows(value, label):
        key = item.get('id')
        require(nonempty(key) and key not in result, label + ' ids must be unique strings')
        result[key] = item
    return result


def strings(value, label, allow_empty=False):
    require(isinstance(value, list) and (allow_empty or value) and
            all(nonempty(item) for item in value), label + ' must contain strings')
    require(len(value) == len(set(value)), label + ' must be unique')
    return value


def number(value):
    try:
        return type(value) in (int, float) and math.isfinite(value) and value > 0
    except OverflowError:
        return False


def shape_check(item, shape):
    for name in ('contour_observation', 'affordance_observation'):
        require(nonempty(item.get(name)), 'shape check lacks ' + name)
    if item['result'] != 'pass' or shape not in ('circle', 'capsule'):
        return
    geometry = item.get('geometry')
    require(isinstance(geometry, dict), 'passing exact shape needs measured geometry')
    require(nonempty(geometry.get('unit')), 'shape geometry lacks unit')
    width, height = geometry.get('width'), geometry.get('height')
    require(number(width) and number(height), 'shape dimensions must be finite positive numbers')
    radii = geometry.get('effective_corner_radii')
    require(isinstance(radii, list) and len(radii) == 4 and all(number(v) for v in radii),
            'shape needs four effective circular corner radii')
    # Tiny tolerance allows reported floating-point rounding, not softened rectangles.
    close = lambda a, b: math.isclose(a, b, rel_tol=1e-6, abs_tol=1e-6)
    require(shape != 'circle' or close(width, height), 'circle needs equal visible axes')
    require(all(close(radius, min(width, height) / 2) for radius in radii),
            'exact round contour conflicts with measured radii')


def validate(root, data):
    require(isinstance(data, dict), 'outcome review must be an object')
    require(type(data.get('schema_version')) is int and data['schema_version'] == 1,
            'unsupported schema_version')
    require(data.get('status') in ('rework', 'ready-for-user-review'), 'invalid status')
    require(data.get('mode') in ('ordinary', 'frozen-trial'), 'invalid mode')
    require(nonempty(data.get('contract_source')), 'missing task-scoped contract source')
    artifacts = indexed(data.get('artifacts'), 'artifacts')
    requirements = indexed(data.get('requirements'), 'requirements')
    expected = set()
    for req_id, req in requirements.items():
        require(req.get('category') in ('shape', 'relational-identity', 'sequence', 'continuity', 'language', 'other'),
                'unknown requirement category')
        require(nonempty(req.get('instruction')), 'requirement needs scoped instruction')
        if req['category'] == 'shape':
            require(nonempty(req.get('shape')), 'shape requirement lacks requested silhouette')
        for target in rows(req.get('targets'), 'requirement targets'):
            aid, location = target.get('artifact_id'), target.get('location')
            require(nonempty(aid) and aid in artifacts and nonempty(location), 'invalid requirement target')
            key = (req_id, aid, location)
            require(key not in expected, 'duplicate requirement target')
            expected.add(key)
    for aid, artifact in artifacts.items():
        require(integer(artifact.get('produced_seq')), 'artifact needs positive produced_seq')
        require(artifact.get('kind') in ('raster', 'pdf'), 'unsupported artifact kind')
        if artifact['kind'] == 'raster':
            snapshot(root, artifact, 'path')
        else:
            raw = local_file(root, artifact.get('path')).read_bytes()
            require(raw.startswith(b'%PDF-'), 'PDF signature missing')
            require(hashlib.sha256(raw).hexdigest() == artifact.get('sha256'), 'PDF hash mismatch')
    view_map = {}
    for view in rows(data.get('views'), 'views', allow_empty=True):
        vid, aid = view.get('id'), view.get('artifact_id')
        require(nonempty(vid) and vid not in view_map, 'view ids must be unique strings')
        require(nonempty(aid) and aid in artifacts and artifacts[aid]['kind'] == 'raster',
                'visual view needs a declared raster artifact')
        require(integer(view.get('seq')) and view['seq'] > artifacts[aid]['produced_seq'],
                'view must follow its artifact')
        require(view.get('sha256') == artifacts[aid]['sha256'], 'view is stale for artifact')
        require(view.get('evidence_type') == 'image-view' and nonempty(view.get('view_tool_result_id')),
                'view needs declared image-view evidence')
        require(view.get('scale') in ('overview', 'use', 'detail'), 'view needs a relevant scale')
        view_map[vid] = view
    checked = set()
    for item in rows(data.get('checks'), 'checks'):
        key = (item.get('requirement_id'), item.get('artifact_id'), item.get('location'))
        require(all(nonempty(k) for k in key) and key in expected, 'check has undeclared target')
        require(key not in checked, 'duplicate target check')
        checked.add(key)
        require(item.get('result') in ('pass', 'fail', 'not-tested'), 'invalid check result')
        require(nonempty(item.get('observation')), 'check needs concrete observation or gap')
        vids = strings(item.get('view_ids'), 'check view_ids', allow_empty=item['result'] == 'not-tested')
        require(all(v in view_map and view_map[v]['artifact_id'] == key[1] for v in vids),
                'check cites missing or wrong artifact view')
        req = requirements[key[0]]
        if item['result'] != 'not-tested' and req['category'] == 'shape':
            shape_check(item, req['shape'])
        if item['result'] != 'not-tested' and req['category'] == 'relational-identity':
            for field in ('observed_relationship', 'interpretation', 'alternative_reading'):
                require(nonempty(item.get(field)), 'relationship check lacks ' + field)
        if data['status'] == 'ready-for-user-review':
            require(item['result'] == 'pass', 'unmet scoped requirement blocks readiness')
    require(checked == expected, 'checks must cover every scoped target')
    sequence = data.get('sequence')
    has_sequence = any(req['category'] == 'sequence' for req in requirements.values())
    require(not has_sequence or isinstance(sequence, dict), 'sequence requirement lacks sequence contract')
    if sequence is not None:
        require(isinstance(sequence, dict), 'invalid sequence contract')
        pages = strings(sequence.get('ordered_pages'), 'ordered_pages')
        require(len(pages) >= 2 and all(p in artifacts and artifacts[p]['kind'] == 'raster' for p in pages),
                'sequence needs at least two distinct raster pages')
        overview = sequence.get('overview_id')
        require(nonempty(overview) and overview in artifacts and overview not in pages and
                artifacts[overview]['kind'] == 'raster', 'sequence needs its own raster overview')
        order = sequence.get('overview_members')
        expected_order = [{'artifact_id': p, 'sha256': artifacts[p]['sha256']} for p in pages]
        require(order == expected_order, 'overview must bind current pages in order')
        require(artifacts[overview]['produced_seq'] > max(artifacts[p]['produced_seq'] for p in pages),
                'overview must follow final pages')
        require(type(sequence.get('sequence_first')) is bool, 'sequence_first must be explicit')
        if sequence['sequence_first']:
            plan = sequence.get('plan')
            require(isinstance(plan, dict) and integer(plan.get('seq')) and
                    plan['seq'] < min(artifacts[p]['produced_seq'] for p in pages),
                    'sequence plan must precede final pages')
            require(plan.get('ordered_pages') == pages, 'plan and final page order differ')
            require(nonempty(plan.get('argument')), 'sequence plan lacks argument')
            roles = rows(plan.get('contributions'), 'page contributions')
            require([row.get('artifact_id') for row in roles] == pages and
                    all(nonempty(row.get('contribution')) for row in roles), 'plan needs ordered page contributions')
            require(nonempty(plan.get('transition')), 'sequence plan lacks transition')
            if data['mode'] == 'ordinary':
                study, study_view = plan.get('study_artifact_id'), plan.get('study_view_id')
                require(nonempty(study) and study in artifacts and study not in pages and study != overview and
                        artifacts[study]['kind'] == 'raster', 'sequence plan needs a separate visual study')
                require(nonempty(study_view) and study_view in view_map and
                        view_map[study_view]['artifact_id'] == study, 'sequence plan needs its visual study review')
                require(view_map[study_view]['seq'] < min(artifacts[p]['produced_seq'] for p in pages),
                        'sequence study must be viewed before final pages')
        pdf_id = sequence.get('pdf_id')
        if sequence.get('format') == 'pdf':
            require(nonempty(pdf_id) and pdf_id in artifacts and artifacts[pdf_id]['kind'] == 'pdf',
                    'PDF deliverable requires actual PDF artifact')
            require(not sequence['sequence_first'] or sequence['plan']['seq'] < artifacts[pdf_id]['produced_seq'],
                    'sequence plan must precede final PDF')
            require(not sequence['sequence_first'] or data['mode'] == 'frozen-trial' or view_map[sequence['plan']['study_view_id']]['seq'] < artifacts[pdf_id]['produced_seq'],
                    'sequence study must be viewed before final PDF')
            require(sequence.get('pdf_page_count') == len(pages) and type(sequence.get('pdf_page_count')) is int,
                    'PDF page count must match ordered pages')
            for index, page in enumerate(pages, 1):
                parent = artifacts[page].get('derived_from')
                require(isinstance(parent, dict) and integer(parent.get('page_number')),
                        'PDF page number must be a positive integer')
                require(artifacts[page].get('derived_from') == {'artifact_id': pdf_id,
                        'sha256': artifacts[pdf_id]['sha256'], 'page_number': index},
                        'page render must bind final PDF and page number')
                require(artifacts[page]['produced_seq'] > artifacts[pdf_id]['produced_seq'],
                        'PDF page render must follow final PDF')
        else:
            require(sequence.get('format') == 'raster-series' and pdf_id is None,
                    'use pdf or raster-series format explicitly')
        review = sequence.get('review')
        require(isinstance(review, dict), 'sequence needs a review or explicit untested gap')
        require(review.get('result') in ('pass', 'fail', 'not-tested'), 'invalid sequence review result')
        for field in ('hierarchy', 'transition', 'continuity', 'reading_size'):
            require(nonempty(review.get(field)), 'sequence review lacks ' + field)
        vids = strings(review.get('view_ids'), 'sequence review views', allow_empty=review['result'] == 'not-tested')
        require(all(v in view_map for v in vids), 'sequence review cites unknown view')
        if review['result'] != 'not-tested':
            require(all(any(view_map[v]['artifact_id'] == page and view_map[v]['scale'] == 'use'
                            for v in vids) for page in pages), 'sequence needs every page at use scale')
            require(any(view_map[v]['artifact_id'] == overview and view_map[v]['scale'] == 'overview'
                        for v in vids), 'sequence needs full overview review')
        require(data['status'] != 'ready-for-user-review' or review['result'] == 'pass',
                'unmet sequence outcome blocks readiness')
    # A frozen record can document success or failure, but never grants revision authority.


def check(project_root, evidence):
    try:
        root = Path(project_root).absolute()
        require(not any(p.is_symlink() for p in (root, *root.parents)), 'project root must not contain symlinks')
        root = root.resolve()
        require(root.is_dir(), 'project root must be a directory')
        validate(root, load_json(local_file(root, evidence).read_text(encoding='utf-8')))
        return []
    except (EvidenceError, OSError, ValueError, TypeError, RecursionError, KeyError) as exc:
        return [str(exc)]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--project-root', required=True)
    parser.add_argument('--evidence', required=True)
    args = parser.parse_args()
    errors = check(args.project_root, args.evidence)
    if errors:
        print('FAIL scoped outcome record: ' + '; '.join(errors), file=sys.stderr)
        return 1
    print('PASS declared scoped outcome consistency only; not visual quality, execution or owner acceptance')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
