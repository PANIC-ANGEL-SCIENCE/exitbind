#!/usr/bin/env python3
"""Written capture contracts and synthetic declarations, never image scoring."""
from pathlib import Path
import unittest

from outcome_test_support import PNG, SyntheticOutcomeCase, add_requirement
from capture_outcome_fixtures import (
    capture_fixture,
    scene_reset_fixture,
    supporting_artifact_fixture,
    fashion_fidelity_fixture,
    material_continuity_fixture,
    framing_mode_fixture,
    presentation_fixture,
    subject_continuity_fixture,
    outfit_intent_fixture,
    request_context_fixture
)


ROOT = Path(__file__).resolve().parents[1]
GUIDANCE = ROOT / 'skills/mockup-image/references/capture-styling.md'

# Exact text deletion guards. Presence is not semantic or visual compliance.
CONTRACTS = {
    'actual_evidence': (
        'Inspect actual source pixels',
        'Separate recurring relationships from exceptions and missing coverage',
        'Do not infer device, focal length, exposure settings or spontaneous behavior'),
    'frame_not_label': (
        "camera's height, distance and angle",
        'subject scale and position',
        'foreground obstruction, mirror/phone involvement',
        'Diagnose the combined relationship, not a single forbidden feature',
        'An explicit catalogue brief can require those same choices'),
    'no_damage_recipe': (
        'do not insert clutter, random tilt or accidental blur as authenticity tokens',
        'hard flash, crushed blacks and mixed color are not mandatory phone-photo cues',
        'Do not fake poor quality with grain, haze, underexposure or blown highlights'),
    'lighting_evidence': (
        'Read light independently from room style, wardrobe and color palette',
        'face-to-background brightness',
        'near-to-far falloff',
        'a bright face alone does not prove flash',
        'Do not invent camera metadata or exact color temperature'),
    'lighting_transfer': (
        'Separate person/outfit references from spatial references by source role',
        "the new subject's actual position",
        'do not paste an evenly lit person onto a darker room',
        'Choose one coherent lighting situation from the reference variation',
        'Soft window light, clean daylight and even exposure remain valid when supported',
        'Assess lighting fit separately from texture'),
    'worn_relationships': (
        'relative upper/lower volume and length',
        'layer lengths and openings, closures, sleeve and hem bunching',
        'Transfer a few supported relationships, not a shopping list or a brand label',
        "Do not convert one user's board into universal dark clothing",
        'If garments, construction or identity are fixed, preserve them',
        'generated clothing remains illustrative'),
    'separate_actual_review': (
        'compare framing/moment, lighting, styling/wear, expression and capture texture separately',
        'Recheck every receiving crop',
        'keep it in rework even when face, outfit and texture pass',
        'Explicit commercial staging, bright clean rooms and simple outfits remain valid'),
    'trace_limits': (
        'Retain actual guidance reads, submitted call/input records, native outputs and their review',
        'Distinguish manual use of local guidance from host activation',
        'A package version, written receipt or structural test cannot prove'),
}


class CaptureStylingGuidanceTests(unittest.TestCase):
    def test_written_contracts_present(self):
        text = GUIDANCE.read_text()
        for name, cues in CONTRACTS.items():
            with self.subTest(contract=name):
                self.assertEqual([cue for cue in cues if cue not in text], [])

    def test_removing_each_cue_is_detected(self):
        text = GUIDANCE.read_text()
        for name, cues in CONTRACTS.items():
            for cue in cues:
                with self.subTest(contract=name, cue=cue):
                    mutated = text.replace(cue, '')
                    self.assertNotIn(cue, mutated)
                    self.assertEqual([item for item in cues if item not in mutated], [cue])

    def test_routes_are_direct(self):
        for path, route in {
            'skills/mockup-image/SKILL.md': 'references/capture-styling.md',
            'skills/mockup-image/references/model-direction.md': 'capture-styling.md',
            'skills/mockup-image/references/image-production.md': 'capture-styling.md',
            'skills/mockup-image/references/generation-contract.md': 'capture-styling.md',
        }.items():
            with self.subTest(path=path):
                self.assertIn('](' + route + ')', (ROOT / path).read_text())

    def test_brief_keeps_observation_and_protection(self):
        text = (ROOT / 'skills/mockup-image/templates/image-brief.md').read_text()
        for cue in ('Framing/moment:', 'Lighting evidence by source role:',
                    'Clothing as worn:', 'fixed garments and authorized changes',
                    'Separate framing, lighting and styling findings against actual pixels'):
            self.assertIn(cue, text)

    def test_scenarios_include_counterexamples(self):
        text = (ROOT / 'EVALUATION.md').read_text()
        for cue in ('Phone label with catalogue composition',
                    'Person evenly lit against a shaded room',
                    'Dark outfit copied without its proportions or wear',
                    'Explicit commercial or bright clean scene brief',
                    'Fixed garments with a new camera direction',
                    'Unknown source device and light provenance'):
            self.assertIn(cue, text)

    def test_limits_are_explicit(self):
        text = (ROOT / 'VALIDATION.md').read_text()
        for cue in ('not camera, lighting or fashion classifiers',
                    'cannot authenticate guidance reads or generator calls',
                    'No live generation is run by these tests'):
            self.assertIn(cue, text)


class IndependentCaptureStylingOutcomes(SyntheticOutcomeCase):
    fixture = staticmethod(capture_fixture)
    temp_prefix = 'capture-styling-contract-'


    def test_complete_declared_record_passes_without_visual_claim(self):
        self.assertEqual(self.result(), [])

    def test_framing_failure_not_overridden_by_other_passes(self):
        self.check_for('framing-fit').update(result='fail')
        self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_lighting_failure_not_overridden_by_texture_pass(self):
        self.check_for('lighting-fit').update(result='fail')
        self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_styling_failure_not_overridden_by_face_pass(self):
        self.check_for('styling-fit').update(result='fail')
        self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_unreviewed_axes_block_ready(self):
        for requirement_id in ('framing-fit', 'lighting-fit', 'styling-fit'):
            with self.subTest(requirement_id=requirement_id):
                self.data = capture_fixture()
                self.check_for(requirement_id).update(result='not-tested', view_ids=[])
                self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_missing_declared_check_is_rejected(self):
        self.remove_check('styling-fit')
        self.assertTrue(self.result())

    def test_honest_rework_is_valid(self):
        self.data['status'] = 'rework'
        self.check_for('lighting-fit').update(result='fail')
        self.assertEqual(self.result(), [])

    def test_commercial_clean_or_simple_is_not_globally_rejected(self):
        for declaration in ('explicit catalogue composition fits the task',
                            'bright clean room lighting fits the task',
                            'simple fixed outfit fits the task'):
            with self.subTest(declaration=declaration):
                self.check_for('styling-fit')['observation'] = 'Synthetic declared ' + declaration + '.'
                self.assertEqual(self.result(), [])

    def test_frozen_failure_is_rework_not_edit_authority(self):
        self.data.update(mode='frozen-trial', status='rework')
        self.check_for('framing-fit').update(result='fail')
        self.assertEqual(self.result(), [])


