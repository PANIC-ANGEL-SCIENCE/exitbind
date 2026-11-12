#!/usr/bin/env python3
"""Optional project evidence check; not visual-quality or actual-view verification.

Run explicitly with --project-root PROJECT --evidence relative/run.json.
Evidence and immutable raster snapshots belong in the private project, not this
pack. Supported signatures: PNG, JPEG, GIF, WebP. Signature checks do not decode
images or prove tool execution, provenance, timing, or that a person/model saw
anything. Sequence and tool IDs are self-reported. No networking or host hook.

Mode boundary: ordinary permits multiple complete-screen render events;
frozen-trial permits exactly one. Individual image assets are not render events.
Frozen quality failures and remaining findings are allowed.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path, PurePosixPath
import re
import sys


class EvidenceError(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise EvidenceError(message)


def nonempty(value):
    return isinstance(value, str) and bool(value.strip())


def integer(value):
    return type(value) is int and value > 0


def local_file(root, value):
    require(nonempty(value), 'missing local file path')
    path = PurePosixPath(value)
    require(not path.is_absolute() and '..' not in path.parts and
            '\\' not in value and ':' not in value, 'unsafe local file path')
    current = root
    for part in path.parts:
        current = current / part
        require(not current.is_symlink(), 'symlinks are not allowed')
    require(current.is_file(), 'local file missing or not a regular file')
    require(current.resolve().is_relative_to(root), 'file escapes project root')
    return current


def snapshot(root, event, path_key):
    path = local_file(root, event.get(path_key))
    digest = event.get('sha256')
    require(isinstance(digest, str) and re.fullmatch('[0-9a-f]{64}', digest),
            'invalid sha256')
    data = path.read_bytes()
    require(hashlib.sha256(data).hexdigest() == digest, 'snapshot hash mismatch')
    raster = (data.startswith(b'\x89PNG\r\n\x1a\n') or data.startswith(b'\xff\xd8\xff') or
              data.startswith((b'GIF87a', b'GIF89a')) or
              (data.startswith(b'RIFF') and data[8:12] == b'WEBP'))
    require(raster, 'snapshot lacks a supported raster signature')


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, 'duplicate JSON key')
        result[key] = value
    return result


def load_json(text):
    def invalid_constant(value):
        raise EvidenceError('non-finite JSON number')
    def finite_float(value):
        result = float(value)
        if not math.isfinite(result):
            raise EvidenceError('non-finite JSON number')
        return result
    return json.loads(text, object_pairs_hook=unique_object,
                      parse_constant=invalid_constant, parse_float=finite_float)


def validate(root, data):
    require(isinstance(data, dict), 'run must be an object')
    require(type(data.get('schema_version')) is int and data['schema_version'] == 1,
            'unsupported schema_version')
    require(data.get('mode') in ('ordinary', 'frozen-trial'), 'invalid mode')
    maker = data.get('maker_id')
    require(nonempty(maker), 'missing maker_id')
    require(nonempty(data.get('pack_version')), 'missing pack_version')
    events = data.get('events')
    require(isinstance(events, list) and events, 'events must be a nonempty list')
    previous, refs, renders, reviewed = 0, {}, {}, set()
    direction_seq = None
    for event in events:
        require(isinstance(event, dict), 'event must be an object')
        seq = event.get('seq')
        require(integer(seq) and seq > previous, 'seq must be unique positive ascending integers')
        previous = seq
        kind = event.get('kind')
        if kind == 'reference_view':
            require(event.get('maker_id') == maker, 'reference must be viewed by run maker')
            require(event.get('viewed') is True, 'reference viewed must be true')
            require(event.get('evidence_type') == 'image-view', 'reference must be image-view evidence')
            for field in ('input_id', 'source_locator', 'source_state', 'observation', 'view_tool_result_id'):
                require(nonempty(event.get(field)), 'missing reference ' + field)
            snapshot(root, event, 'capture_path')
            refs[seq] = event
        elif kind == 'direction':
            require(event.get('maker_id') == maker, 'direction maker mismatch')
            decisions = event.get('decisions')
            require(isinstance(decisions, list) and decisions, 'missing direction decisions')
            for decision in decisions:
                require(isinstance(decision, dict) and nonempty(decision.get('decision')),
                        'missing concrete direction decision')
                sources = decision.get('source_refs')
                require(isinstance(sources, list) and sources, 'decision lacks source refs')
                require(all(integer(s) and s in refs for s in sources),
                        'decision must cite earlier maker reference_view seq')
            direction_seq = seq
        elif kind == 'render':
            require(data['mode'] != 'frozen-trial' or not renders,
                    'frozen-trial permits only one complete-screen render')
            require(direction_seq is not None, 'render needs earlier direction')
            require(event.get('direction_seq') == direction_seq and integer(event.get('direction_seq')),
                    'render must cite latest direction')
            snapshot(root, event, 'artifact_path')
            renders[seq] = event
        elif kind == 'review':
            render_seq = event.get('render_seq')
            require(integer(render_seq) and render_seq in renders, 'review needs an earlier render')
            require(render_seq == max(renders), 'review is stale for latest render')
            require(renders[render_seq]['direction_seq'] == direction_seq,
                    'review is stale for latest direction')
            require(nonempty(event.get('reviewer_id')), 'missing reviewer_id')
            require(event.get('viewed') is True and event.get('evidence_type') == 'image-view',
                    'review must record image-view evidence')
            require(nonempty(event.get('view_tool_result_id')), 'missing review view tool result ID')
            require(event.get('sha256') == renders[render_seq]['sha256'], 'review hash mismatch')
            findings = event.get('findings')
            require(isinstance(findings, list) and findings, 'review needs concrete findings')
            for finding in findings:
                require(isinstance(finding, dict), 'finding must be an object')
                for field in ('location', 'observation', 'impact', 'action', 'status'):
                    require(nonempty(finding.get(field)), 'missing finding ' + field)
            reviewed.add(render_seq)
        else:
            raise EvidenceError('unsupported event kind')
    require(renders and max(renders) in reviewed, 'latest render needs matching review')
    require(renders[max(renders)]['direction_seq'] == direction_seq,
            'latest direction needs rendered and reviewed evidence')


def check(project_root, evidence):
    try:
        raw_root = Path(project_root).absolute()
        require(not any(p.is_symlink() for p in (raw_root, *raw_root.parents)),
                'project root must not contain symlinks')
        root = raw_root.resolve()
        require(root.is_dir(), 'project root must be a directory')
        data = load_json(local_file(root, evidence).read_text(encoding='utf-8'))
        validate(root, data)
        return []
    except (EvidenceError, OSError, ValueError, TypeError, RecursionError) as exc:
        return [str(exc)]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--project-root', required=True)
    parser.add_argument('--evidence', required=True)
    args = parser.parse_args()
    errors = check(args.project_root, args.evidence)
    if errors:
        print('FAIL evidence: ' + '; '.join(errors), file=sys.stderr)
        return 1
    print('PASS evidence completeness, local hashes and declared order only; not quality or actual viewing')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
