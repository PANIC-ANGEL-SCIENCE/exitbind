"""Written-contract and synthetic declaration guards, not UI/pixel testing."""
from pathlib import Path
import copy
import unittest
from outcome_test_support import SyntheticOutcomeCase, outcome_fixture, add_requirement

ROOT = Path(__file__).resolve().parents[1]
CONTRACTS = {
    'components.md': ('framework-neutral', 'shipped behavior', 'draft design proposal',
                      'controlled/uncontrolled ownership', 'least invasive supported layer',
                      'primitive → semantic → component', 'actual production components',
                      'representative downstream composition'),
    'iconography.md': ('Separate semantic meaning from artwork', 'native use size',
                       'The glyph is not the hit target', 'Classify directionality by meaning',
                       'accessible name', 'license notices', 'not a mandate',
                       'Automated name or mapping checks cannot establish recognizability'),
}

class WrittenContracts(unittest.TestCase):
    def test_guidance_boundaries(self):
        for name, cues in CONTRACTS.items():
            text = (ROOT / 'skills/design-pack/references' / name).read_text()
            for cue in cues:
                with self.subTest(name=name, cue=cue): self.assertIn(cue, text)

    def test_routes_from_existing_production(self):
        for name in ('methods', 'interface', 'screen-production', 'brand-system',
                     'elements', 'interface-craft'):
            text = (ROOT / 'skills/design-pack/references' / (name + '.md')).read_text()
            for target in ('components.md', 'iconography.md'):
                with self.subTest(name=name, target=target): self.assertIn('](' + target + ')', text)

class ComponentIconDeclarations(SyntheticOutcomeCase):
    axes = ('component-behavior', 'icon-meaning', 'icon-optical-fit', 'icon-accessibility')

    def fixture(self):
        data = outcome_fixture()
        for axis in self.axes:
            add_requirement(data, axis, instruction='Synthetic brief requires ' + axis,
                            observation='Synthetic declaration only; no UI or icon inspected.')
        return data

    def test_complete_declared_record(self):
        self.assertEqual(self.result(), [])

    def test_each_required_axis_blocks_ready_independently(self):
        original = copy.deepcopy(self.data)
        for axis in self.axes:
            for result in ('fail', 'untested', None):
                self.data = copy.deepcopy(original)
                if result is None: self.remove_check(axis)
                else: self.check_for(axis)['result'] = result
                with self.subTest(axis=axis, result=result): self.assertTrue(self.result())

    def test_honest_rework_is_valid(self):
        self.data['status'] = 'rework'
        self.check_for('icon-optical-fit')['result'] = 'fail'
        self.assertEqual(self.result(), [])

    def test_task_can_have_no_icon_requirement(self):
        self.data = outcome_fixture()
        self.assertEqual(self.result(), [])

if __name__ == '__main__': unittest.main()
