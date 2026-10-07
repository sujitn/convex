"""Exact supplied operation-map controls; passing metadata awards no certificate."""
import copy
import json
from pathlib import Path
import unittest
import api

ROOT = Path(__file__).resolve().parents[2]


class OperationRoutes(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.matrix = json.loads((ROOT / 'tools/library/api-capabilities.json').read_bytes())

    def rejects(self, mutate):
        candidate = copy.deepcopy(self.matrix); mutate(candidate)
        with self.assertRaises((ValueError, KeyError)): api.check(candidate)

    def test_exact_requests_and_all_original_mandatory_records_remain(self):
        result = api.check(self.matrix)
        self.assertEqual(result['mandatory_records'], 2154)
        self.assertEqual(result['exact_operation_routes'], 14)
        self.assertEqual(result['engineering_examples'], 220)
        self.assertFalse(result['certified'])

    def test_missing_or_unsafe_supplemental_corpus_rejected(self):
        for value in ([], ['tests/fixtures/library/operations.json'] * 2,
                      ['../../secrets.json'], ['/tmp/corpus.json']):
            self.rejects(lambda m: m.update(supplemental_corpora=value))

    def test_operation_needs_actual_boundary_positive_and_failure_examples(self):
        for kind in ('positive_examples', 'boundary_examples', 'negative_examples'):
            self.rejects(lambda m: m['operation_routes']['curve_time.date_to_tenor'].update({kind: []}))
        self.rejects(lambda m: m['operation_routes']['curve_time.date_to_tenor'].update(negative_examples=['conversion.negative_time']))
        self.rejects(lambda m: m['operation_routes']['curve_time.date_to_tenor'].update(boundary_examples=['time.before_reference']))

    def test_operation_selector_cannot_be_replaced_with_family_label(self):
        for selectors in ({}, {'operation.kind': 'bond'}, {'operation.action.kind': 'tenor_to_date'},
                          {'operation.missing': 'date_to_tenor'}):
            self.rejects(lambda m: m['operation_routes']['curve_time.date_to_tenor'].update(selectors=selectors))

    def test_operation_anchors_record_scope_and_certificates_are_enforced(self):
        for field, value in (('anchors', []), ('required_record_ids', ['LC-023/invented/rust']),
                             ('certified', True), ('qualification', '')):
            self.rejects(lambda m: m['operation_routes']['curve_time.date_to_tenor'].update({field: value}))
        self.rejects(lambda m: m['operation_routes']['curve_time.date_to_tenor']['anchors'][0].update(text='invented function'))


if __name__ == '__main__': unittest.main()
