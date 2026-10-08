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
        self.assertEqual(result['exact_operation_routes'], 24)
        self.assertEqual(result['engineering_examples'], 348)
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

    def test_full_cash_quote_and_invoice_examples_cannot_be_substituted(self):
        for identity, other in (('cash_security.quote', 'cash.invoice.long'),
                                ('cash_security.invoice', 'cash.quote.USD.CashflowYield')):
            self.rejects(lambda m: m['operation_routes'][identity].update(positive_examples=[other]))

    def test_actual_government_selection_cannot_be_replaced_by_other_policy(self):
        for identity, other in (('government_spread.linear', 'government.exact'),
                                ('government_spread.exact', 'government.linear')):
            self.rejects(lambda m: m['operation_routes'][identity].update(positive_examples=[other]))

    def test_cash_oracle_is_not_a_supplied_request_or_certificate(self):
        self.rejects(lambda m: m.update(supplemental_corpora=['tests/fixtures/library/cash-reference.json']))
        for identity in ('cash_security.quote', 'cash_security.invoice',
                         'government_spread.linear', 'government_spread.exact'):
            self.rejects(lambda m: m['operation_routes'][identity].update(certified=True))

    def test_helper_mappings_require_the_actual_product_and_subtype(self):
        for kind, other in [('deposit','fra'), ('fra','deposit'), ('irs','ois'), ('ois','irs'),
                            ('basis','irs'), ('rate_future','deposit')]:
            self.rejects(lambda m: m['operation_routes']['rate_helpers.'+kind].update(
                positive_examples=['rate_helpers.'+other+'.positive']))

    def test_helper_limits_and_missing_history_are_actual_examples(self):
        for kind in ('deposit','fra','irs','ois','rate_future','basis'):
            self.assertTrue(self.matrix['operation_routes']['rate_helpers.'+kind]['negative_examples'])
            self.rejects(lambda m: m['operation_routes']['rate_helpers.'+kind].update(
                boundary_examples=['rate_helpers.'+kind+'.missing_curve']))


if __name__ == '__main__': unittest.main()