# These guards protect written workflow boundaries, not aesthetic behavior.
SCENE_CONTRACTS = {
    'skills/design-pack/references/reference-roles.md': (
        'inspect the actual current sections and relevant source pixels',
        'A section title suggests a role',
        'not as a mandatory ingredient list',
        'An isolated garment does not establish a complete outfit',
        'Synthesize a coherent scene across roles',
        'private taste and completed plans remain project-local'),
    'skills/mockup-image/references/image-production.md': (
        'An explicit whole reset reopens the named axes and cancels their old preservation locks',
        'Do not discard unrelated current constraints',
        'Check both the actual image arguments and inherited prompt text',
        'Construct one situated moment',
        'Let each choice constrain the others',
        'develop a provisional fictional person rather than a fashion inventory',
        'occupation does not determine style',
        'never a fact inferred about a real reference person',
        'hidden contents should not be claimed as visible evidence',
        'do not add accessories, clutter, branded tokens or an itemized bag display',
        'A lived space need not be cluttered, shabby or domestic',
        'Changed labels, hair color, accessories or wall color alone do not demonstrate',
        'A pass on anatomy, texture or reflection cannot close a failed central relationship',
        'reopen the coupled premise when the same mismatch persists',
        'Do not reroll an unchanged prompt',
        "Reviewer readiness is not the user's OK",
        'Preserve any explicit frozen-trial boundary'),
    'skills/mockup-image/references/generation-contract.md': (
        'inspect both submitted image arguments and inherited prompt text',
        'A prior output may be inspected for comparison without becoming a provider input',
        'do not disguise another descendant edit as a fresh start',
        'A declared reset or version label does not prove that old conditioning was removed'),
}


class SceneResetGuidanceTests(unittest.TestCase):
    def test_written_scene_contracts_present(self):
        for path, cues in SCENE_CONTRACTS.items():
            text = (ROOT / path).read_text()
            for cue in cues:
                with self.subTest(path=path, cue=cue):
                    self.assertIn(cue, text)

    def test_each_removed_scene_contract_is_detected(self):
        for path, cues in SCENE_CONTRACTS.items():
            text = (ROOT / path).read_text()
            for cue in cues:
                with self.subTest(path=path, cue=cue):
                    mutated = text.replace(cue, '')
                    self.assertEqual([item for item in cues if item not in mutated], [cue])

    def test_brief_and_routes_cover_reset_decisions(self):
        brief = (ROOT / 'skills/mockup-image/templates/image-brief.md').read_text()
        for cue in ('Sectioned reference coverage:', 'Repair or whole reset:',
                    'Coupled scene premise:', 'Fictional person when useful:',
                    'Reset scope and whole-scene fit:',
                    'reviewer readiness separately from user acceptance'):
            self.assertIn(cue, brief)
        for relative in ('SKILL.md', 'references/model-direction.md',
                         'references/capture-styling.md', 'references/generation-contract.md'):
            with self.subTest(path=relative):
                self.assertIn('image-production.md)',
                              (ROOT / 'skills/mockup-image' / relative).read_text())

    def test_scenarios_cover_repair_reset_and_counterexamples(self):
        text = (ROOT / 'EVALUATION.md').read_text()
        for cue in ('User reopens person, outfit and space together',
                    'Narrow expression repair with fixed garments',
                    'Board reorganized into sections',
                    'Attractive face, outfit and room do not belong together',
                    'Isolated item or face section lacks context',
                    'Same central mismatch across revisions',
                    'Reset one axis while another remains fixed',
                    'Fictional person requested rather than a clothing inventory',
                    'Reviewer ready but user has not accepted'):
            self.assertIn(cue, text)

    def test_scene_test_limits_are_explicit(self):
        text = (ROOT / 'VALIDATION.md').read_text()
        for cue in ('not identity comparison, outfit recognition, room classification or scene-coherence scoring',
                    'They cannot inspect hidden prompt conditioning',
                    'No new photograph is generated or reviewed by these tests',
                    "A reviewer or tool gate cannot supply the user's OK"):
            self.assertIn(cue, text)


class IndependentSceneResetOutcomes(SyntheticOutcomeCase):
    fixture = staticmethod(scene_reset_fixture)
    temp_prefix = 'scene-reset-contract-'


    def test_complete_declared_reset_passes_without_visual_claim(self):
        self.assertEqual(self.result(), [])

    def test_each_failed_reset_axis_blocks_ready(self):
        for requirement_id in ('reset-subject', 'reset-outfit', 'reset-space'):
            with self.subTest(requirement_id=requirement_id):
                self.data = scene_reset_fixture()
                self.check_for(requirement_id).update(result='fail')
                self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_component_passes_do_not_override_scene_failure(self):
        self.check_for('scene-coherence').update(result='fail')
        self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_each_untested_scene_axis_blocks_ready(self):
        for requirement_id in ('reset-subject', 'reset-outfit', 'reset-space', 'scene-coherence'):
            with self.subTest(requirement_id=requirement_id):
                self.data = scene_reset_fixture()
                self.check_for(requirement_id).update(result='not-tested', view_ids=[])
                self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_missing_scene_check_is_rejected(self):
        self.remove_check('scene-coherence')
        self.assertTrue(self.result())

    def test_honest_scene_rework_is_valid(self):
        self.data.update(status='rework')
        self.check_for('scene-coherence').update(result='fail')
        self.assertEqual(self.result(), [])

    def test_retained_explicit_constants_are_not_globally_rejected(self):
        for declaration in ('current mirror capture preserved in new scene',
                            'current adult life-stage and restrained expression preserved',
                            'a narrow repair preserves the fixed outfit'):
            with self.subTest(declaration=declaration):
                self.check_for('scene-coherence')['observation'] = 'Synthetic declared ' + declaration + '.'
                self.assertEqual(self.result(), [])

    def test_frozen_scene_failure_is_not_repair_authority(self):
        self.data.update(mode='frozen-trial', status='rework')
        self.check_for('scene-coherence').update(result='fail')
        self.assertEqual(self.result(), [])


SUPPORT_CONTRACTS = {
    'skills/mockup-image/references/model-direction.md': (
        'task-specific casting and presentation feedback, not a demographic law',
        'inspect actual references without assigning their subjects an age',
        'not the required appearance of every age, gender or nationality',
        'do not replace it with childlike proportions or smoothing alone'),
    'skills/mockup-image/references/image-production.md': (
        'make an inspectable supporting artifact before relying on it in the scene',
        'A fictional backstory and a prompt are not that artifact',
        'does not require a complete invented archive for every prop',
        'Do not retroactively assign a newly invented design',
        'Retain the full design and the particular crop/side',
        'a mockup does not prove a physical object was printed',
        'A filename or detailed description is not submitted image evidence',
        'Review artifact existence, scene depiction and actual legibility separately',
        'enlargement cannot recover missing letters',
        'A separate correct master does not excuse a failed required depiction'),
    'skills/mockup-image/references/generation-contract.md': (
        'Map the exact current asset to the actual supported image argument',
        'Master legibility, submitted conditioning and legibility in the scene are independent facts',
        'A generated approximation is not exact reproduction'),
    'skills/mockup-image/references/capture-styling.md': (
        'reopen relevant current source images',
        'light reaching the scene, the surfaces reflecting it, and the recorded tonal/color response',
        'Relative brightness visible in the image is not measured illuminance',
        'Do not invent lux, ISO, shutter, aperture or sensor characteristics from pixels',
        'visible grain does not establish high ISO',
        'a clean low-light picture does not prove studio lighting',
        'Do not manufacture a sensitivity value or apply uniform fake grain',
        'Test light arrangement, exposure fit, capture response and required evidence independently'),
}


