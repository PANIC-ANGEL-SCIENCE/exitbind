#!/usr/bin/env python3
"""Stdlib regression tests; temporary evidence never becomes package content."""
import base64
import copy
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from check_run import check

# Tiny raster fixture; the checker deliberately checks signatures, not decoding.
PNG = base64.b64decode('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII=')
HASH = hashlib.sha256(PNG).hexdigest()


def run_fixture():
    return {
        'schema_version': 1, 'mode': 'ordinary', 'maker_id': 'maker',
        'pack_version': '6.5.0',
        'events': [
            {'seq': 1, 'kind': 'reference_view', 'maker_id': 'maker',
             'input_id': 'ref-a', 'capture_path': 'reference.png', 'sha256': HASH,
             'source_locator': 'project reference', 'source_state': 'captured',
             'observation': 'The headline occupies the left half.',
             'viewed': True, 'evidence_type': 'image-view', 'view_tool_result_id': 'view-1'},
            {'seq': 2, 'kind': 'direction', 'maker_id': 'maker',
             'decisions': [{'decision': 'Keep the large left headline.', 'source_refs': [1]}]},
            {'seq': 3, 'kind': 'render', 'direction_seq': 2,
             'artifact_path': 'render.png', 'sha256': HASH},
            {'seq': 4, 'kind': 'review', 'render_seq': 3, 'reviewer_id': 'reviewer',
             'viewed': True, 'evidence_type': 'image-view', 'view_tool_result_id': 'view-2',
             'sha256': HASH,
             'findings': [{'location': 'header title', 'observation': 'Crowded edge',
                           'impact': 'Weak reading margin', 'action': 'Disclose; frozen trial',
                           'status': 'remaining'}]}
        ]}


class EvidenceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='design-run-test-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        (self.root / 'reference.png').write_bytes(PNG)
        (self.root / 'render.png').write_bytes(PNG)
        self.data = run_fixture()

    def result(self):
        (self.root / 'run.json').write_text(json.dumps(self.data), encoding='utf-8')
        return check(self.root, 'run.json')

    def fails(self):
        self.assertTrue(self.result())

    def test_complete(self):
        self.assertEqual(self.result(), [])

    def test_honest_failed_frozen_quality(self):
        self.data['mode'] = 'frozen-trial'
        self.data['quality_outcome'] = 'failed'
        self.assertEqual(self.result(), [])

    def test_arbitrary_style_and_remaining_flaw(self):
        self.data['style'] = 'ornate chrome gradients with dense comic typography'
        self.assertEqual(self.result(), [])

    def test_absent_reference(self):
        (self.root / 'reference.png').unlink()
        self.fails()

    def test_absent_render(self):
        (self.root / 'render.png').unlink()
        self.fails()

    def test_wrong_reference_hash(self):
        self.data['events'][0]['sha256'] = '0' * 64
        self.fails()

    def test_wrong_render_hash(self):
        self.data['events'][2]['sha256'] = '0' * 64
        self.fails()

    def test_wrong_review_hash(self):
        self.data['events'][3]['sha256'] = '0' * 64
        self.fails()

    def test_changed_output(self):
        (self.root / 'render.png').write_bytes(PNG + b'changed')
        self.fails()

    def test_text_masquerading_as_raster(self):
        data = b'Image summary: headline on left'
        (self.root / 'reference.png').write_bytes(data)
        self.data['events'][0]['sha256'] = hashlib.sha256(data).hexdigest()
        self.fails()

    def test_text_summary_evidence(self):
        self.data['events'][0]['evidence_type'] = 'text-summary'
        self.fails()

    def test_not_viewed_reference(self):
        self.data['events'][0]['viewed'] = False
        self.fails()

    def test_not_viewed_review(self):
        self.data['events'][3]['viewed'] = False
        self.fails()

    def test_metadata_review(self):
        self.data['events'][3]['evidence_type'] = 'metadata'
        self.fails()

    def test_wrong_maker(self):
        self.data['events'][0]['maker_id'] = 'handoff-inspector'
        self.fails()

    def test_wrong_direction_maker(self):
        self.data['events'][1]['maker_id'] = 'other-maker'
        self.fails()

    def test_unordered_events(self):
        self.data['events'][0], self.data['events'][1] = self.data['events'][1], self.data['events'][0]
        self.fails()

    def test_duplicate_seq(self):
        self.data['events'][1]['seq'] = 1
        self.fails()

    def test_boolean_seq(self):
        self.data['events'][0]['seq'] = True
        self.fails()

    def test_render_before_direction(self):
        del self.data['events'][1]
        self.fails()

    def test_future_source_reference(self):
        self.data['events'][1]['decisions'][0]['source_refs'] = [4]
        self.fails()

    def test_missing_source_reference(self):
        self.data['events'][1]['decisions'][0]['source_refs'] = []
        self.fails()

    def test_missing_review(self):
        self.data['events'].pop()
        self.fails()

    def test_stale_review_after_new_render(self):
        render = copy.deepcopy(self.data['events'][2])
        render['seq'] = 4
        self.data['events'][3]['seq'] = 5
        self.data['events'].insert(3, render)
        self.fails()

    def test_new_render_after_review(self):
        render = copy.deepcopy(self.data['events'][2])
        render['seq'] = 5
        self.data['events'].append(render)
        self.fails()

    def test_new_direction_after_review(self):
        direction = copy.deepcopy(self.data['events'][1])
        direction['seq'] = 5
        self.data['events'].append(direction)
        self.fails()

    def test_unrelated_review_target(self):
        self.data['events'][3]['render_seq'] = 1
        self.fails()

    def test_missing_observation(self):
        self.data['events'][0]['observation'] = ' '
        self.fails()

    def test_missing_tool_result(self):
        self.data['events'][3]['view_tool_result_id'] = None
        self.fails()

    def test_missing_finding_details(self):
        del self.data['events'][3]['findings'][0]['location']
        self.fails()

    def test_empty_findings(self):
        self.data['events'][3]['findings'] = []
        self.fails()

    def test_traversal(self):
        self.data['events'][0]['capture_path'] = '../reference.png'
        self.fails()

    def test_absolute_path(self):
        self.data['events'][0]['capture_path'] = str(self.root / 'reference.png')
        self.fails()

    def test_remote_capture_not_fetched(self):
        self.data['events'][0]['capture_path'] = 'https:' + '/' * 2 + 'invalid.example/image.png'
        self.fails()

    def test_remote_locator_is_only_metadata(self):
        self.data['events'][0]['source_locator'] = 'https:' + '/' * 2 + 'invalid.example/reference'
        self.assertEqual(self.result(), [])

    def test_symlink_capture(self):
        (self.root / 'alias.png').symlink_to(self.root / 'reference.png')
        self.data['events'][0]['capture_path'] = 'alias.png'
        self.fails()

    def test_symlink_parent(self):
        (self.root / 'alias').symlink_to(self.root, target_is_directory=True)
        self.data['events'][0]['capture_path'] = 'alias/reference.png'
        self.fails()

    def test_symlink_evidence(self):
        self.result()
        (self.root / 'alias.json').symlink_to(self.root / 'run.json')
        self.assertTrue(check(self.root, 'alias.json'))

    def test_invalid_json(self):
        (self.root / 'run.json').write_text('{')
        self.assertTrue(check(self.root, 'run.json'))

    def test_duplicate_json_key(self):
        self.result()
        path = self.root / 'run.json'
        path.write_text(path.read_text().replace('"schema_version": 1', '"schema_version": 1, "schema_version": 1'))
        self.assertTrue(check(self.root, 'run.json'))

    def test_overflow_exponent_number(self):
        self.result()
        path = self.root / 'run.json'
        path.write_text(path.read_text()[:-1] + ', "extra": 1e999}')
        self.assertTrue(check(self.root, 'run.json'))

    def test_nonfinite_number(self):
        self.data['extra'] = float('nan')
        self.fails()

    def test_invalid_top_types(self):
        for value in (None, [], True, 42, 'summary'):
            with self.subTest(value=value):
                self.data = value
                self.fails()

    def test_invalid_nested_types(self):
        for index, field, value in ((0, 'seq', []), (0, 'capture_path', {}),
                                   (0, 'sha256', []), (1, 'decisions', {}),
                                   (1, 'decisions', [False]), (3, 'findings', [None]),
                                   (3, 'render_seq', [])):
            with self.subTest(field=field, value=value):
                self.data = run_fixture()
                self.data['events'][index][field] = value
                self.fails()

    def test_multiple_reviewed_iterations(self):
        direction = copy.deepcopy(self.data['events'][1])
        direction['seq'] = 5
        render = copy.deepcopy(self.data['events'][2])
        render.update(seq=6, direction_seq=5)
        review = copy.deepcopy(self.data['events'][3])
        review.update(seq=7, render_seq=6, view_tool_result_id='view-3')
        self.data['events'].extend([direction, render, review])
        self.assertEqual(self.result(), [])

    def test_frozen_trial_rejects_multiple_complete_screen_renders(self):
        render = copy.deepcopy(self.data['events'][2])
        render['seq'] = 5
        review = copy.deepcopy(self.data['events'][3])
        review.update(seq=6, render_seq=5, view_tool_result_id='view-3')
        self.data['events'].extend([render, review])
        self.data['mode'] = 'frozen-trial'
        self.assertIn('frozen-trial permits only one complete-screen render', self.result())

    def test_invalid_utf8(self):
        (self.root / 'run.json').write_bytes(bytes([255]))
        self.assertTrue(check(self.root, 'run.json'))

    def test_empty_events(self):
        self.data['events'] = []
        self.fails()

    def test_boolean_source_reference(self):
        self.data['events'][1]['decisions'][0]['source_refs'] = [True]
        self.fails()

    def test_missing_evidence(self):
        self.assertTrue(check(self.root, 'missing.json'))

    def test_symlink_project_root(self):
        self.result()
        alias = self.root / 'alias'
        alias.symlink_to(self.root, target_is_directory=True)
        self.assertTrue(check(alias, 'run.json'))

    def test_template_is_unpopulated_not_passing(self):
        path = Path(__file__).resolve().parents[1] / 'skills/design-pack/templates/screen-run.json'
        self.data = json.loads(path.read_text())
        self.assertEqual(self.data['events'], [])
        self.fails()


if __name__ == '__main__':
    unittest.main()
