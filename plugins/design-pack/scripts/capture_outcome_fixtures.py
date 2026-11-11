"""Explicit synthetic scenario composition; not image or real-world evidence."""
from outcome_test_support import add_requirement, outcome_fixture


def capture_fixture():
    """Only declaration logic; the PNG signature does not depict a photograph."""
    data = outcome_fixture()
    for axis in ('framing-fit', 'lighting-fit', 'styling-fit'):
        add_requirement(data, axis, instruction='Synthetic brief requires ' + axis + '.',
            observation='Synthetic declared ' + axis + '; no visual judgment performed.')
    return data


def scene_reset_fixture():
    data = capture_fixture()
    for axis in ('reset-subject', 'reset-outfit', 'reset-space', 'scene-coherence'):
        add_requirement(data, axis, instruction='Synthetic scoped requirement: ' + axis + '.',
            observation='Synthetic declared ' + axis + '; no image comparison performed.')
    return data


def supporting_artifact_fixture():
    # Two synthetic signature-only files exercise binding, not artwork perception.
    data = capture_fixture()
    sample_hash = data['artifacts'][0]['sha256']
    data['artifacts'].append({'id': 'support', 'kind': 'raster',
                              'path': 'supporting-master.png', 'sha256': sample_hash,
                              'produced_seq': 3})
    data['views'].append({'id': 'support-view', 'artifact_id': 'support',
                          'sha256': sample_hash, 'seq': 4, 'evidence_type': 'image-view',
                          'view_tool_result_id': 'synthetic-not-executed', 'scale': 'use'})
    for axis in ('artifact-master', 'depiction-fit', 'legibility-fit',
                 'exposure-fit', 'capture-response'):
        target = 'support' if axis == 'artifact-master' else 'sample'
        add_requirement(data, axis, instruction='Synthetic scoped requirement: ' + axis + '.',
            observation='Synthetic declared ' + axis + '; no image perception performed.',
            artifact_id=target, view_id='support-view' if target == 'support' else 'sample-view')
    return data


def fashion_fidelity_fixture():
    data = capture_fixture()
    for axis in ('reference-fashion-fit', 'construction-fit', 'garment-visibility'):
        add_requirement(data, axis, instruction='Synthetic scoped requirement: ' + axis + '.',
            observation='Synthetic declared ' + axis + '; no reference comparison performed.')
    return data


def material_continuity_fixture():
    data = capture_fixture()
    for axis in ('material-response', 'garment-continuity', 'protected-region'):
        add_requirement(data, axis, instruction='Synthetic scoped requirement: ' + axis + '.',
            observation='Synthetic declared ' + axis + '; no material or pixel inspection performed.')
    return data


def framing_mode_fixture(mode='object-only'):
    # The schema does not impose portrait-specific requirements on every record.
    data = outcome_fixture()
    data['requirements'] = []
    data['checks'] = []
    data['contract_source'] = 'Synthetic ' + mode + ' brief; no actual image classification.'
    for axis in ('subject-mode', 'framing-evidence', 'content-evidence'):
        add_requirement(data, axis, instruction='Synthetic ' + mode + ' requirement: ' + axis + '.',
            observation='Synthetic declared ' + axis + '; no source or output inspection performed.')
    return data


def presentation_fixture(group=False):
    # Generic optional declarations: no new image classifier or identity field.
    data = framing_mode_fixture('task-selected person view')
    axes = ['pose-distinction', 'presentation-fit', 'personal-detail-fit']
    if group:
        axes += ['people-count', 'individual-distinction', 'contact-coherence']
    for axis in axes:
        add_requirement(data, axis, instruction='Synthetic scoped ' + axis + '.',
            observation='Synthetic declared ' + axis + '; no visual or identity verification.')
    return data


def subject_continuity_fixture():
    data = framing_mode_fixture('bounded subject repair')
    for axis in ('subject-continuity', 'capture-continuity', 'protected-regions'):
        add_requirement(data, axis, instruction='Synthetic scoped ' + axis + '.',
            observation='Synthetic declared ' + axis + '; no actual image inspection.')
    return data


def outfit_intent_fixture():
    data = framing_mode_fixture('outfit-intent repair')
    for axis in ('outfit-intent', 'personal-detail-coherence', 'graphic-transfer', 'protected-regions'):
        add_requirement(data, axis, instruction='Synthetic scoped ' + axis + '.',
            observation='Synthetic declared ' + axis + '; no actual visual or cultural judgment.')
    return data


def request_context_fixture():
    data = framing_mode_fixture('scoped project-context task')
    for axis in ('request-coverage', 'style-fit', 'item-semantics', 'preserved-requirements'):
        add_requirement(data, axis, instruction='Synthetic explicitly scoped ' + axis + '.',
            observation='Synthetic declared ' + axis + '; no real history, style or application check.')
    return data