class SupportingArtifactAndLightGuidanceTests(unittest.TestCase):
    def test_written_support_and_light_contracts_present(self):
        for path, cues in SUPPORT_CONTRACTS.items():
            text = (ROOT / path).read_text()
            for cue in cues:
                with self.subTest(path=path, cue=cue):
                    self.assertIn(cue, text)

    def test_each_removed_support_or_light_contract_is_detected(self):
        for path, cues in SUPPORT_CONTRACTS.items():
            text = (ROOT / path).read_text()
            for cue in cues:
                with self.subTest(path=path, cue=cue):
                    mutated = text.replace(cue, '')
                    self.assertEqual([item for item in cues if item not in mutated], [cue])

    def test_routes_and_brief_cover_actual_asset_and_response(self):
        brief = (ROOT / 'skills/mockup-image/templates/image-brief.md').read_text()
        for cue in ('Consequential supporting artifact:', 'Supporting-artifact transfer:',
                    'Light/exposure/capture response:', 'Supporting artifact review:',
                    'Independent light arrangement, exposure fit and capture-response findings'):
            self.assertIn(cue, brief)
        for relative in ('SKILL.md', 'references/generation-contract.md'):
            text = (ROOT / 'skills/mockup-image' / relative).read_text()
            self.assertIn('supporting-artifact method', text)
            self.assertIn('image-production.md)', text)

    def test_counterexamples_and_claim_limits(self):
        scenarios = (ROOT / 'EVALUATION.md').read_text()
        for cue in ('User asks to explain and show a designed object',
                    'Existing ambiguous fragment precedes the new design',
                    'Companion master is correct but required depiction fails',
                    'Incidental props or intentionally unreadable fragment',
                    'Dark material beside a bright surface',
                    'Dim scene with clean detail or mixed local color'):
            self.assertIn(cue, scenarios)
        limits = (ROOT / 'VALIDATION.md').read_text()
        for cue in ('They do not create a print', 'recognize text, estimate illuminance or recover camera metadata',
                    'No-text graphics, coherent unreadable small fragments and clean low-light pictures'):
            self.assertIn(cue, limits)


class IndependentSupportingArtifactOutcomes(SyntheticOutcomeCase):
    fixture = staticmethod(supporting_artifact_fixture)
    temp_prefix = 'support-light-contract-'


    def test_complete_declared_support_record_is_valid_without_visual_claim(self):
        self.assertEqual(self.result(), [])

    def test_each_failed_axis_blocks_ready_independently(self):
        for requirement_id in ('artifact-master', 'depiction-fit', 'legibility-fit', 'exposure-fit', 'capture-response'):
            with self.subTest(requirement_id=requirement_id):
                self.data = supporting_artifact_fixture()
                self.check_for(requirement_id).update(result='fail')
                self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_untested_axis_blocks_ready(self):
        for requirement_id in ('artifact-master', 'depiction-fit', 'legibility-fit', 'exposure-fit', 'capture-response'):
            with self.subTest(requirement_id=requirement_id):
                self.data = supporting_artifact_fixture()
                self.check_for(requirement_id).update(result='not-tested', view_ids=[])
                self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_missing_depiction_check_is_rejected(self):
        self.remove_check('depiction-fit')
        self.assertTrue(self.result())

    def test_missing_bound_master_file_is_rejected(self):
        (self.root / 'supporting-master.png').unlink()
        self.assertTrue(self.result())

    def test_changed_master_bytes_are_rejected(self):
        (self.root / 'supporting-master.png').write_bytes(PNG + b'changed')
        self.assertTrue(self.result())

    def test_honest_depiction_rework_is_valid(self):
        self.data.update(status='rework')
        self.check_for('depiction-fit').update(result='fail')
        self.assertEqual(self.result(), [])

    def test_brief_permitted_graphics_and_clean_response_are_not_globally_rejected(self):
        for observation in ('abstract graphic without text meets brief',
                            'small coherent fragment needs no readable words',
                            'clean low-light capture fits the evidence'):
            with self.subTest(observation=observation):
                self.check_for('legibility-fit')['observation'] = 'Synthetic declared ' + observation + '.'
                self.assertEqual(self.result(), [])

    def test_frozen_depiction_failure_remains_rework_without_edit_permission(self):
        self.data.update(mode='frozen-trial', status='rework')
        self.check_for('depiction-fit').update(result='fail')
        self.assertEqual(self.result(), [])


FIDELITY_CONTRACTS = {
    'skills/mockup-image/references/capture-styling.md': (
        'Color, a broad garment category and a fit adjective are not enough',
        'Do not infer exact fiber, price, brand authenticity or unseen pattern construction',
        'Preserve those relationships through the actual prompt and returned image',
        'Those attributes do not determine clothing taste',
        'Fix the failed construction/material relationship without deleting the distinctive silhouette',
        'Sparse detail is not automatically better or more expensive',
        'Decoration can be intentional',
        'posing, layer length and framing do not hide the very feature',
        'keep reference fidelity in rework even when the image is clean, young-looking or technically plausible'),
    'skills/mockup-image/references/model-direction.md': (
        'do not infer orientation or gender identity from appearance',
        'reopen the casting premise instead of protecting the same rejected face',
        'without assigning source people an age',
        'If identity is fixed, disclose the correction boundary',
        'a fashion improvement cannot close an unresolved casting requirement'),
    'skills/mockup-image/references/generation-contract.md': (
        'A detailed research note not carried into production is not applied direction',
        'neither prompt wording nor attached inputs proves faithful transfer'),
    'skills/design-pack/references/reference-roles.md': (
        'defining visual relationships from incidental objects and broad category labels',
        'shared palette does not establish fidelity',
        'revisit it when a repair accidentally normalizes the result'),
}


class ReferenceFashionFidelityGuidanceTests(unittest.TestCase):
    def test_written_fidelity_contracts_present(self):
        for path, cues in FIDELITY_CONTRACTS.items():
            text = (ROOT / path).read_text()
            for cue in cues:
                with self.subTest(path=path, cue=cue):
                    self.assertIn(cue, text)

    def test_each_removed_fidelity_contract_is_detected(self):
        for path, cues in FIDELITY_CONTRACTS.items():
            text = (ROOT / path).read_text()
            for cue in cues:
                with self.subTest(path=path, cue=cue):
                    self.assertEqual([item for item in cues if item not in text.replace(cue, '')], [cue])

    def test_fidelity_brief_and_routes_are_present(self):
        brief = (ROOT / 'skills/mockup-image/templates/image-brief.md').read_text()
        for cue in ('Defining fashion relationships:', 'Local repair risk:',
                    'Reference fidelity:', 'Repeated casting correction:'):
            self.assertIn(cue, brief)
        for path in ('SKILL.md', 'references/model-direction.md',
                     'references/generation-contract.md', 'references/image-production.md'):
            self.assertIn('capture-styling.md)', (ROOT / 'skills/mockup-image' / path).read_text())

    def test_fidelity_scenarios_and_limits_are_explicit(self):
        text = (ROOT / 'EVALUATION.md').read_text()
        for cue in ('Distinctive references reduced to conventional basics',
                    'Cheap-looking or arbitrary detail feedback',
                    'New fictional person or changed presentation',
                    'Required waist or hem feature hidden',
                    'Simple outfit is actually requested or referenced',
                    'Repeated rejected apparent adult life stage'):
            self.assertIn(cue, text)
        text = (ROOT / 'VALIDATION.md').read_text()
        for cue in ('Exact text deletion guards are not semantic comprehension tests',
                    'do not compare real outfits, identify brands, price garments',
                    'cannot detect a false observation or an omitted true requirement'):
            self.assertIn(cue, text)


