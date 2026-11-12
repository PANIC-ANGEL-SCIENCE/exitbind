"""Small helpers for synthetic outcome declarations, never visual validation."""
import base64
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from check_outcome import check

# Tiny signature fixture; these tests do not decode or inspect a photograph.
PNG = base64.b64decode('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII=')
HASH = hashlib.sha256(PNG).hexdigest()


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


def add_requirement(data, requirement_id, *, instruction, observation,
                    artifact_id='sample', view_id='sample-view', result='pass'):
    """Append one explicitly scoped requirement and its synthetic declaration."""
    if any(item['id'] == requirement_id for item in data['requirements']):
        raise ValueError('duplicate synthetic requirement: ' + requirement_id)
    data['requirements'].append({
        'id': requirement_id, 'category': 'other', 'instruction': instruction,
        'targets': [{'artifact_id': artifact_id, 'location': requirement_id}]})
    data['checks'].append({
        'requirement_id': requirement_id, 'artifact_id': artifact_id,
        'location': requirement_id, 'result': result,
        'view_ids': [view_id], 'observation': observation})


def check_for(data, requirement_id):
    """Select exactly one check; missing/ambiguous fixtures fail loudly."""
    matches = [item for item in data['checks']
               if item['requirement_id'] == requirement_id]
    if len(matches) != 1:
        raise ValueError(f'expected one check for {requirement_id}, found {len(matches)}')
    return matches[0]


def remove_check(data, requirement_id):
    data['checks'].remove(check_for(data, requirement_id))


class SyntheticOutcomeCase(unittest.TestCase):
    """Temporary artifacts and serialization shared by declaration-only suites."""
    temp_prefix = 'synthetic-outcome-'

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix=self.temp_prefix)
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.data = self.fixture()
        for artifact in self.data['artifacts']:
            (self.root / artifact['path']).write_bytes(PNG)

    def check_for(self, requirement_id):
        return check_for(self.data, requirement_id)

    def remove_check(self, requirement_id):
        remove_check(self.data, requirement_id)

    def result(self):
        (self.root / 'outcome.json').write_text(json.dumps(self.data))
        return check(self.root, 'outcome.json')
