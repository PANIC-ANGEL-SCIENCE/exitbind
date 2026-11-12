#!/usr/bin/env python3
"""Written expression contracts and declared outcomes, never pixel classification."""
import json
from pathlib import Path
import tempfile
import unittest

from check_outcome import check
from test_attitude_guidance import outcome_fixture
from test_check_run import PNG

ROOT = Path(__file__).resolve().parents[1]
GUIDANCE = ROOT / 'skills/mockup-image/references/model-direction.md'

# Exact passages are deletion guards only, not a prompt or visual evaluator.
CONTRACTS = {
    'independent_expression': (
        'Treat facial expression intensity and intended attitude as separate judgments',
        'Do not maximize every cue at once',
        'pose, clothing wear and camera relationship carry most of its character'),
    'no_stacked_recipe': (
        'Check for facial cue stacking before generation',
        'a lowered chin, upward stare, narrowed or half-lidded eyes, raised brow and crooked smile/pout',
        'Do not use that bundle by default',
        'There is no universal cue count or numerical expression threshold'),
    'describe_moment_first': (
        "Describe the person's attention and action first",
        'only the facial detail needed to support them',
        'rather than accumulating synonyms for intensity or a long list of facial negatives'),
    'preserve_during_correction': (
        'first reduce the excessive facial tension or performed gesture',
        'preserving the intended attitude, age/life-stage, successful pose, styling/wear, camera relationship and capture',
        'adjust it only if it materially causes the failure',
        'rather than silently changing protected direction'),
    'not_blank_or_retail': (
        'A quieter expression is not permission to make the person generic',
        'instead of replacing the entire scene with a friendly retail portrait',
        'No permanent neutral expression, closed-mouth rule or smile ban applies',
        'an expressive face remains valid when the task and inspected references support it'),
    'styling_separate': (
        'do not escalate them merely to compensate for a quieter face',
        'bold makeup does not require a hard stare',
        'a soft face does not require bland styling'),
    'independent_rework': (
        'Inspect expression fit independently at face detail, whole-scene and receiving-crop scales',
        'If facial overperformance remains, the image stays in rework even when attitude, casting and phone texture pass',
        'If the face is quieter but the intended attitude disappears, that revision also stays in rework',
        'A successful prompt edit or written restraint cannot establish either result'),
    'photographic_surface_not_diagnosis': (
        'Keep this review separate from expression and from an approved layout',
        "Do not diagnose a viewer's response or claim a medical or psychological explanation",
        'Preserve genuine textile construction, garment folds, skin variation and intentionally detailed styling',
        'Do not use global blur, beauty smoothing, blanket desaturation or added fake grain as a realism repair',
        'Do not change face geometry merely because an image is disliked',
        'hold layout, UI, copy and composition fixed within the current change scope',
        'Expression improvement cannot pass an unresolved photographic-surface failure',
        'If the cause remains uncertain, report the uncertainty'),
    'no_soft_gaze_penalty': (
        'A softened gaze is not itself a defect',
        'judge whether it serves the requested moment'),
}


def missing_contract(text, cues):
    return [cue for cue in cues if cue not in text]


class ExpressionGuidanceTests(unittest.TestCase):
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

    def test_production_routes_distinguish_expression(self):
        routes = {
            'skills/mockup-image/SKILL.md': 'Calibrate expression to the actual moment instead of stacking intense facial cues',
            'skills/mockup-image/references/image-production.md': 'Evaluate expression intensity separately from intended attitude',
            'skills/mockup-image/references/generation-contract.md': 'Separate expression intensity from attitude before prompting',
        }
        for rel, cue in routes.items():
            with self.subTest(path=rel):
                self.assertIn(cue, (ROOT / rel).read_text())

    def test_brief_requires_separate_intent_and_review(self):
        text = (ROOT / 'skills/mockup-image/templates/image-brief.md').read_text()
        for cue in ('Expression intensity independently of attitude',
                    'unsupported facial cue stacking to remove',
                    'Expression fit and retained attitude after correction, judged independently',
                    'excess tension and generic-flattening findings'):
            self.assertIn(cue, text)

    def test_scenarios_and_test_limits_explicit(self):
        text = (ROOT / 'EVALUATION.md').read_text()
        for cue in ('Creative attitude with lightly engaged reference faces',
                    'Lowered chin, upward stare and crooked mouth stacked by default',
                    'Expression is too much; preserve the rest',
                    'Quieter face but attitude disappears',
                    'Small smile or ordinary attention fits the reference',
                    'Explicit expressive performance brief',
                    'Strong makeup with relaxed expression',
                    'Whole scene works but close receiving crop overemphasizes the face',
                    'not unseen cue stacking'):
            self.assertIn(cue, text)
        validation = (ROOT / 'VALIDATION.md').read_text()
        for cue in ('They do not analyze prompt semantics, detect expressions in images',
                    'record-logic tests, not facial classifiers or executed image trials',
                    'A false pass or omitted task requirement can evade them'):
            self.assertIn(cue, validation)


