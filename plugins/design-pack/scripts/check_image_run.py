#!/usr/bin/env python3
"""Optional private-project declared image-call evidence check, not quality proof.

Checks local hashes, PNG/JPEG header dimensions, explicit argument bindings and
self-reported order. Cannot authenticate tool execution, actual viewing, model
conditioning, freeze timing, or full image decoding. Input roles and opaque-ID
mappings are declared attestations, not provider schema validation. Keep evidence
outside pack.
exact_target_frame means equal aspect ratio, not equal CSS and raster pixels.
"""
import argparse
import hashlib
from pathlib import Path
import struct
import sys
import zlib

sys.dont_write_bytecode = True
from check_run import EvidenceError, require, nonempty, integer, local_file, load_json


def file_hash(root, path, digest):
    file = local_file(root, path)
    data = file.read_bytes()
    require(isinstance(digest, str) and hashlib.sha256(data).hexdigest() == digest,
            'local file hash mismatch')
    return data


def dimensions(data):
    if data.startswith(b'\x89PNG\r\n\x1a\n'):
        require(len(data) >= 33 and data[8:16] == b'\x00\x00\x00\rIHDR', 'invalid PNG header')
        require(zlib.crc32(data[12:29]) == int.from_bytes(data[29:33], 'big'), 'invalid PNG header CRC')
        width, height = struct.unpack('>II', data[16:24])
    elif data.startswith(b'\xff\xd8'):
        position = 2
        while position < len(data):
            require(data[position] == 255, 'invalid JPEG marker')
            while position < len(data) and data[position] == 255:
                position += 1
            require(position < len(data), 'truncated JPEG marker')
            marker = data[position]
            position += 1
            require(marker not in (0, 0xDA, 0xD9), 'JPEG lacks dimension header')
            if marker == 1 or 0xD0 <= marker <= 0xD7:
                continue
            require(position + 2 <= len(data), 'truncated JPEG length')
            length = int.from_bytes(data[position:position + 2], 'big')
            require(length >= 2 and position + length <= len(data), 'truncated JPEG segment')
            if marker in (0xC0, 0xC1, 0xC2, 0xC3, 0xC5, 0xC6, 0xC7, 0xC9, 0xCA, 0xCB, 0xCD, 0xCE, 0xCF):
                require(length >= 8, 'invalid JPEG frame')
                height, width = struct.unpack('>HH', data[position + 3:position + 7])
                break
            position += length
        else:
            raise EvidenceError('JPEG lacks dimension header')
    else:
        raise EvidenceError('only PNG/JPEG dimension headers supported')
    require(integer(width) and integer(height), 'invalid raster dimensions')
    return width, height


def pointer(value, path):
    require(nonempty(path) and path.startswith('/'), 'invalid argument pointer')
    tokens = path[1:].split('/')
    for segment in tokens:
        require('~' not in segment.replace('~0', '').replace('~1', ''), 'invalid pointer escape')
        segment = segment.replace('~1', '/').replace('~0', '~')
        if isinstance(value, dict):
            require(segment in value, 'argument pointer missing')
            value = value[segment]
        elif isinstance(value, list):
            require(segment.isdigit() and str(int(segment)) == segment and int(segment) < len(value), 'invalid array pointer')
            value = value[int(segment)]
        else:
            raise EvidenceError('argument pointer traverses scalar text')
    return value


def size(value):
    require(isinstance(value, dict) and integer(value.get('width')) and integer(value.get('height')), 'invalid dimensions')
    return value['width'], value['height']