class IndependentReferenceFashionOutcomes(SyntheticOutcomeCase):
    fixture = staticmethod(fashion_fidelity_fixture)
    temp_prefix = 'reference-fashion-contract-'


    def test_complete_declared_fidelity_record_passes_without_visual_claim(self):
        self.assertEqual(self.result(), [])

    def test_each_failed_fashion_axis_blocks_ready_despite_casting_pass(self):
        for requirement_id in ('reference-fashion-fit', 'construction-fit', 'garment-visibility'):
            with self.subTest(requirement_id=requirement_id):
                self.data = fashion_fidelity_fixture()
                self.check_for(requirement_id).update(result='fail')
                self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_fashion_pass_does_not_override_casting_failure(self):
        self.check_for('age-fit').update(result='fail')
        self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_untested_fashion_axes_block_ready(self):
        for requirement_id in ('reference-fashion-fit', 'construction-fit', 'garment-visibility'):
            with self.subTest(requirement_id=requirement_id):
                self.data = fashion_fidelity_fixture()
                self.check_for(requirement_id).update(result='not-tested', view_ids=[])
                self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_missing_visibility_check_is_rejected(self):
        self.remove_check('garment-visibility')
        self.assertTrue(self.result())

    def test_honest_fashion_failure_is_valid_rework(self):
        self.data.update(status='rework')
        self.check_for('reference-fashion-fit').update(result='fail')
        self.assertEqual(self.result(), [])

    def test_authorized_simple_or_decorative_choices_are_not_globally_rejected(self):
        for observation in ('simple outfit matches simple references',
                            'user explicitly asked for simpler styling',
                            'decorative trim is intentional and visually coherent'):
            with self.subTest(observation=observation):
                self.check_for('reference-fashion-fit')['observation'] = 'Synthetic declared ' + observation + '.'
                self.assertEqual(self.result(), [])


MATERIAL_CONTRACTS = {
    'skills/mockup-image/references/capture-styling.md': (
        'diagnose visible behavior before adding texture',
        'Uniform grain, repeated wrinkles, global sharpening or extra gloss cannot establish fabric weight',
        'These observations do not verify actual fiber, mass or manufacturability',
        'Trace the selected garment from its support points to its finish',
        'rather than requiring every seam to run the whole length',
        'the failure is lost structural/material logic, not unequal decoration density',
        'A cropped product image cannot establish its unseen hem or back',
        'Do not describe invented stitching, lining, fasteners or unseen construction as observed evidence',
        'only describe or verify detail the actual output resolves',
        'Do not trade a believable hem for a redesigned outfit or a new face',
        'Review material response and whole-garment continuity independently'),
    'skills/mockup-image/references/image-production.md': (
        'protect the newly accepted regions even if an earlier attempt had reopened them',
        'a requested detail repair is not a new casting brief'),
    'skills/mockup-image/references/generation-contract.md': (
        'Distinguish observed source detail from an authorized illustrative design choice',
        'inspect the repaired region, its transition and the full image'),
}


class MaterialContinuityGuidanceTests(unittest.TestCase):
    def test_written_material_contracts_present(self):
        for path, cues in MATERIAL_CONTRACTS.items():
            text = (ROOT / path).read_text()
            for cue in cues:
                with self.subTest(path=path, cue=cue):
                    self.assertIn(cue, text)

    def test_each_removed_material_contract_is_detected(self):
        for path, cues in MATERIAL_CONTRACTS.items():
            text = (ROOT / path).read_text()
            for cue in cues:
                with self.subTest(path=path, cue=cue):
                    self.assertEqual([item for item in cues if item not in text.replace(cue, '')], [cue])

    def test_material_brief_and_routes_cover_protection(self):
        text = (ROOT / 'skills/mockup-image/templates/image-brief.md').read_text()
        for cue in ('Material/finish correction:', 'Repair protection:',
                    'Material and whole-garment continuity:'):
            self.assertIn(cue, text)
        for path in ('SKILL.md', 'references/image-production.md', 'references/generation-contract.md'):
            self.assertIn('capture-styling.md)', (ROOT / 'skills/mockup-image' / path).read_text())

    def test_material_scenarios_and_limits_are_explicit(self):
        text = (ROOT / 'EVALUATION.md').read_text()
        for cue in ('Garment surface reads molded or weightless',
                    'Strong hip design loses structure at the hem',
                    'Reference crops out the finish', 'Quiet lower leg is intentional',
                    'Hem meets footwear asymmetrically',
                    'User accepts the person and general outfit',
                    'Surface passes but construction fails'):
            self.assertIn(cue, text)
        text = (ROOT / 'VALIDATION.md').read_text()
        for cue in ('not cloth simulation, sewing inspection, material identification or pixel-level preservation tests',
                    'do not prove that a generated garment is manufacturable or that hidden stitches exist'):
            self.assertIn(cue, text)


class IndependentMaterialContinuityOutcomes(SyntheticOutcomeCase):
    fixture = staticmethod(material_continuity_fixture)
    temp_prefix = 'material-continuity-contract-'


    def test_complete_declared_material_record_passes_without_visual_claim(self):
        self.assertEqual(self.result(), [])

    def test_material_and_continuity_failures_block_ready_independently(self):
        for requirement_id in ('material-response', 'garment-continuity'):
            with self.subTest(requirement_id=requirement_id):
                self.data = material_continuity_fixture()
                self.check_for(requirement_id).update(result='fail')
                self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_repair_pass_does_not_override_protected_region_failure(self):
        self.check_for('protected-region').update(result='fail')
        self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_untested_material_axes_block_ready(self):
        for requirement_id in ('material-response', 'garment-continuity', 'protected-region'):
            with self.subTest(requirement_id=requirement_id):
                self.data = material_continuity_fixture()
                self.check_for(requirement_id).update(result='not-tested', view_ids=[])
                self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_missing_protection_check_is_rejected(self):
        self.remove_check('protected-region')
        self.assertTrue(self.result())

    def test_honest_continuity_failure_is_valid_rework(self):
        self.data.update(status='rework')
        self.check_for('garment-continuity').update(result='fail')
        self.assertEqual(self.result(), [])

    def test_quiet_finish_or_asymmetric_contact_is_not_globally_rejected(self):
        for observation in ('quiet unadorned hem is coherent with the brief',
                            'asymmetric shoe contact follows the pose',
                            'panel seam ends intentionally before the hem'):
            with self.subTest(observation=observation):
                self.check_for('garment-continuity')['observation'] = 'Synthetic declared ' + observation + '.'
                self.assertEqual(self.result(), [])

    def test_frozen_continuity_failure_is_not_edit_authority(self):
        self.data.update(mode='frozen-trial', status='rework')
        self.check_for('garment-continuity').update(result='fail')
        self.assertEqual(self.result(), [])


FRAMING_MODE_CONTRACTS = {
    'skills/mockup-image/references/capture-styling.md': (
        'Choose the subject and framing mode from the current task and actual references',
        'a board thumbnail, browser viewport crop or blocked image edge does not prove',
        'an obscured face is not an excluded head',
        'clothing emphasis does not prove a selfie',
        'do not automatically widen it to add a head',
        'Do not invent an off-frame face, gaze, exact age or identity assessment',
        'Do not insert a second phone, face or reflection merely to prove the word selfie',
        'no face, full person, hand or elaborate room is compulsory',
        'A printed portrait can be the content of an object',
        'A face intentionally outside an outfit-only brief is not a casting failure',
        'hiding a face cannot solve an unresolved portrait requirement'),
    'skills/mockup-image/references/model-direction.md': (
        'Apply this method to the visible people and casting decisions the task actually requires',
        'Do not add a face, person or biography merely to satisfy model-direction fields',
        'Do not use that exception to hide a face the current brief requires'),
    'skills/mockup-image/references/generation-contract.md': (
        'Identify required visible evidence and intentional exclusions',
        'Do not carry an old portrait, face requirement or full-body default',
        'do not crop away a currently protected face or object to evade a failed requirement'),
    'skills/design-pack/references/reference-roles.md': (
        'a partial interface view does not establish the source crop',
        'An object can be the main content rather than a prop'),
}


