#!/usr/bin/env python3
"""Synthetic gate fixtures, not visual production or taste evaluation."""
import copy
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from check_outcome import check
from test_check_run import PNG

HASH = hashlib.sha256(PNG).hexdigest()
PDF = b'%PDF-1.7\nsynthetic signature fixture, not a rendered document\n%%EOF\n'
PDF_HASH = hashlib.sha256(PDF).hexdigest()


def fixture():
    return {
        'schema_version': 1, 'mode': 'ordinary', 'status': 'ready-for-user-review',
        'contract_source': 'Synthetic brief: capsule action on opening page; connected PDF.',
        'requirements': [
            {'id': 'shape', 'category': 'shape', 'instruction': 'Only opening action must be capsule.',
             'shape': 'capsule', 'targets': [{'artifact_id': 'one', 'location': 'action'}]},
            {'id': 'names', 'category': 'language', 'instruction': 'Preserve names consistently across the pair.',
             'targets': [{'artifact_id': 'one', 'location': 'names'}, {'artifact_id': 'two', 'location': 'names'}]}],
        'artifacts': [
            {'id': 'pdf', 'kind': 'pdf', 'path': 'series.pdf', 'sha256': PDF_HASH, 'produced_seq': 3},
            {'id': 'one', 'kind': 'raster', 'path': 'one.png', 'sha256': HASH, 'produced_seq': 4,
             'derived_from': {'artifact_id': 'pdf', 'sha256': PDF_HASH, 'page_number': 1}},
            {'id': 'two', 'kind': 'raster', 'path': 'two.png', 'sha256': HASH, 'produced_seq': 4,
             'derived_from': {'artifact_id': 'pdf', 'sha256': PDF_HASH, 'page_number': 2}},
            {'id': 'overview', 'kind': 'raster', 'path': 'overview.png', 'sha256': HASH, 'produced_seq': 5},
            {'id': 'study', 'kind': 'raster', 'path': 'study.png', 'sha256': HASH, 'produced_seq': 1}],
        'views': [{'id': aid+'-view', 'artifact_id': aid, 'sha256': HASH, 'seq': 6,
                   'evidence_type': 'image-view', 'view_tool_result_id': 'synthetic-'+aid,
                   'scale': 'overview' if aid == 'overview' else 'use'} for aid in ('one', 'two', 'overview')] + [
                   {'id': 'study-view', 'artifact_id': 'study', 'sha256': HASH, 'seq': 2,
                    'evidence_type': 'image-view', 'view_tool_result_id': 'synthetic-study', 'scale': 'overview'}],
        'checks': [
            {'requirement_id': 'shape', 'artifact_id': 'one', 'location': 'action', 'result': 'pass',
             'observation': 'Synthetic measurement case only.', 'view_ids': ['one-view'],
             'contour_observation': 'Illustrative full end caps.', 'affordance_observation': 'Illustrative action label remains distinct.',
             'geometry': {'width': 144, 'height': 40, 'effective_corner_radii': [20]*4, 'unit': 'px'}},
            *[{'requirement_id': 'names', 'artifact_id': aid, 'location': 'names', 'result': 'pass',
               'observation': 'Synthetic names retained; visual judgment not performed.', 'view_ids': [aid+'-view']}
              for aid in ('one', 'two')]],
        'sequence': {
            'format': 'pdf', 'ordered_pages': ['one', 'two'], 'overview_id': 'overview',
            'overview_members': [{'artifact_id': aid, 'sha256': HASH} for aid in ('one', 'two')],
            'pdf_id': 'pdf', 'pdf_page_count': 2, 'sequence_first': True,
            'plan': {'seq': 1, 'study_artifact_id': 'study', 'study_view_id': 'study-view', 'ordered_pages': ['one', 'two'], 'argument': 'Synthetic introduction and evidence.',
                     'contributions': [{'artifact_id': aid, 'contribution': 'Synthetic '+aid} for aid in ('one', 'two')],
                     'transition': 'Synthetic change of evidence scale.'},
            'review': {'result': 'pass', 'view_ids': ['one-view', 'two-view', 'overview-view'],
                       'hierarchy': 'Synthetic first-read check.', 'transition': 'Synthetic turn check.',
                       'continuity': 'Synthetic identity check.', 'reading_size': 'Synthetic use-size check.'}}}


class OutcomeTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='outcome-test-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        for name in ('one', 'two', 'overview', 'study'):
            (self.root / (name+'.png')).write_bytes(PNG)
        (self.root / 'series.pdf').write_bytes(PDF)
        self.data = fixture()

    def result(self):
        (self.root / 'outcome.json').write_text(json.dumps(self.data))
        return check(self.root, 'outcome.json')

    def test_complete_record(self):
        self.assertEqual(self.result(), [])

    def test_single_original_no_sequence_or_pill(self):
        self.data.pop('sequence')
        self.data['requirements'] = [self.data['requirements'][1]]
        self.data['checks'] = self.data['checks'][1:]
        self.assertEqual(self.result(), [])

    def test_continuity_does_not_force_sequence(self):
        self.data.pop('sequence')
        self.data['requirements'][1]['category'] = 'continuity'
        self.assertEqual(self.result(), [])

    def test_non_pdf_series(self):
        self.data['sequence'].update(format='raster-series', pdf_id=None)
        self.assertEqual(self.result(), [])

    def test_no_sequence_first_claim(self):
        self.data['sequence']['sequence_first'] = False
        self.data['sequence'].pop('plan')
        self.assertEqual(self.result(), [])

    def test_unrelated_shapes_are_unconstrained(self):
        self.data['artifacts'][2]['description'] = 'Square and irregular controls intentionally outside shape scope.'
        self.assertEqual(self.result(), [])

    def test_explicit_square_supported(self):
        self.data['requirements'][0]['shape'] = 'square'
        self.data['checks'][0].pop('geometry')
        self.assertEqual(self.result(), [])

    def test_circle(self):
        self.data['requirements'][0]['shape'] = 'circle'
        self.data['checks'][0]['geometry'].update(width=40)
        self.assertEqual(self.result(), [])

    def test_honest_rework(self):
        self.data['status'] = 'rework'
        self.data['checks'][0]['result'] = 'fail'
        self.data['checks'][0]['geometry']['effective_corner_radii'] = [4]*4
        self.data['sequence']['review']['result'] = 'fail'
        self.assertEqual(self.result(), [])

    def test_honest_frozen_rework(self):
        self.data.update(mode='frozen-trial', status='rework')
        self.data['checks'][0].update(result='not-tested', view_ids=[])
        self.data['sequence']['review'].update(result='not-tested', view_ids=[])
        self.assertEqual(self.result(), [])

    def test_frozen_sequence_uses_text_plan_without_rendered_study(self):
        self.data['mode'] = 'frozen-trial'
        self.data['sequence']['plan'].pop('study_artifact_id')
        self.data['sequence']['plan'].pop('study_view_id')
        self.data['artifacts'] = self.data['artifacts'][:-1]
        self.data['views'] = self.data['views'][:-1]
        self.assertEqual(self.result(), [])

    def test_relationship_observation_not_membership_verdict(self):
        self.data['requirements'][0].update(category='relational-identity')
        self.data['checks'][0].update(observed_relationship='Hands engaged with a shared activity; camera at peer distance.',
                                    interpretation='May read as participating.', alternative_reading='Could still be staged.')
        self.assertEqual(self.result(), [])

    def test_mutations(self):
        cases = {
            'slightly rounded rectangle': lambda d: d['checks'][0]['geometry'].update(effective_corner_radii=[4]*4),
            'pill called circle': lambda d: d['requirements'][0].update(shape='circle'),
            'unmeasured circle': lambda d: d['checks'][0].pop('geometry'),
            'no affordance observation': lambda d: d['checks'][0].pop('affordance_observation'),
            'empty contour observation': lambda d: d['checks'][0].update(contour_observation=''),
            'missing target coverage': lambda d: d['checks'].pop(),
            'failed shape marked ready': lambda d: d['checks'][0].update(result='fail'),
            'untested language marked ready': lambda d: d['checks'][1].update(result='not-tested'),
            'duplicate check': lambda d: d['checks'].append(copy.deepcopy(d['checks'][0])),
            'undeclared location': lambda d: d['checks'][0].update(location='unrequested'),
            'wrong artifact view': lambda d: d['checks'][0].update(view_ids=['two-view']),
            'stale view': lambda d: d['views'][0].update(sha256='0'*64),
            'metadata masquerades as view': lambda d: d['views'][0].update(evidence_type='metadata'),
            'view before artifact': lambda d: d['views'][0].update(seq=3),
            'duplicated page': lambda d: d['sequence'].update(ordered_pages=['one','one']),
            'missing overview': lambda d: d['sequence'].update(overview_id='one'),
            'stale overview': lambda d: d['sequence']['overview_members'][1].update(sha256='0'*64),
            'wrong overview order': lambda d: d['sequence']['overview_members'].reverse(),
            'overview before pages': lambda d: d['artifacts'][3].update(produced_seq=2),
            'missing visual study': lambda d: d['sequence']['plan'].pop('study_artifact_id'),
            'final page as visual study': lambda d: d['sequence']['plan'].update(study_artifact_id='one'),
            'unviewed study': lambda d: d['sequence']['plan'].pop('study_view_id'),
            'study after final PDF': lambda d: d['views'][3].update(seq=3),
            'missing plan': lambda d: d['sequence'].pop('plan'),
            'late plan': lambda d: d['sequence']['plan'].update(seq=9),
            'wrong plan order': lambda d: d['sequence']['plan']['ordered_pages'].reverse(),
            'missing page contribution': lambda d: d['sequence']['plan']['contributions'].pop(),
            'missing pdf': lambda d: d['sequence'].update(pdf_id='missing'),
            'wrong PDF count': lambda d: d['sequence'].update(pdf_page_count=3),
            'stale PDF page render': lambda d: d['artifacts'][1]['derived_from'].update(sha256='0'*64),
            'PDF rendered pages wrong order': lambda d: d['artifacts'][1]['derived_from'].update(page_number=2),
            'page render before PDF': lambda d: d['artifacts'][1].update(produced_seq=2),
            'isolated page review': lambda d: d['sequence']['review'].update(view_ids=['one-view','two-view']),
            'no second use-size review': lambda d: d['views'][1].update(scale='detail'),
            'no continuity observation': lambda d: d['sequence']['review'].pop('continuity'),
            'sequence failed marked ready': lambda d: d['sequence']['review'].update(result='fail'),
            'no contract source': lambda d: d.pop('contract_source'),
            'unbound relationship claim': lambda d: d['requirements'][0].update(category='relational-identity'),
            'boolean PDF page number': lambda d: d['artifacts'][1]['derived_from'].update(page_number=True),
            'floating PDF page number': lambda d: d['artifacts'][1]['derived_from'].update(page_number=1.0),
            'huge shape integer': lambda d: d['checks'][0]['geometry'].update(width=10**1000),
            'plan after PDF': lambda d: d['sequence']['plan'].update(seq=3),
            'invalid shape number': lambda d: d['checks'][0]['geometry'].update(width=True),
            'path traversal': lambda d: d['artifacts'][0].update(path='../series.pdf'),
        }
        for name, change in cases.items():
            with self.subTest(name=name):
                self.data = fixture(); change(self.data)
                self.assertTrue(self.result(), name)


if __name__ == '__main__':
    unittest.main()
