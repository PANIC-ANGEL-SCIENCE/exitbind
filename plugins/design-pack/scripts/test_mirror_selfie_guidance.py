#!/usr/bin/env python3
"""Mirror-selfie text contracts and declared gates, not optical or age analysis."""
import json
from pathlib import Path
import tempfile
import unittest

from check_outcome import check
from test_attitude_guidance import outcome_fixture
from test_check_run import PNG

ROOT = Path(__file__).resolve().parents[1]
GUIDANCE = ROOT / 'skills/mockup-image/references/mirror-selfie.md'

# These exact passages are deletion guards, not semantic/pixel evaluators.
CONTRACTS = {
    'source_and_mode': (
        'Open relevant source pixels at a useful scale',
        'Distinguish a mirror selfie from a front-camera selfie',
        'small thumbnails cannot resolve finger placement, lens position or reflection detail',
        'A visible phone alone does not make a photograph a mirror selfie'),
    'no_fixed_recipe': (
        'Record visible frame-left/frame-right before asserting',
        'do not infer its camera mode or editing history from appearance',
        'Do not require a visible mirror border, a particular hand, heavy phone occlusion',
        'Retain enough contextual evidence to make the requested capture mode legible'),
    'physical_arrangement': (
        'Choose the camera/phone, mirror plane, person and room arrangement together',
        'the lens points toward the mirror',
        'the reflected phone back faces the viewer and the screen faces the holder',
        'do not silently substitute an external camera viewpoint'),
    'hand_and_reflection': (
        'Trace the phone-holding hand through wrist, elbow and shoulder',
        'do not invent hidden fingers merely to complete a count',
        'Check reflection geometry as a scene, not by applying a blanket horizontal flip',
        'must agree with the mirror plane and selected viewpoint',
        'Do not add brand marks or text just to demonstrate mirroring'),
    'lighting': (
        'Light the person and reflected room together',
        'Do not double the illumination merely because a source is visible in reflection',
        'Flash and no-flash are separate supported choices',
        'glare and blown highlights are not mandatory selfie effects'),
    'life_stage': (
        'assess face and whole-body presentation together at source detail and receiving size',
        'rather than assuming one facial measurement proves age',
        'Do not use wrinkle removal, beauty smoothing, infantile proportions',
        'A phone covering the face must not conceal an unresolved facial-direction issue',
        'visual review supports task fit, not exact age verification'),
    'independent_review': (
        'Separately review capture-mode legibility, hand/phone anatomy, reflection geometry, lighting',
        'Keep any central failed axis in rework even when the others pass',
        'Record unresolved occlusion and detail limits honestly',
        'Written prompts and structural tests cannot establish these visual results'),
}


class MirrorSelfieGuidanceTests(unittest.TestCase):
    def test_written_contracts_present(self):
        text = GUIDANCE.read_text()
        for name, cues in CONTRACTS.items():
            with self.subTest(contract=name):
                self.assertEqual([cue for cue in cues if cue not in text], [])

    def test_each_deleted_contract_cue_is_detected(self):
        text = GUIDANCE.read_text()
        for name, cues in CONTRACTS.items():
            for cue in cues:
                with self.subTest(contract=name, removed=cue):
                    mutated = text.replace(cue, '')
                    self.assertEqual([item for item in cues if item not in mutated], [cue])

    def test_direct_routes_present(self):
        for path, route in {
            'skills/mockup-image/SKILL.md': 'references/mirror-selfie.md',
            'skills/mockup-image/references/capture-styling.md': 'mirror-selfie.md',
            'skills/mockup-image/references/generation-contract.md': 'mirror-selfie.md',
            'skills/mockup-image/references/model-direction.md': 'mirror-selfie.md',
        }.items():
            with self.subTest(path=path):
                self.assertIn('](' + route + ')', (ROOT / path).read_text())

    def test_brief_has_scoped_inputs_and_actual_review(self):
        text = (ROOT / 'skills/mockup-image/templates/image-brief.md').read_text()
        for cue in ('If mirror selfie: actual capture mode',
                    'observed frame coordinates versus uncertain handedness',
                    'separate capture-mode, phone/hand, reflection and lighting findings',
                    'face/body life-stage consistency at source detail and final use size'):
            self.assertIn(cue, text)

    def test_scenarios_include_ordinary_alternatives(self):
        text = (ROOT / 'EVALUATION.md').read_text()
        for cue in ('External-camera portrait with a decorative phone',
                    'Centred figure with unobscured face in an actual mirror reference',
                    'Phone hand looks plausible but reflected room disagrees',
                    'Face/body life-stage mismatch hidden behind the phone',
                    'No-flash mirror image with a bright background'):
            self.assertIn(cue, text)

    def test_limits_explicit(self):
        text = (ROOT / 'VALIDATION.md').read_text()
        for cue in ('not ray tracing, hand-anatomy detection or age estimation',
                    'No mirror image is generated by these tests',
                    'cannot prove that guidance was read before generation'):
            self.assertIn(cue, text)


def mirror_fixture():
    """The raster signature fixture depicts no person, hand or mirror."""
    data = outcome_fixture()
    for axis in ('mirror-mode', 'mirror-hand', 'mirror-geometry', 'mirror-light'):
        data['requirements'].append({
            'id': axis, 'category': 'other', 'instruction': 'Synthetic task requires ' + axis + '.',
            'targets': [{'artifact_id': 'sample', 'location': axis}]})
        data['checks'].append({
            'requirement_id': axis, 'artifact_id': 'sample', 'location': axis,
            'result': 'pass', 'view_ids': ['sample-view'],
            'observation': 'Synthetic declared ' + axis + '; no visual judgment performed.'})
    return data


class IndependentMirrorOutcomes(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='mirror-contract-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / 'sample.png').write_bytes(PNG)
        self.data = mirror_fixture()

    def result(self):
        (self.root / 'outcome.json').write_text(json.dumps(self.data))
        return check(self.root, 'outcome.json')

    def test_declared_complete_record_passes_without_visual_claim(self):
        self.assertEqual(self.result(), [])

    def test_each_mirror_failure_blocks_ready_independently(self):
        for index in (-4, -3, -2, -1):
            with self.subTest(index=index):
                self.data = mirror_fixture()
                self.data['checks'][index].update(result='fail')
                self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_geometry_pass_cannot_override_life_stage_failure(self):
        self.data['checks'][0].update(result='fail')
        self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_hidden_untested_face_fit_blocks_ready(self):
        self.data['checks'][0].update(result='not-tested', view_ids=[])
        self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_unreviewed_reflection_blocks_ready(self):
        self.data['checks'][-2].update(result='not-tested', view_ids=[])
        self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_missing_mirror_check_rejected(self):
        self.data['checks'].pop()
        self.assertTrue(self.result())

    def test_honest_rework_valid(self):
        self.data['status'] = 'rework'
        self.data['checks'][-3].update(result='fail')
        self.assertEqual(self.result(), [])

    def test_centered_clear_face_and_no_flash_not_globally_rejected(self):
        for declaration in ('centred figure fits source relationship',
                            'unobscured face fits camera arrangement',
                            'no-flash capture fits available room light'):
            with self.subTest(declaration=declaration):
                self.data['checks'][-4]['observation'] = 'Synthetic declared ' + declaration + '.'
                self.assertEqual(self.result(), [])

    def test_frozen_failure_is_rework_not_repair_authority(self):
        self.data.update(mode='frozen-trial', status='rework')
        self.data['checks'][-2].update(result='fail')
        self.assertEqual(self.result(), [])


if __name__ == '__main__':
    unittest.main()
