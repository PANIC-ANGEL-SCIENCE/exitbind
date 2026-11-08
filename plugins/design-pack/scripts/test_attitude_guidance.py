#!/usr/bin/env python3
"""Written contracts and declared-result fixtures, never visual taste evaluation."""
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from check_outcome import check
from test_check_run import PNG

ROOT = Path(__file__).resolve().parents[1]
GUIDANCE = ROOT / 'skills/mockup-image/references/model-direction.md'
HASH = hashlib.sha256(PNG).hexdigest()

# Exact passages protect guidance from accidental deletion. They cannot establish
# that the maker followed it or that an unseen image has the intended attitude.
CONTRACTS = {
    'preserve_on_correction': (
        'change → preserve → verify',
        'A correction to facial age or geometry must preserve the approved or still-required attitude',
        'stronger attitude must not restore an unwanted mature casting default'),
    'no_quality_axis_swap': (
        'Do not equate younger with plainer, softer with friendlier, realistic with generic, or confident with older',
        'If success on one axis regresses another, the revision remains in rework'),
    'no_profession_inference': (
        'Creative affiliation cannot be inferred from appearance',
        'no shared uniform, gaze, ethnic appearance or facial geometry',
        'not a profession identifiable from a face'),
    'concrete_stance': (
        'fashion, art, design or music professionals or aspirants',
        'attention, body, styling and relationship to the camera',
        'rather than appending a job title or making the stare harder'),
    'not_a_costume': (
        'Piercings, tattoos, distressed clothes, unusual glasses or a professional prop cannot earn the reading by themselves',
        'Do not invent a studio, instrument, art tool, venue or career history to prove belonging'),
    'hair_makeup_wear': (
        'Hair and makeup as decisions',
        'Do not erase these choices to signal youth or phone realism',
        'How the clothes are worn',
        'Preserve required garment construction'),
    'camera_not_noise': (
        'Camera relationship and moment',
        'Texture, noise and a device label cannot supply the relationship'),
    'no_expression_uniform': (
        'Neither direct eye contact nor a scowl is required',
        'Plain clothing, a smile, softness and restraint can carry an intentional attitude'),
    'independent_failure': (
        'A young-looking, anatomically sound, phone-realistic image can still fail the attitude brief',
        'An image with a suitable casting choice but an inert or generic moment remains in rework',
        'A scene with convincing attitude but the wrong requested life stage also remains in rework'),
    'actual_review_and_scope': (
        'At face detail, whole-scene and receiving-crop scales',
        'Static tests and prompt text cannot establish either result',
        'user acceptance remains separate',
        'Keep protected products, identity, UI and copy within their existing scope'),
}


def missing_contract(text, cues):
    return [cue for cue in cues if cue not in text]


class AttitudeGuidanceTests(unittest.TestCase):
    def test_written_contracts_present(self):
        text = GUIDANCE.read_text()
        for case, cues in CONTRACTS.items():
            with self.subTest(case=case):
                self.assertEqual(missing_contract(text, cues), [])

    def test_removing_each_safeguard_is_detected(self):
        text = GUIDANCE.read_text()
        for case, cues in CONTRACTS.items():
            for cue in cues:
                with self.subTest(case=case, removed=cue):
                    self.assertEqual(missing_contract(text.replace(cue, ''), cues), [cue])

    def test_brief_distinguishes_intent_from_review(self):
        text = (ROOT / 'skills/mockup-image/templates/image-brief.md').read_text()
        for cue in ('Intended stance / participation as a visual hypothesis',
                    'Correction scope: failed axis → successful task cues to preserve',
                    'Separate actual-pixel findings for age/life-stage, attitude',
                    'Attitude can fail despite age/capture passing'):
            self.assertIn(cue, text)

    def test_production_routes_carry_correction_contract(self):
        files = {
            'skills/mockup-image/SKILL.md': 'a repair to one must preserve successful cues on the others',
            'skills/mockup-image/references/image-production.md': 'An age correction must not make intentional styling or stance generic',
            'skills/mockup-image/references/generation-contract.md': 'For corrections, state the failed axis and the successful cues to preserve',
        }
        for rel, cue in files.items():
            with self.subTest(path=rel):
                self.assertIn(cue, (ROOT / rel).read_text())

    def test_scenarios_and_limits_are_explicit(self):
        text = (ROOT / 'EVALUATION.md').read_text()
        for cue in ('Correct mature-default casting while retaining a chosen stance',
                    'Young phone-realistic result lacks intended attitude',
                    'Restore attitude without aging',
                    'Creative professional or aspirant requested',
                    'Stronger attitude requested',
                    'Plain clothing, smile or soft face fits the brief',
                    'Revision fixes one axis but regresses another',
                    'Full source carries attitude but final crop flattens it',
                    'not executed image trials'):
            self.assertIn(cue, text)
        validation = (ROOT / 'VALIDATION.md').read_text()
        for cue in ('not a portrait or an age/attitude classifier',
                    'The existing optional outcome schema is unchanged',
                    'No generation is performed by the package tests'):
            self.assertIn(cue, validation)