class SubjectFramingModeGuidanceTests(unittest.TestCase):
    def test_written_framing_mode_contracts_present(self):
        for path, cues in FRAMING_MODE_CONTRACTS.items():
            text = (ROOT / path).read_text()
            for cue in cues:
                with self.subTest(path=path, cue=cue):
                    self.assertIn(cue, text)

    def test_each_removed_mode_contract_is_detected(self):
        for path, cues in FRAMING_MODE_CONTRACTS.items():
            text = (ROOT / path).read_text()
            for cue in cues:
                with self.subTest(path=path, cue=cue):
                    self.assertEqual([item for item in cues if item not in text.replace(cue, '')], [cue])

    def test_mode_brief_and_conditional_entrypoint(self):
        brief = (ROOT / 'skills/mockup-image/templates/image-brief.md').read_text()
        for cue in ('Visual subject / framing mode:', 'Source-frame evidence:',
                    'Mode/crop review:', 'Face, expression and casting fields apply only'):
            self.assertIn(cue, brief)
        entry = (ROOT / 'skills/mockup-image/SKILL.md').read_text()
        self.assertIn('Choose the requested visual subject before directing a face', entry)
        self.assertIn('not every phone-camera or object image', entry)
        self.assertIn('capture-styling.md)', entry)

    def test_mode_scenarios_and_nonvisual_limits(self):
        scenarios = (ROOT / 'EVALUATION.md').read_text()
        for cue in ('Outfit-led image intentionally excludes the face',
                    'Downward wearer view rather than mirror selfie',
                    'Object-only isolated or contextual photograph',
                    'Source appears headless only in a browser viewport',
                    "Printed portrait is the object's content",
                    'Current portrait or item detail is required',
                    'Narrow repair of an accepted composition'):
            self.assertIn(cue, scenarios)
        limits = (ROOT / 'VALIDATION.md').read_text()
        for cue in ('not image classification, crop detection, selfie authentication or object recognition',
                    'without irrelevant face/age fields',
                    'no new image was generated by these checks'):
            self.assertIn(cue, limits)


class IndependentSubjectFramingOutcomes(SyntheticOutcomeCase):
    fixture = staticmethod(framing_mode_fixture)
    temp_prefix = 'subject-framing-contract-'


    def test_varied_declared_modes_pass_without_visual_claim(self):
        for mode in ('portrait', 'outfit-led', 'wearer-view', 'object-only'):
            with self.subTest(mode=mode):
                self.data = framing_mode_fixture(mode)
                self.assertEqual(self.result(), [])

    def test_object_and_outfit_modes_need_no_unrequested_face_requirements(self):
        for mode in ('object-only', 'outfit-led'):
            with self.subTest(mode=mode):
                self.data = framing_mode_fixture(mode)
                self.assertNotIn('age-fit', [x['id'] for x in self.data['requirements']])
                self.assertEqual(self.result(), [])

    def test_wrong_declared_mode_blocks_ready(self):
        self.check_for('subject-mode').update(result='fail', observation='Synthetic unrequested portrait replaces object-only frame.')
        self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_hidden_required_portrait_content_blocks_ready(self):
        self.data = framing_mode_fixture('portrait')
        self.check_for('content-evidence').update(result='fail', observation='Synthetic required face evidence absent.')
        self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_untested_mode_or_content_blocks_ready(self):
        for requirement_id in ('subject-mode', 'framing-evidence', 'content-evidence'):
            with self.subTest(requirement_id=requirement_id):
                self.data = framing_mode_fixture()
                self.check_for(requirement_id).update(result='not-tested', view_ids=[])
                self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_missing_content_check_is_rejected(self):
        self.remove_check('content-evidence')
        self.assertTrue(self.result())

    def test_honest_framing_failure_is_valid_rework(self):
        self.data.update(status='rework')
        self.check_for('framing-evidence').update(result='fail')
        self.assertEqual(self.result(), [])

    def test_frozen_framing_failure_is_not_edit_authority(self):
        self.data.update(mode='frozen-trial', status='rework')
        self.check_for('subject-mode').update(result='fail')
        self.assertEqual(self.result(), [])


POSE_PRESENTATION_CONTRACTS = {
    'skills/mockup-image/references/model-direction.md': (
        'Note differences, overlap and counterexamples',
        'Section names are organizational labels, not verified gender identity',
        'keep user-specified gender identity, chosen gender expression, sexual orientation',
        'Do not assign assertiveness to men, softness to women or ambiguity to nonbinary people',
        'Keep expression intensity independent of gender expression',
        'Do not flatten all characters into the same neutral pose',
        'change a meaningful action/body/camera relationship',
        'not whether it looks like a gender label'),
    'skills/mockup-image/references/capture-styling.md': (
        'front, profile, three-quarter, back, partial-body and deliberately obscured-face views',
        'not a permanent front-facing mirror pose',
        'Do not describe unseen eyes, mouth or expression as verified',
        'do not widen a valid back or outfit-led view to satisfy irrelevant facial checks',
        'two people already form a multi-person composition',
        'not proof that the pictured people are friends, partners or relatives',
        'which arm belongs to whom',
        'instead of counting a reflection as an extra person',
        'Review count, individual distinction, interaction/contact and required visibility separately'),
    'skills/mockup-image/references/image-production.md': (
        'fictional interview-ready person',
        'form/material/placement',
        'do not force trauma, occupation or cultural membership as an explanation',
        'No tattoo, piercing, jewelry or accessory is compulsory',
        'Do not claim a standalone master exists when only a written specification exists',
        'Separate fictional interview facts from source evidence and rendered evidence'),
    'skills/mockup-image/references/generation-contract.md': (
        'Do not substitute a gender or orientation label for those directions',
        'A request for one new draft does not authorize a multi-view batch',
        'do not mark unseen details or an imagined interview as visually verified',
        'state count, individual roles and the shared action/spacing'),
}


class PosePresentationGuidanceTests(unittest.TestCase):
    def test_written_pose_presentation_contracts_present(self):
        for path, cues in POSE_PRESENTATION_CONTRACTS.items():
            text = (ROOT / path).read_text()
            for cue in cues:
                with self.subTest(path=path, cue=cue):
                    self.assertIn(cue, text)

    def test_each_removed_presentation_contract_is_detected(self):
        for path, cues in POSE_PRESENTATION_CONTRACTS.items():
            text = (ROOT / path).read_text()
            for cue in cues:
                with self.subTest(path=path, cue=cue):
                    self.assertEqual([item for item in cues if item not in text.replace(cue, '')], [cue])

    def test_routes_brief_and_source_label_limits(self):
        entry = (ROOT / 'skills/mockup-image/SKILL.md').read_text()
        for cue in ('Identity labels do not prescribe a pose',
                    'references/model-direction.md)', 'references/image-production.md)'):
            self.assertIn(cue, entry)
        brief = (ROOT / 'skills/mockup-image/templates/image-brief.md').read_text()
        for cue in ('Fictional identity supplied by the brief', 'Cross-group pose/expression evidence:',
                    'New pose/view intention:', 'Fictional personal details when requested:',
                    'Pose/self-presentation review:', 'If multi-person:'):
            self.assertIn(cue, brief)
        roles = (ROOT / 'skills/design-pack/references/reference-roles.md').read_text()
        self.assertIn('no section provides a fixed gender-to-pose rule', roles)

    def test_scenarios_include_counterexamples_and_test_limits(self):
        scenarios = (ROOT / 'EVALUATION.md').read_text()
        for cue in ('Gendered reference groups with varied examples',
                    'Intentional back, profile or obscured-face view',
                    'No tattoo or a simple decorative preference',
                    'Two people sharing a scene or a larger requested group',
                    'One requested new draft'):
            self.assertIn(cue, scenarios)
        limits = (ROOT / 'VALIDATION.md').read_text()
        for cue in ('not gender classification, pose recognition, story verification or tattoo detection',
                    'no image generation runs as part of them',
                    'A dishonest pass or omitted true requirement can still pass'):
            self.assertIn(cue, limits)


