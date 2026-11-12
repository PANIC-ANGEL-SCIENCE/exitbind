"""Text and declared-result guards, never segmentation or pixel-quality tests."""
from pathlib import Path
import copy
import unittest
from outcome_test_support import SyntheticOutcomeCase, outcome_fixture, add_requirement

ROOT = Path(__file__).resolve().parents[1]
METHOD = ROOT / 'skills/mockup-image/references/background-removal.md'

class BackgroundRemovalGuidance(unittest.TestCase):
    def test_fidelity_and_capability_boundaries(self):
        text = METHOD.read_text()
        for cue in ('normalized source RGB plane unchanged', 'corresponding coordinates',
                    'not strictly alpha-only', 'fails an exact-preservation task',
                    'respect host tool requirements', 'grants no installation, access or upload permission'):
            with self.subTest(cue=cue): self.assertIn(cue, text)

    def test_mask_and_material_boundaries(self):
        text = METHOD.read_text()
        for cue in ('connected-background flood fill', 'small range of tolerances',
                    'protected subject region', 'enclosed openings separately',
                    'local selection or manual correction', 'does not uniquely determine foreground color and alpha',
                    'Label approximated transparency honestly'):
            with self.subTest(cue=cue): self.assertIn(cue, text)

    def test_export_and_review_boundaries(self):
        text = METHOD.read_text()
        for cue in ('light, dark and contrasting backgrounds', 'actual exported PNG',
                    'checkerboard baked into RGB is not transparency', 'premultiplied/straight-alpha',
                    'Keep the private original intact', 'remaining uncertainty visible'):
            with self.subTest(cue=cue): self.assertIn(cue, text)

class BackgroundRemovalDeclarations(SyntheticOutcomeCase):
    axes = ('subject-detail', 'alpha-edge', 'translucency', 'export-transparency')

    def fixture(self):
        data = outcome_fixture()
        for axis in self.axes:
            add_requirement(data, axis, instruction='Synthetic task requires ' + axis,
                            observation='Declared synthetic result only; no pixels reviewed or processed.')
        return data

    def test_complete_declaration(self): self.assertEqual(self.result(), [])

    def test_each_required_axis_blocks_ready(self):
        original = copy.deepcopy(self.data)
        for axis in self.axes:
            for result in ('fail', 'untested', None):
                self.data = copy.deepcopy(original)
                if result is None: self.remove_check(axis)
                else: self.check_for(axis)['result'] = result
                with self.subTest(axis=axis, result=result): self.assertTrue(self.result())

    def test_honest_rework(self):
        self.data['status'] = 'rework'
        self.check_for('alpha-edge')['result'] = 'fail'
        self.assertEqual(self.result(), [])

    def test_opaque_subject_does_not_require_translucency(self):
        self.data['requirements'] = [r for r in self.data['requirements'] if r['id'] != 'translucency']
        self.remove_check('translucency')
        self.assertEqual(self.result(), [])

if __name__ == '__main__': unittest.main()