def validate(root, data):
    require(isinstance(data, dict), 'run must be object')
    require(type(data.get('schema_version')) is int and data['schema_version'] == 1, 'unsupported schema_version')
    require(data.get('mode') in ('ordinary', 'frozen-trial'), 'invalid mode')
    for key in ('maker_id', 'pack_version'):
        require(nonempty(data.get(key)), 'missing ' + key)
    target = size(data.get('target_viewport'))
    events = data.get('events')
    require(isinstance(events, list) and events, 'missing events')
    refs, runs, reviewed = {}, {}, set()
    previous, pending_freeze, frozen = 0, None, False
    for event in events:
        require(isinstance(event, dict), 'event must be object')
        seq, kind = event.get('seq'), event.get('kind')
        require(integer(seq) and seq > previous, 'seq must ascend')
        previous = seq
        require(pending_freeze is None or kind == 'freeze', 'first output must freeze immediately')
        if kind == 'reference_view':
            require(event.get('maker_id') == data['maker_id'], 'maker mismatch')
            for key in ('view_tool_result_id', 'observation'):
                require(nonempty(event.get(key)), 'missing reference ' + key)
            dimensions(file_hash(root, event.get('path'), event.get('sha256')))
            refs[seq] = event
        elif kind == 'generation':
            require(data['mode'] != 'frozen-trial' or not runs, 'frozen-trial permits one generation')
            for key in ('tool_name', 'tool_result_id'):
                require(nonempty(event.get(key)), 'missing ' + key)
            args = event.get('arguments')
            require(isinstance(args, dict), 'arguments must be object')
            prompt = file_hash(root, event.get('prompt_path'), event.get('prompt_sha256')).decode('utf-8')
            require(nonempty(prompt) and pointer(args, event.get('prompt_argument_pointer')) == prompt, 'prompt argument mismatch')
            inputs = event.get('image_inputs')
            require(isinstance(inputs, list), 'image_inputs must be list')
            require(event.get('conditioning') in ('reference-images', 'text-only'), 'invalid conditioning')
            require(bool(inputs) == (event['conditioning'] == 'reference-images'), 'conditioning lacks image inputs')
            # A viewed research image is not automatically a required provider input.
            # Older records retain their text-only omission rule; new records can
            # explicitly distinguish required conditioning from optional study.
            required_refs = event.get('required_reference_seqs')
            if 'required_reference_seqs' in event:
                require(isinstance(required_refs, list) and
                        all(integer(seq) and seq in refs for seq in required_refs),
                        'required inputs need earlier viewed references')
                require(len(set(required_refs)) == len(required_refs),
                        'duplicate required reference')
            elif not inputs and refs:
                require(nonempty(event.get('text_only_approval')),
                        'omitted references need approval evidence')
            seen, mapped_refs = set(), set()
            for item in inputs:
                require(isinstance(item, dict), 'image input must be object')
                arg_pointer = item.get('argument_pointer')
                require(arg_pointer != event['prompt_argument_pointer'] and not any(part.lower() in ('prompt', 'text', 'instructions') for part in str(arg_pointer).split('/')), 'prompt is not image input')
                supplied = pointer(args, arg_pointer)
                require(type(supplied) is type(item.get('supplied_value')) and supplied == item['supplied_value'], 'supplied value mismatch')
                reference_seqs = item.get('reference_seqs')
                require(isinstance(reference_seqs, list) and reference_seqs and all(integer(s) and s in refs for s in reference_seqs), 'input needs earlier viewed references')
                require(len(set(reference_seqs)) == len(reference_seqs), 'duplicate mapped reference')
                mapped_refs.update(reference_seqs)
                require(arg_pointer not in seen, 'duplicate image input pointer')
                seen.add(arg_pointer)
                require(nonempty(item.get('mapping_evidence')), 'missing mapping evidence')
                binding = item.get('input_binding')
                if binding == 'local-path':
                    require(len(reference_seqs) == 1 and nonempty(supplied), 'invalid path binding')
                    ref = refs[reference_seqs[0]]
                    expected = local_file(root, ref['path'])
                    require(supplied in (ref['path'], str(expected)), 'path does not bind viewed image')
                elif binding == 'conversation-count':
                    require(integer(supplied) and supplied == len(reference_seqs), 'ambiguous conversation image count')
                    require(nonempty(item.get('mapping_evidence')), 'missing conversation mapping')
                elif binding == 'opaque-image-id':
                    require(nonempty(supplied) and len(reference_seqs) == 1, 'invalid opaque image binding')
                else:
                    raise EvidenceError('unsupported input_binding')
            if required_refs is not None:
                omitted = set(required_refs) - mapped_refs
                require(not omitted or nonempty(event.get('reference_omission_approval')),
                        'required image input omitted without approval evidence')
            output = event.get('output')
            require(isinstance(output, dict), 'missing output')
            actual = dimensions(file_hash(root, output.get('path'), output.get('sha256')))
            require(size(output) == actual, 'encoded output dimensions mismatch')
            require(type(event.get('exact_target_frame')) is bool, 'exact_target_frame must be boolean')
            require(not event['exact_target_frame'] or actual[0] * target[1] == actual[1] * target[0], 'false exact target aspect ratio claim')
            if not runs and data['mode'] == 'frozen-trial':
                pending_freeze = seq
            runs[seq] = event
        elif kind == 'freeze':
            # Ordinary receipts may retain the legacy preservation marker, but
            # only explicit trials require a freeze before first-output review.
            require(runs and not frozen and integer(event.get('generation_seq')) and
                    event.get('generation_seq') == min(runs),
                    'freeze must match first generation once')
            require(event.get('sha256') == runs[min(runs)]['output']['sha256'], 'freeze hash mismatch')
            pending_freeze, frozen = None, True
        elif kind == 'review':
            run = event.get('generation_seq')
            require(integer(run) and run in runs and run == max(runs), 'review must match latest generation')
            require(event.get('sha256') == runs[run]['output']['sha256'], 'review hash mismatch')
            require(nonempty(event.get('view_tool_result_id')), 'missing review image view ID')
            findings = event.get('findings')
            require(isinstance(findings, list) and findings, 'missing findings')
            for finding in findings:
                require(isinstance(finding, dict) and all(nonempty(finding.get(k)) for k in ('location', 'observation', 'impact', 'action', 'status')), 'incomplete finding')
            reviewed.add(run)
        else:
            raise EvidenceError('unsupported event kind')
    require(runs and max(runs) in reviewed, 'latest output needs review')
    require(data['mode'] != 'frozen-trial' or frozen, 'frozen-trial needs first-output freeze')


def check(project_root, evidence):
    try:
        raw = Path(project_root).absolute()
        require(not any(p.is_symlink() for p in (raw, *raw.parents)), 'project root contains symlink')
        root = raw.resolve()
        require(root.is_dir(), 'project root missing')
        validate(root, load_json(local_file(root, evidence).read_text(encoding='utf-8')))
        return []
    except (EvidenceError, OSError, ValueError, TypeError, RecursionError, struct.error) as exc:
        return [str(exc)]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--project-root', required=True)
    parser.add_argument('--evidence', required=True)
    args = parser.parse_args()
    errors = check(args.project_root, args.evidence)
    if errors:
        print('FAIL image evidence: ' + '; '.join(errors), file=sys.stderr)
        return 1
    print('PASS declared image-call evidence, hashes, header dimensions and order only; not execution or quality')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