def expression_fixture():
    """Synthetic observations only; the PNG bytes do not depict a face."""
    data = outcome_fixture()
    data['contract_source'] = 'Synthetic brief: independently assess expression fit and retained attitude.'
    data['requirements'].append({
        'id': 'expression-fit', 'category': 'other',
        'instruction': 'Synthetic task requires expression suited to its scene.',
        'targets': [{'artifact_id': 'sample', 'location': 'expression-fit'}]})
    data['checks'].append({
        'requirement_id': 'expression-fit', 'artifact_id': 'sample',
        'location': 'expression-fit', 'result': 'pass',
        'observation': 'Synthetic declared expression fit; no visual judgment performed.',
        'view_ids': ['sample-view']})
    return data


class IndependentExpressionOutcomeTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='expression-contract-test-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / 'sample.png').write_bytes(PNG)
        self.data = expression_fixture()

    def result(self):
        (self.root / 'outcome.json').write_text(json.dumps(self.data))
        return check(self.root, 'outcome.json')

    def test_declared_complete_record_accepted_without_visual_claim(self):
        self.assertEqual(self.result(), [])

    def test_declared_cue_stacking_blocks_ready_even_with_other_passes(self):
        self.data['checks'][-1].update(
            result='fail', observation='Synthetic declared unsupported facial cue stacking.')
        self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_declared_overperformed_correction_blocks_ready(self):
        self.data['checks'][-1].update(
            result='fail', observation='Synthetic declared correction still overperforms the facial expression.')
        self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_quieter_face_cannot_override_attitude_loss(self):
        self.data['checks'][-1]['observation'] = 'Synthetic declared quieter expression fits.'
        self.data['checks'][1].update(
            result='fail', observation='Synthetic declared intended attitude lost during correction.')
        self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_unreviewed_expression_blocks_ready(self):
        self.data['checks'][-1].update(result='not-tested', view_ids=[])
        self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_missing_expression_check_blocks_ready(self):
        self.data['checks'].pop()
        self.assertTrue(self.result())

    def test_expression_rework_is_honest_record(self):
        self.data['status'] = 'rework'
        self.data['checks'][-1].update(result='fail')
        self.assertEqual(self.result(), [])

    def test_small_smile_and_intense_brief_are_not_globally_rejected(self):
        for expression in ('small smile supports quiet scene', 'intense expression supports explicit performance brief'):
            with self.subTest(declaration=expression):
                self.data['checks'][-1]['observation'] = 'Synthetic declared ' + expression + '.'
                self.assertEqual(self.result(), [])

    def test_frozen_expression_failure_is_valid_rework_not_edit_permission(self):
        self.data.update(mode='frozen-trial', status='rework')
        self.data['checks'][-1].update(result='fail')
        self.assertEqual(self.result(), [])


class IndependentSurfaceOutcomeTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='surface-contract-test-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / 'sample.png').write_bytes(PNG)
        self.data = expression_fixture()
        self.data['requirements'].append({
            'id': 'surface-fit', 'category': 'other',
            'instruction': 'Synthetic task requires photographic surface fit.',
            'targets': [{'artifact_id': 'sample', 'location': 'surface-fit'}]})
        self.data['checks'].append({
            'requirement_id': 'surface-fit', 'artifact_id': 'sample',
            'location': 'surface-fit', 'result': 'pass',
            'observation': 'Synthetic declared surface fit; no visual judgment performed.',
            'view_ids': ['sample-view']})

    def result(self):
        (self.root / 'outcome.json').write_text(json.dumps(self.data))
        return check(self.root, 'outcome.json')

    def test_declared_complete_record_passes_without_visual_claim(self):
        self.assertEqual(self.result(), [])

    def test_expression_pass_cannot_override_declared_surface_failure(self):
        self.data['checks'][-1].update(
            result='fail', observation='Synthetic declared invented texture repetition still fails.')
        self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_unreviewed_surface_blocks_ready(self):
        self.data['checks'][-1].update(result='not-tested', view_ids=[])
        self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_surface_repair_cannot_override_lost_attitude(self):
        self.data['checks'][1].update(result='fail')
        self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_honest_surface_rework_passes(self):
        self.data['status'] = 'rework'
        self.data['checks'][-1].update(result='fail')
        self.assertEqual(self.result(), [])

    def test_legitimate_detail_not_globally_rejected(self):
        self.data['checks'][-1]['observation'] = 'Synthetic declared authentic fabric pattern and skin variation fit the task.'
        self.assertEqual(self.result(), [])


if __name__ == '__main__':
    unittest.main()