class IndependentPosePresentationOutcomes(SyntheticOutcomeCase):
    fixture = staticmethod(presentation_fixture)
    temp_prefix = 'pose-presentation-contract-'


    def test_complete_declared_presentation_passes_without_visual_claim(self):
        self.assertEqual(self.result(), [])

    def test_repeated_pose_failure_not_overridden_by_new_person(self):
        self.check_for('pose-distinction').update(result='fail')
        self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_presentation_failure_not_overridden_by_identity_label(self):
        self.check_for('presentation-fit').update(result='fail')
        self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_required_personal_detail_failure_not_rescued_by_story(self):
        self.check_for('personal-detail-fit').update(result='fail')
        self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_untested_presentation_requirements_block_ready(self):
        for requirement_id in ('pose-distinction', 'presentation-fit', 'personal-detail-fit'):
            with self.subTest(requirement_id=requirement_id):
                self.data = presentation_fixture()
                self.check_for(requirement_id).update(result='not-tested', view_ids=[])
                self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_missing_personal_detail_check_is_rejected(self):
        self.remove_check('personal-detail-fit')
        self.assertTrue(self.result())

    def test_back_view_no_tattoo_or_shared_gesture_not_globally_rejected(self):
        for observation in ('back view intentionally omits facial expression',
                            'character chooses no tattoo',
                            'same gesture serves two differently presented characters'):
            with self.subTest(observation=observation):
                self.check_for('personal-detail-fit')['observation'] = 'Synthetic declared ' + observation + '.'
                self.assertEqual(self.result(), [])

    def test_honest_pose_failure_is_valid_rework(self):
        self.data.update(status='rework')
        self.check_for('pose-distinction').update(result='fail')
        self.assertEqual(self.result(), [])

    def test_group_axes_fail_independently(self):
        for requirement_id in ('people-count', 'individual-distinction', 'contact-coherence'):
            with self.subTest(requirement_id=requirement_id):
                self.data = presentation_fixture(group=True)
                self.check_for(requirement_id).update(result='fail')
                self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_two_or_more_declared_people_not_rejected_by_minimum_three(self):
        for count in (2, 3, 5):
            with self.subTest(count=count):
                self.data = presentation_fixture(group=True)
                self.check_for('people-count')['observation'] = 'Synthetic declared requested count ' + str(count) + '.'
                self.assertEqual(self.result(), [])

    def test_missing_group_contact_check_is_rejected(self):
        self.data = presentation_fixture(group=True)
        self.remove_check('contact-coherence')
        self.assertTrue(self.result())

    def test_frozen_pose_failure_is_rework_not_edit_authority(self):
        self.data.update(mode='frozen-trial', status='rework')
        self.check_for('pose-distinction').update(result='fail')
        self.assertEqual(self.result(), [])


SUBJECT_CONTINUITY_CONTRACTS = {
    'skills/mockup-image/references/model-direction.md': (
        'subject-continuity and casting-fit feedback, not evidence of gender identity',
        'If no structural defect can be established',
        'apparent head size including hair',
        'the jaw/nape-to-neck-to-shoulder transition',
        'Do not impose fixed head-to-body ratios',
        'Check capture continuity separately from casting and geometry',
        'Do not flatten all skin to one brightness',
        'Choose a bounded correction to the combined relationship',
        'Revealing the entire face or neck is not a universal repair',
        'adding more hair, darkness or crop cannot close the reported mismatch',
        'reference-led body direction',
        'Separate these observations from assumptions about sex, training, health or strength',
        'Do not sharpen every tendon, exaggerate veins',
        'A dark eye area alone does not prove liner',
        'Makeup is neither required for every woman nor restricted to women',
        'Do not use a compulsory feminine makeover to repair disconnected anatomy'),
    'skills/mockup-image/references/capture-styling.md': (
        'Read perspective, hair/clothing volume and pose-related occlusion',
        'without equalizing naturally shaded and lit skin',
        'a front-facing replacement or extra blur is not evidence of continuity',
        'Ink should not float as a flat sticker',
        'Avoid floating rings, fused jewelry, duplicate chains',
        'do not multiply fingers or enlarge jewelry to display it',
        'Do not average varied references into default black/grey',
        'Monochrome remains valid when requested or specifically supported',
        'Apply color changes only to the authorized clothing/accessory scope'),
    'skills/mockup-image/references/image-production.md': (
        'return to the exact current image',
        'Do not reinterpret a physical/perceptual correction as a request to change gender identity'),
    'skills/mockup-image/references/generation-contract.md': (
        'bounded connected region and protected scene requirements',
        'instead of a gendered feature list',
        'Preserve intentional pose and face visibility',
        'compare transition and whole figure',
        'update the change/preserve boundary instead of carrying a stale absolute lock',
        'Review tattoo/attachment and palette fidelity independently'),
}


class SubjectContinuityGuidanceTests(unittest.TestCase):
    def test_written_subject_continuity_contracts_present(self):
        for path, cues in SUBJECT_CONTINUITY_CONTRACTS.items():
            text = (ROOT / path).read_text()
            for cue in cues:
                with self.subTest(path=path, cue=cue):
                    self.assertIn(cue, text)

    def test_each_removed_subject_continuity_contract_is_detected(self):
        for path, cues in SUBJECT_CONTINUITY_CONTRACTS.items():
            text = (ROOT / path).read_text()
            for cue in cues:
                with self.subTest(path=path, cue=cue):
                    self.assertEqual([item for item in cues if item not in text.replace(cue, '')], [cue])

    def test_brief_and_entrypoint_expose_bounded_continuity_route(self):
        brief = (ROOT / 'skills/mockup-image/templates/image-brief.md').read_text()
        for cue in ('Subject-continuity diagnosis when needed:', 'Continuity repair boundary:',
                    'Subject-continuity review:'):
            self.assertIn(cue, brief)
        entry = (ROOT / 'skills/mockup-image/SKILL.md').read_text()
        self.assertIn('head-neck-shoulder-body continuity', entry)
        self.assertIn('references/model-direction.md)', entry)

    def test_scenarios_and_nonvisual_limits_remain_explicit(self):
        scenarios = (ROOT / 'EVALUATION.md').read_text()
        for cue in ('Large hair mass or near-camera head',
                    'Supporting shoulder raised in a side/back pose',
                    'Face shaded by hair, shoulder lit by the doorway',
                    'No established structural defect but perceptual fit rejected'):
            self.assertIn(cue, scenarios)
        limits = (ROOT / 'VALIDATION.md').read_text()
        self.assertIn('not anatomical measurements, gender classification, face/body matching', limits)
        self.assertIn('No live image generation runs in these tests', limits)

    def test_body_personal_detail_palette_and_makeup_brief_fields(self):
        brief = (ROOT / 'skills/mockup-image/templates/image-brief.md').read_text()
        for cue in ('Body-detail evidence:', 'Tattoo/piercing/accessory transfer:',
                    'Outfit palette when reopened:', 'Makeup when requested:', 'Body/detail/color review:'):
            self.assertIn(cue, brief)

    def test_body_detail_and_color_scenarios_have_counterexamples(self):
        scenarios = (ROOT / 'EVALUATION.md').read_text()
        for cue in ('Body structure under a specific support or motion',
                    'Original tattoo on a turning arm', 'Varied piercing/accessory choices',
                    'Broader outfit color combinations requested', 'Makeup detail requested for a character',
                    'Small nails, rings or partially hidden makeup'):
            self.assertIn(cue, scenarios)


