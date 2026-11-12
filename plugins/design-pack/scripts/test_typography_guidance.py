"""Lexical scope guards; these are not typography quality or maker-read tests."""
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]
CONTRACTS = {
    'skills/design-pack/references/typography.md': (
        'required languages', 'actual weight and role size',
        'first equal declared size, then optical compensation',
        'test actual glyph ownership', 'required combining sequences',
        'line-start/end punctuation', 'fallback and loaded phases',
        'no example language is the default', 'English-only is not a quality rule',
    ),
    'skills/design-pack/references/discovery.md': ('required languages and scripts',),
    'skills/design-pack/templates/reference-session.md': ('Actual required languages/scripts/numeral content',),
    'skills/design-pack/references/interface.md': ('composition input methods for supported languages',),
    'EVALUATION.md': ('Required-script and mixed-script typography',),
}

class TypographyGuidance(unittest.TestCase):
    def test_concrete_methods_survive_generalization(self):
        for path, cues in CONTRACTS.items():
            text = (ROOT / path).read_text()
            for cue in cues:
                with self.subTest(path=path, cue=cue):
                    self.assertIn(cue, text)

    def test_private_dependency_is_not_required(self):
        text = (ROOT / 'skills/design-pack/references/discovery.md').read_text()
        self.assertIn('needs no private calibration access to operate', text)

if __name__ == '__main__':
    unittest.main()
