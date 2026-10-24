"""Complete declared host requests must retain their original terms and errors."""
import copy
import json
import unittest

from library_host_census import validate_row


class CompleteHostRows(unittest.TestCase):
    def setUp(self):
        self.case = dict(id='full.original', expected_error=None, request=dict(
            schema_version=1, operation=dict(kind='bond', terms=dict(
                face='1000000.0000000000000001', calendar=dict(id='cal', version='v7'),
                profiles=[dict(start='2026-01-01', end='2027-01-01', spread='-0.005')]))))
        self.row = dict(self.case, expected_envelope=dict(ok='true', schema_version=1,
                        result=dict(data=dict(amount='1000000.0000000000000001'))))
        self.row['source_case'] = copy.deepcopy(self.case)
        self.row['request_json'] = json.dumps(self.case['request'])

    def test_complete_original_request_survives(self):
        validate_row(self.case, self.row)

    def test_missing_term_changed_revision_or_cash_units_rejected(self):
        for field, replacement in [('face', 1000000.0),
                                   ('calendar', dict(id='cal', version='v8')),
                                   ('profiles', [])]:
            row = copy.deepcopy(self.row)
            row['request']['operation']['terms'][field] = replacement
            with self.assertRaises(ValueError):
                validate_row(self.case, row)

    def test_error_cannot_be_success_or_another_category(self):
        case = copy.deepcopy(self.case)
        case['expected_error'] = 'missing_input'
        row = copy.deepcopy(self.row)
        row['expected_error'] = 'missing_input'
        row['source_case'] = copy.deepcopy(case)
        for envelope in [dict(ok='true', schema_version=1, result={}),
                         dict(ok='false', schema_version=1, error=dict(code='unsupported'))]:
            row['expected_envelope'] = envelope
            with self.assertRaises(ValueError):
                validate_row(case, row)
        row['expected_envelope'] = dict(ok='false', schema_version=1, error=dict(code='missing_input'))
        validate_row(case, row)

    def test_original_positive_expected_payload_and_omission_survive(self):
        case = copy.deepcopy(self.case)
        case.pop('expected_error')
        case['expected'] = dict(currency='USD', amount='1.0000000000000001')
        row = copy.deepcopy(self.row)
        row['source_case'] = copy.deepcopy(case)
        validate_row(case, row)
        row['source_case']['expected']['amount'] = 1.0
        with self.assertRaises(ValueError):
            validate_row(case, row)


if __name__ == '__main__':
    unittest.main()