class IndependentSubjectContinuityOutcomes(SyntheticOutcomeCase):
    fixture = staticmethod(subject_continuity_fixture)
    temp_prefix = 'subject-continuity-contract-'


    def test_complete_declared_subject_record_passes_without_visual_claim(self):
        self.assertEqual(self.result(), [])

    def test_subject_failure_not_overridden_by_clothing_or_identity_label(self):
        self.check_for('subject-continuity').update(result='fail')
        self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_capture_failure_not_overridden_by_geometry_pass(self):
        self.check_for('capture-continuity').update(result='fail')
        self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_preserved_region_failure_not_overridden_by_face_repair(self):
        self.check_for('protected-regions').update(result='fail')
        self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_untested_continuity_axes_block_ready(self):
        for requirement_id in ('subject-continuity', 'capture-continuity', 'protected-regions'):
            with self.subTest(requirement_id=requirement_id):
                self.data = subject_continuity_fixture()
                self.check_for(requirement_id).update(result='not-tested', view_ids=[])
                self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_missing_protection_check_is_rejected(self):
        self.remove_check('protected-regions')
        self.assertTrue(self.result())

    def test_legitimate_shadow_support_or_occlusion_not_globally_rejected(self):
        for observation in ('face remains shaded while shoulder is lit',
                            'supporting shoulder remains raised',
                            'intentional partial profile remains hidden'):
            with self.subTest(observation=observation):
                self.check_for('capture-continuity')['observation'] = 'Synthetic declared ' + observation + '.'
                self.assertEqual(self.result(), [])

    def test_honest_subject_failure_is_valid_rework(self):
        self.data.update(status='rework')
        self.check_for('subject-continuity').update(result='fail')
        self.assertEqual(self.result(), [])

    def test_frozen_subject_failure_is_rework_not_edit_authority(self):
        self.data.update(mode='frozen-trial', status='rework')
        self.check_for('subject-continuity').update(result='fail')
        self.assertEqual(self.result(), [])

    def test_optional_detail_requirements_fail_independently(self):
        for axis in ('body-load-fit', 'tattoo-fit', 'accessory-fit', 'palette-fit', 'makeup-fit'):
            with self.subTest(axis=axis):
                self.data = subject_continuity_fixture()
                add_requirement(self.data, axis,
                                instruction='Synthetic explicitly requested ' + axis + '.',
                                observation='Synthetic declared failure; no visual inspection.',
                                result='fail')
                self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_unrequested_adornments_or_makeup_are_not_required_fields(self):
        ids = [r['id'] for r in self.data['requirements']]
        for axis in ('tattoo-fit', 'accessory-fit', 'makeup-fit'):
            self.assertNotIn(axis, ids)
        self.assertEqual(self.result(), [])

    def test_task_requested_monochrome_or_no_makeup_not_globally_rejected(self):
        for observation in ('monochrome is specifically requested',
                            'character chooses no makeup or piercing',
                            'natural relaxed limb does not show defined muscle'):
            with self.subTest(observation=observation):
                self.check_for('capture-continuity')['observation'] = 'Synthetic declared ' + observation + '.'
                self.assertEqual(self.result(), [])


OUTFIT_INTENT_CONTRACTS = {
    'skills/mockup-image/references/capture-styling.md': (
        'inspect the exact selected image and current output rather than reducing the source to a palette',
        'what is visibly observed, what the user explicitly specifies',
        'A user-specified material or intended mood is a task constraint',
        'Treat color, material, silhouette and styling context as mutually dependent choices',
        'matching the color cannot establish that the intended outfit survived',
        'design authorized missing regions consistently',
        'A mood is not permission to add a standard bundle of symbols',
        'Keep outfit-intent fit in rework when the key relationship disappears',
        'Resolve the coupled outfit intent above before selecting a palette'),
    'skills/mockup-image/references/image-production.md': (
        'reopen the fictional premise as well as the individual object',
        'Do not force every possession to match one theme',
        'it is no longer automatically protected',
        'instead of inventing an elaborate after-the-fact explanation',
        'retire its old master from that role',
        'Keep character coherence and mark transfer separate'),
    'skills/design-pack/references/reference-roles.md': (
        'user-supplied interpretation and inferred associations',
        'instead of assigning the image only a color-swatch role',
        'does not become a reusable cultural costume or palette preset'),
    'skills/mockup-image/references/generation-contract.md': (
        'not just the palette or a mood word',
        'explicitly reopen rejected garments and personal details',
        'do not silently carry the old mark or retrospectively assign it a new identity',
        'A palette pass or plausible story cannot override a failed required relationship'),
}


class CoupledOutfitIntentGuidanceTests(unittest.TestCase):
    def test_written_outfit_intent_contracts_present(self):
        for path, cues in OUTFIT_INTENT_CONTRACTS.items():
            text = (ROOT / path).read_text()
            for cue in cues:
                with self.subTest(path=path, cue=cue):
                    self.assertIn(cue, text)

    def test_each_removed_outfit_intent_contract_is_detected(self):
        for path, cues in OUTFIT_INTENT_CONTRACTS.items():
            text = (ROOT / path).read_text()
            for cue in cues:
                with self.subTest(path=path, cue=cue):
                    self.assertEqual([item for item in cues if item not in text.replace(cue, '')], [cue])

    def test_entrypoint_and_brief_expose_coupled_relationship(self):
        entry = (ROOT / 'skills/mockup-image/SKILL.md').read_text()
        self.assertIn('before choosing colors', entry)
        self.assertIn('references/capture-styling.md)', entry)
        brief = (ROOT / 'skills/mockup-image/templates/image-brief.md').read_text()
        for cue in ('Coupled outfit intent:', 'Current personal-detail coherence:',
                    'Outfit-intent review:', 'Character/detail coherence review:'):
            self.assertIn(cue, brief)

    def test_scenarios_and_nonvisual_limits(self):
        scenarios = (ROOT / 'EVALUATION.md').read_text()
        for cue in ('Same palette but different materials and silhouette',
                    'Source crop omits the lower outfit',
                    'User reopens a conflicting tattoo or personal story',
                    'Intentional contrast or accumulated personal taste'):
            self.assertIn(cue, scenarios)
        limits = (ROOT / 'VALIDATION.md').read_text()
        self.assertIn('not fashion classification, cultural-authenticity scoring or biography verification', limits)
        self.assertIn('no live generation runs in these tests', limits)


