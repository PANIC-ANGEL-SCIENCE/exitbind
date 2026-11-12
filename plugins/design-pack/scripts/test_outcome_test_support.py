"""Regression tests for fixture independence and requirement-ID selection."""
import copy
import unittest

from capture_outcome_fixtures import presentation_fixture
from outcome_test_support import (SyntheticOutcomeCase, add_requirement,
                                  check_for, outcome_fixture, remove_check)


class OutcomeSupportTests(unittest.TestCase):
    def test_factories_return_independent_records(self):
        first = presentation_fixture(group=True)
        second = presentation_fixture(group=True)
        check_for(first, 'people-count')['result'] = 'fail'
        self.assertEqual(check_for(second, 'people-count')['result'], 'pass')

    def test_missing_or_ambiguous_check_refuses_selection(self):
        data = outcome_fixture()
        with self.assertRaisesRegex(ValueError, 'found 0'):
            check_for(data, 'absent')
        data['checks'].append(copy.deepcopy(check_for(data, 'attitude')))
        with self.assertRaisesRegex(ValueError, 'found 2'):
            check_for(data, 'attitude')

    def test_duplicate_requirement_refuses_before_mutation(self):
        data = outcome_fixture()
        before = copy.deepcopy(data)
        with self.assertRaisesRegex(ValueError, 'duplicate'):
            add_requirement(data, 'attitude', instruction='synthetic', observation='synthetic')
        self.assertEqual(data, before)

    def test_remove_by_id_after_reorder_leaves_other_checks_intact(self):
        data = presentation_fixture(group=True)
        data['checks'].reverse()
        expected = [item for item in data['checks'] if item['requirement_id'] != 'people-count']
        remove_check(data, 'people-count')
        self.assertEqual(data['checks'], expected)
        self.assertTrue(any(r['id'] == 'people-count' for r in data['requirements']))


class ReorderedOutcomeTests(SyntheticOutcomeCase):
    fixture = staticmethod(presentation_fixture)

    def test_inserted_requirement_and_reordered_checks_keep_named_failure(self):
        add_requirement(self.data, 'unrelated', instruction='Synthetic extra requirement.',
                        observation='Synthetic declaration only.')
        self.data['checks'].reverse()
        self.check_for('presentation-fit')['result'] = 'fail'
        self.assertEqual(self.check_for('unrelated')['result'], 'pass')
        self.assertIn('unmet scoped requirement blocks readiness', self.result())
        self.data['status'] = 'rework'
        self.assertEqual(self.result(), [])

    def test_reordered_untested_check_is_still_not_ready(self):
        self.data['checks'].reverse()
        self.check_for('personal-detail-fit').update(result='not-tested', view_ids=[])
        self.assertIn('unmet scoped requirement blocks readiness', self.result())


if __name__ == '__main__':
    unittest.main()
