#!/usr/bin/env python3
"""Temporary synthetic fixtures only; never package private trial evidence."""
import base64
import copy
import hashlib
import json
from pathlib import Path
import struct
import sys
import tempfile
import unittest
import zlib

sys.dont_write_bytecode = True
from check_image_run import check, dimensions, EvidenceError

PNG = base64.b64decode('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII=')
HASH = hashlib.sha256(PNG).hexdigest()
PROMPT = 'Draw a calm interface using the supplied reference.'


def fixture():
    return {'schema_version': 1, 'mode': 'ordinary', 'maker_id': 'maker', 'pack_version': '0.7.1',
            'target_viewport': {'width': 390, 'height': 390}, 'events': [
        {'seq': 1, 'kind': 'reference_view', 'maker_id': 'maker', 'path': 'ref.png', 'sha256': HASH,
         'view_tool_result_id': 'view-1', 'observation': 'Large title.'},
        {'seq': 2, 'kind': 'generation', 'tool_name': 'host-image-tool', 'tool_result_id': 'gen-1',
         'arguments': {'prompt': PROMPT, 'referenced_image_paths': ['ref.png']},
         'prompt_path': 'prompt.txt', 'prompt_sha256': hashlib.sha256(PROMPT.encode()).hexdigest(),
         'prompt_argument_pointer': '/prompt', 'conditioning': 'reference-images',
         'image_inputs': [{'argument_pointer': '/referenced_image_paths/0', 'supplied_value': 'ref.png',
                           'reference_seqs': [1], 'input_binding': 'local-path', 'mapping_evidence': 'call-1'}],
         'output': {'path': 'first.png', 'sha256': HASH, 'width': 1, 'height': 1}, 'exact_target_frame': True},
        {'seq': 3, 'kind': 'freeze', 'generation_seq': 2, 'sha256': HASH},
        {'seq': 4, 'kind': 'review', 'generation_seq': 2, 'sha256': HASH, 'view_tool_result_id': 'view-2',
         'findings': [{'location': 'title', 'observation': 'Too small.', 'impact': 'Poor hierarchy.',
                       'action': 'Disclose remaining flaw.', 'status': 'remaining'}]}]}


class ImageEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='image-run-test-')
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name).resolve()
        for name in ('ref.png', 'first.png'):
            (self.root / name).write_bytes(PNG)
        (self.root / 'prompt.txt').write_text(PROMPT)
        self.data = fixture()
        self.gen = self.data['events'][1]
        self.binding = self.gen['image_inputs'][0]

    def result(self):
        (self.root / 'run.json').write_text(json.dumps(self.data))
        return check(self.root, 'run.json')

    def test_valid_remaining_findings(self):
        self.assertEqual(self.result(), [])

    def test_absolute_path_binding(self):
        value = str(self.root / 'ref.png')
        self.gen['arguments']['referenced_image_paths'][0] = value
        self.binding['supplied_value'] = value
        self.assertEqual(self.result(), [])

    def test_approved_text_only(self):
        self.gen.update(image_inputs=[], conditioning='text-only', text_only_approval='user-approved omission')
        self.gen['arguments'].pop('referenced_image_paths')
        self.assertEqual(self.result(), [])

    def test_original_text_only(self):
        self.data['events'].pop(0)
        self.gen.update(image_inputs=[], conditioning='text-only')
        self.gen['arguments'].pop('referenced_image_paths')
        self.assertEqual(self.result(), [])

    def test_conversation_count(self):
        self.gen['arguments'] = {'prompt': PROMPT, 'num_last_images_to_include': 1}
        self.binding.update(argument_pointer='/num_last_images_to_include', supplied_value=1, input_binding='conversation-count')
        self.assertEqual(self.result(), [])
        self.gen['arguments']['num_last_images_to_include'] = 2
        self.binding['supplied_value'] = 2
        self.assertTrue(self.result())

    def test_opaque_mapping(self):
        self.gen['arguments'] = {'prompt': PROMPT, 'image_id': 'image-123'}
        self.binding.update(argument_pointer='/image_id', supplied_value='image-123', input_binding='opaque-image-id')
        self.assertEqual(self.result(), [])

    def test_honest_size_mismatch(self):
        self.data['target_viewport']['height'] = 844
        self.gen['exact_target_frame'] = False
        self.assertEqual(self.result(), [])
        self.gen['exact_target_frame'] = True
        self.assertTrue(self.result())

    def test_failures(self):
        mutations = [
            lambda: self.gen.update(image_inputs=[]),
            lambda: self.gen.update(image_inputs=[], conditioning='text-only'),
            lambda: self.gen['arguments'].pop('referenced_image_paths'),
            lambda: self.binding.update(supplied_value='other.png'),
            lambda: self.binding.update(reference_seqs=[99]),
            lambda: self.binding.update(reference_seqs=[1, 1]),
            lambda: self.binding.update(mapping_evidence=''),
            lambda: self.binding.update(argument_pointer='/prompt', supplied_value=PROMPT, input_binding='opaque-image-id'),
            lambda: self.binding.update(argument_pointer='/prompt/0'),
            lambda: self.gen['output'].update(width=True),
            lambda: self.gen['output'].update(width=2),
            lambda: self.gen['output'].update(path='../first.png'),
            lambda: self.gen['output'].update(sha256='0' * 64),
            lambda: self.gen.update(prompt_sha256='0' * 64),
            lambda: self.gen['arguments'].update(prompt='Other prompt'),
            lambda: self.data['events'][0].update(maker_id='someone-else'),
            lambda: self.data['events'][2].update(sha256='0' * 64),
            lambda: self.data['events'][3].update(sha256='0' * 64),
            lambda: self.data['events'][3].update(findings=[]),
            lambda: self.data['events'][3].update(seq=2),
        ]
        for index, mutate in enumerate(mutations):
            with self.subTest(index=index):
                self.data = fixture()
                self.gen = self.data['events'][1]
                self.binding = self.gen['image_inputs'][0]
                mutate()
                self.assertTrue(self.result())

    def test_iteration_is_not_frozen_trial(self):
        second = copy.deepcopy(self.gen)
        second['seq'] = 5
        review = copy.deepcopy(self.data['events'][-1])
        review.update(seq=6, generation_seq=5)
        self.data['events'].extend([second, review])
        self.assertEqual(self.result(), [])
        self.data['mode'] = 'frozen-trial'
        self.assertTrue(self.result())

    def test_ordinary_without_freeze(self):
        self.data['events'].pop(2)
        self.assertEqual(self.result(), [])

    def test_ordinary_iteration_without_freeze(self):
        self.data['events'].pop(2)
        second = copy.deepcopy(self.gen)
        second['seq'] = 5
        review = copy.deepcopy(self.data['events'][-1])
        review.update(seq=6, generation_seq=5)
        self.data['events'].extend([second, review])
        self.assertEqual(self.result(), [])

    def test_trial_without_freeze(self):
        self.data['mode'] = 'frozen-trial'
        self.data['events'].pop(2)
        self.assertTrue(self.result())

    def test_optional_freeze_still_binds_original(self):
        self.data['events'][2]['generation_seq'] = 99
        self.assertTrue(self.result())

    def test_duplicate_freeze_rejected(self):
        duplicate = copy.deepcopy(self.data['events'][2])
        duplicate['seq'] = 5
        self.data['events'].append(duplicate)
        self.assertTrue(self.result())

    def test_scaled_raster_exact_frame(self):
        # Header dimensions suffice for this checker, not a full PNG decode.
        header = bytearray(PNG)
        header[16:24] = struct.pack('>II', 780, 1688)
        header[29:33] = struct.pack('>I', zlib.crc32(header[12:29]))
        raw = bytes(header)
        (self.root / 'first.png').write_bytes(raw)
        digest = hashlib.sha256(raw).hexdigest()
        self.gen['output'].update(width=780, height=1688, sha256=digest)
        self.data['target_viewport'].update(width=390, height=844)
        for event in self.data['events'][2:]:
            event['sha256'] = digest
        self.assertEqual(self.result(), [])

    def test_frozen_first_output(self):
        self.data['mode'] = 'frozen-trial'
        self.assertEqual(self.result(), [])
        self.data['events'][2], self.data['events'][3] = self.data['events'][3], self.data['events'][2]
        self.data['events'][2]['seq'] = 3
        self.data['events'][3]['seq'] = 4
        self.assertTrue(self.result())

    def test_modified_original(self):
        (self.root / 'first.png').write_bytes(PNG + b'changed')
        self.assertTrue(self.result())

    def test_symlink(self):
        (self.root / 'link.png').symlink_to(self.root / 'first.png')
        self.gen['output']['path'] = 'link.png'
        self.assertTrue(self.result())

    def test_json_rejections(self):
        for text in ('{"schema_version":1,"schema_version":1}', '{"x":NaN}'):
            (self.root / 'run.json').write_text(text)
            self.assertTrue(check(self.root, 'run.json'))

    def test_optional_research_can_be_text_only(self):
        self.gen.update(image_inputs=[], conditioning='text-only', required_reference_seqs=[])
        self.gen['arguments'].pop('referenced_image_paths')
        self.assertEqual(self.result(), [])

    def test_required_reference_must_reach_input(self):
        self.gen['required_reference_seqs'] = [1]
        self.assertEqual(self.result(), [])
        self.gen.update(image_inputs=[], conditioning='text-only')
        self.gen['arguments'].pop('referenced_image_paths')
        self.assertIn('required image input omitted without approval evidence', self.result())
        self.gen['reference_omission_approval'] = 'explicit omission authorized for this run'
        self.assertEqual(self.result(), [])

    def test_partial_required_omission(self):
        ref = copy.deepcopy(self.data['events'][0])
        ref['seq'] = 2
        self.data['events'].insert(1, ref)
        for event in self.data['events'][2:]:
            event['seq'] += 1
        self.data['events'][3]['generation_seq'] = 3
        self.data['events'][4]['generation_seq'] = 3
        self.gen['required_reference_seqs'] = [1, 2]
        self.assertIn('required image input omitted without approval evidence', self.result())
        self.gen['reference_omission_approval'] = 'authorized omission of the second required image'
        self.assertEqual(self.result(), [])

    def test_invalid_required_references(self):
        for value in (None, {}, [True], [99], [1, 1]):
            with self.subTest(value=value):
                self.gen['required_reference_seqs'] = value
                self.assertTrue(self.result())

    def test_headers(self):
        self.assertEqual(dimensions(PNG), (1, 1))
        # Synthetic SOF dimensions fixture, not a full decodable JPEG.
        jpeg = b'\xff\xd8\xff\xc0\x00\x0b\x08\x00\x64\x00\xc8\x01\x01\x11\x00'
        self.assertEqual(dimensions(jpeg), (200, 100))
        for bad in (b'text', PNG[:20], PNG[:29] + b'xxxx', jpeg[:10], b'\xff\xd8\xff\xd9'):
            with self.assertRaises(EvidenceError):
                dimensions(bad)

    def test_blank_template_not_valid_run(self):
        template = Path(__file__).resolve().parents[1] / 'skills/mockup-image/templates/image-run.json'
        self.data = json.loads(template.read_text())
        self.assertTrue(self.result())


if __name__ == '__main__':
    unittest.main()