class IndependentOutfitIntentOutcomes(SyntheticOutcomeCase):
    fixture = staticmethod(outfit_intent_fixture)
    temp_prefix = 'outfit-intent-contract-'


    def test_complete_declared_intent_passes_without_visual_claim(self):
        self.assertEqual(self.result(), [])

    def test_intent_failure_not_overridden_by_palette_pass(self):
        self.check_for('outfit-intent').update(result='fail')
        self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_personal_detail_conflict_not_overridden_by_outfit_pass(self):
        self.check_for('personal-detail-coherence').update(result='fail')
        self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_old_graphic_failure_not_overridden_by_new_story(self):
        self.check_for('graphic-transfer').update(result='fail')
        self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_protected_subject_failure_not_overridden_by_style_pass(self):
        self.check_for('protected-regions').update(result='fail')
        self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_untested_coupled_requirements_block_ready(self):
        for requirement_id in ('outfit-intent', 'personal-detail-coherence', 'graphic-transfer', 'protected-regions'):
            with self.subTest(requirement_id=requirement_id):
                self.data = outfit_intent_fixture()
                self.check_for(requirement_id).update(result='not-tested', view_ids=[])
                self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_missing_graphic_check_is_rejected(self):
        self.remove_check('graphic-transfer')
        self.assertTrue(self.result())

    def test_intentional_contrast_or_authorized_material_change_not_globally_rejected(self):
        for observation in ('authorized material change keeps the revised intent',
                            'older personal mark intentionally contrasts with current outfit',
                            'simple garments express the chosen relationship'):
            with self.subTest(observation=observation):
                self.check_for('personal-detail-coherence')['observation'] = 'Synthetic declared ' + observation + '.'
                self.assertEqual(self.result(), [])

    def test_honest_intent_failure_is_valid_rework(self):
        self.data.update(status='rework')
        self.check_for('outfit-intent').update(result='fail')
        self.assertEqual(self.result(), [])

    def test_frozen_intent_failure_is_rework_not_edit_authority(self):
        self.data.update(mode='frozen-trial', status='rework')
        self.check_for('outfit-intent').update(result='fail')
        self.assertEqual(self.result(), [])


REQUEST_CONTEXT_CONTRACTS = {
    'skills/design-pack/references/briefing.md': (
        'reconcile the accessible relevant user requests with the exact current artifact',
        "Do not rely only on the latest short instruction or an assistant's previous summary",
        'Separate current requirements from superseded choices, task-local examples',
        'do not mark an unmet active requirement not applicable just to finish',
        'guidance present in the pack, guidance actually read by this maker',
        'do not silently collapse it into decoration or promote every prop to an item',
        'do not claim an exhaustive audit when sources are missing',
        'rather than substituting a root-only summary for the reusable methods'),
    'skills/design-pack/references/memory.md': (
        'Prefer an explicit task instruction, then an explicit project voice',
        "then the user's configured default for relevant work",
        'A deliberate neutral choice is a real choice, not a missing value',
        'Keep the concrete style guide, examples and project-specific phrases outside this package',
        'A profile path, installation or resolver result is not proof',
        'does not authorize sending, publishing or changing protected copy'),
    'skills/mockup-image/SKILL.md': (
        'first reconcile the current [brief]',
        'supported modes are not a checklist of everything to include',
        'record actual consumption rather than assuming it from a version label'),
}


class RequestContextGuidanceTests(unittest.TestCase):
    def test_written_request_context_contracts_present(self):
        for path, cues in REQUEST_CONTEXT_CONTRACTS.items():
            text = (ROOT / path).read_text()
            for cue in cues:
                with self.subTest(path=path, cue=cue):
                    self.assertIn(cue, text)

    def test_each_removed_context_contract_is_detected(self):
        for path, cues in REQUEST_CONTEXT_CONTRACTS.items():
            text = (ROOT / path).read_text()
            for cue in cues:
                with self.subTest(path=path, cue=cue):
                    self.assertEqual([item for item in cues if item not in text.replace(cue, '')], [cue])

    def test_profile_briefs_and_entrypoint_route_current_context(self):
        entry = (ROOT / 'skills/design-pack/SKILL.md').read_text()
        self.assertIn('references/briefing.md)', entry)
        self.assertIn('references/memory.md)', entry)
        profile = (ROOT / 'skills/design-pack/templates/project-profile.md').read_text()
        self.assertIn('Writing voice / configured user default / project override', profile)
        self.assertIn('Product content entities and selection/detail relationships', profile)
        brief = (ROOT / 'skills/design-pack/templates/brief.md').read_text()
        self.assertIn('Accumulated-request coverage:', brief)
        self.assertIn('Selected writing style and precedence:', brief)
        image_brief = (ROOT / 'skills/mockup-image/templates/image-brief.md').read_text()
        self.assertIn('Requirement coverage review:', image_brief)
        self.assertIn('Product-defined item meaning when relevant:', image_brief)

    def test_scenarios_include_unknown_history_and_style_counterexamples(self):
        scenarios = (ROOT / 'EVALUATION.md').read_text()
        for cue in ('Long revision history with later corrections',
                    'Several supported photo modes, one requested draft',
                    'User writing default with a current task override',
                    'Explicit neutral voice or no configured preference',
                    'Required private profile is inaccessible',
                    'Some historical messages cannot be retrieved'):
            self.assertIn(cue, scenarios)
        limits = (ROOT / 'VALIDATION.md').read_text()
        self.assertIn('do not retrieve full conversations, execute a profile resolver', limits)
        self.assertIn('A false pass or omitted real requirement can pass', limits)


class IndependentRequestContextOutcomes(SyntheticOutcomeCase):
    fixture = staticmethod(request_context_fixture)
    temp_prefix = 'request-context-contract-'


    def test_complete_declared_context_passes_without_runtime_claim(self):
        self.assertEqual(self.result(), [])

    def test_each_failed_active_requirement_blocks_ready(self):
        for requirement_id in ('request-coverage', 'style-fit', 'item-semantics', 'preserved-requirements'):
            with self.subTest(requirement_id=requirement_id):
                self.data = request_context_fixture()
                self.check_for(requirement_id).update(result='fail')
                self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_untested_required_profile_or_item_behavior_blocks_ready(self):
        for requirement_id in ('style-fit', 'item-semantics'):
            with self.subTest(requirement_id=requirement_id):
                self.data = request_context_fixture()
                self.check_for(requirement_id).update(result='not-tested', view_ids=[])
                self.assertIn('unmet scoped requirement blocks readiness', self.result())

    def test_missing_active_coverage_check_is_rejected(self):
        self.remove_check('request-coverage')
        self.assertTrue(self.result())

    def test_photo_only_fixture_does_not_require_copy_or_app_checks(self):
        self.data = framing_mode_fixture('photo-only')
        ids = [x['id'] for x in self.data['requirements']]
        self.assertNotIn('style-fit', ids)
        self.assertNotIn('item-semantics', ids)
        self.assertEqual(self.result(), [])

    def test_explicit_neutral_or_task_override_not_globally_rejected(self):
        for observation in ('task explicitly selects neutral voice',
                            'task overrides the user default',
                            'optional group capability is not part of this photo'):
            with self.subTest(observation=observation):
                self.check_for('style-fit')['observation'] = 'Synthetic declared ' + observation + '.'
                self.assertEqual(self.result(), [])

    def test_honest_incomplete_context_is_valid_rework(self):
        self.data.update(status='rework')
        self.check_for('request-coverage').update(result='not-tested', view_ids=[])
        self.assertEqual(self.result(), [])

    def test_frozen_context_failure_is_rework_not_edit_authority(self):
        self.data.update(mode='frozen-trial', status='rework')
        self.check_for('request-coverage').update(result='fail')
        self.assertEqual(self.result(), [])

if __name__ == '__main__':
    unittest.main()