def outcome_fixture():
    """Only declared-result logic: the PNG signature fixture is not a portrait."""
    axes = ('age-fit', 'attitude', 'capture')
    return {
        'schema_version': 1, 'mode': 'ordinary', 'status': 'ready-for-user-review',
        'contract_source': 'Synthetic brief: independently assess age fit, chosen stance and capture.',
        'requirements': [
            {'id': axis, 'category': 'other', 'instruction': 'Synthetic task requires ' + axis + '.',
             'targets': [{'artifact_id': 'sample', 'location': axis}]} for axis in axes],
        'artifacts': [{'id': 'sample', 'kind': 'raster', 'path': 'sample.png',
                       'sha256': HASH, 'produced_seq': 1}],
        'views': [{'id': 'sample-view', 'artifact_id': 'sample', 'sha256': HASH,
                   'seq': 2, 'evidence_type': 'image-view',
                   'view_tool_result_id': 'synthetic-not-executed', 'scale': 'use'}],
        'checks': [
            {'requirement_id': axis, 'artifact_id': 'sample', 'location': axis,
             'result': 'pass', 'observation': 'Synthetic declared ' + axis + '; no visual judgment performed.',
             'view_ids': ['sample-view']} for axis in axes],
        'sequence': None,
    }


class IndependentOutcomeTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='attitude-contract-test-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / 'sample.png').write_bytes(PNG)
        self.data = outcome_fixture()

    def result(self):
        (self.root / 'outcome.json').write_text(json.dumps(self.data))
        return check(self.root, 'outcome.json')

    def test_declared_complete_record_accepted_without_visual_claim(self):
        self.assertEqual(self.result(), [])

    def test_age_and_capture_pass_cannot_override_attitude_failure(self):
        self.data['checks'][1].update(result='fail', observation='Synthetic generic stance despite declared age and capture passes.')
        self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_unreviewed_attitude_cannot_be_ready(self):
        self.data['checks'][1].update(result='not-tested', view_ids=[])
        self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_honest_attitude_failure_is_valid_rework(self):
        self.data['status'] = 'rework'
        self.data['checks'][1].update(result='fail')
        self.assertEqual(self.result(), [])

    def test_attitude_pass_cannot_override_age_or_capture_regression(self):
        for index in (0, 2):
            with self.subTest(axis=self.data['checks'][index]['requirement_id']):
                self.data = outcome_fixture()
                self.data['checks'][index].update(result='fail')
                self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_missing_attitude_check_cannot_be_ready(self):
        self.data['checks'].pop(1)
        self.assertTrue(self.result())

    def test_frozen_attitude_failure_is_valid_rework_without_repair(self):
        self.data.update(mode='frozen-trial', status='rework')
        self.data['checks'][1].update(result='fail')
        self.assertEqual(self.result(), [])


if __name__ == '__main__':
    unittest.main()
