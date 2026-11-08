#!/usr/bin/env python3
"""Static documentation contracts, not model execution or actual age judgments."""
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]
GUIDANCE = ROOT / 'skills/mockup-image/references/model-direction.md'
BRIEF = ROOT / 'skills/mockup-image/templates/image-brief.md'
SCENARIOS = ROOT / 'EVALUATION.md'

# Synthetic cases map to exact written safeguards. Matching prose proves only
# that these passages survive editing; it does not execute the case or see pixels.
CONTRACTS = {
    'teen_casual_16': ('nonsexual casual outfit photo of a 16-year-old is permissible',
                       'age-appropriate ordinary clothing', 'there is no fixed teen phenotype'),
    'styled_selfie_21': ('styled 21-year-old selfie', 'retain intentional fashion, makeup and expression',
                         'Youthful everyday phone photography can still be deliberately styled'),
    'older_subject': ('Older-adult requests retain their intended age and styling',
                      'not one global youth default'),
    'ethnic_adjective_only': ('An ethnic adjective supplies neither age nor a uniform facial structure',
                              'separately from ethnicity, attractiveness, fashion status and image texture'),
    'source_age_unknown': ("If a reference person's age is unknown, leave it unknown",
                           'Pixel review evaluates task fit, not age verification'),
    'sexualized_age_down': ('decline making a 40-year-old subject look 16 while retaining an explicitly sexualized pose/treatment',
                            'offer a clearly adult subject in that treatment or a nonsexual, age-appropriate teen concept'),
    'nonsexual_age_adjustment': ('Nonsexual age-adjustment and teen fashion are not blanket refusal categories',
                                 'Makeup, fashion and deliberate styling are not sexual by themselves'),
    'casting_not_texture': ('An age number in a prompt alone is insufficient',
                            'noise, compression and casual framing do not make a mature-default cast meet a younger brief'),
    'avoid_inverse_stereotype': ('Those features individually or together do not prove age',
                                'Do not make all young people round-cheeked, large-eyed or makeup-free',
                                'Do not use a beauty-filter face, infantile proportions or wrinkle-removal de-aging'),
    'scope_and_privacy': ('A pack update authorizes no image generation or distribution on its own',
                          'preserving protected photos, garments and UI',
                          'private photos, personal preferences, filled evidence and generated examples outside'),
    'teen_label_and_optional_default': ('18–19-year-olds can be adults',
                                       'Never silently replace an explicitly requested nonsexual teen subject with an adult',
                                       'optional validation examples whose subject age is unconstrained'),
}


def missing_contract(text, cues):
    return [cue for cue in cues if cue not in text]


class AgeCastingGuidanceTests(unittest.TestCase):
    def test_all_synthetic_contracts_are_present(self):
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

    def test_brief_separates_intent_from_pixel_review(self):
        text = BRIEF.read_text()
        for cue in ('Requested subject age bracket / life-stage and social context',
                    'source age unknown unless supplied',
                    'Actual-pixel casting/context fit to declared age bracket',
                    'independent of phone texture'):
            self.assertIn(cue, text)

    def test_scenarios_and_limits_are_explicit(self):
        text = SCENARIOS.read_text()
        for cue in ('16-year-old, nonsexual', '21-year-old, deliberately styled',
                    'Older adult portrait', 'Ethnic adjective only',
                    "Source person's age unknown", 'Make 40 look 16',
                    'Nonsexual age adjustment or teen fashion',
                    'not executed image trials'):
            self.assertIn(cue, text)
        validation = (ROOT / 'VALIDATION.md').read_text()
        self.assertIn('not a language-model evaluation', validation)
        self.assertIn('No images were generated for this candidate', validation)


if __name__ == '__main__':
    unittest.main()
