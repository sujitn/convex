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
        # Preserve all 77 original routes/4975 requests; add three full Treasury quote axes.
        self.assertEqual(result['exact_operation_routes'], 80)
        self.assertEqual(result['engineering_examples'], 4975 + 400 + 21 + 21 + 27 + 27 + 34 + 26 + 69 + 37 + 49 + 110 + 90 + 183)
        self.assertEqual({name for name in self.matrix['operation_routes'] if name.startswith('icma_risk.')},
                         {'icma_risk.yield_sensitivity', 'icma_risk.hedge_vector'})
        self.assertFalse(result['certified'])

    def test_optional_parameter_routes_retain_exact_native_actions_and_failures(self):
        for action, other in [('model_scenario', 'optional_graph.direct.volatility.risk.h'),
                              ('model_risk', 'optional_graph.direct.volatility.scenario.1')]:
            route = 'optional_bond.' + action
            self.rejects(lambda m: m['operation_routes'][route].update(positive_examples=[other]))
            self.rejects(lambda m: m['operation_routes'][route].update(certified=True))
        self.rejects(lambda m: m['operation_routes']['optional_bond.model_risk'].update(
            negative_examples=['optional_graph.direct.volatility.risk.h']))

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

    def test_government_inverse_preserves_selection_operation_and_failure_scope(self):
        for identity, other in [('government_spread_price.exact','gov_inverse.linear.SemiAnnual'),
                                ('government_spread_price.linear','gov_inverse.exact')]:
            self.rejects(lambda m: m['operation_routes'][identity].update(positive_examples=[other]))
            self.rejects(lambda m: m['operation_routes'][identity].update(certified=True))
            self.rejects(lambda m: m['operation_routes'][identity].update(selectors={'operation.kind':'government_spread'}))
        self.rejects(lambda m: m['operation_routes']['government_spread_price.linear'].update(
            negative_examples=['gov_inverse.linear.SemiAnnual']))

    def test_complete_hedge_actions_cannot_substitute_or_certify(self):
        for name,other in [('aggregate','hedge.mixed'),('solve','hedge.aggregate')]:
            self.rejects(lambda m: m['operation_routes']['hedge.'+name].update(positive_examples=[other]))
            self.rejects(lambda m: m['operation_routes']['hedge.'+name].update(certified=True))
        self.rejects(lambda m: m['operation_routes']['hedge.solve'].update(negative_examples=['hedge.mixed']))

    def test_icma_price_and_inverse_cannot_drop_context_or_substitute_actions(self):
        for name, other in [('price', 'icma.regular.SemiAnnual.0.04125.inverse'),
                            ('yield', 'icma.regular.SemiAnnual.0.04125')]:
            self.rejects(lambda m: m['operation_routes']['icma.'+name].update(positive_examples=[other]))
            self.rejects(lambda m: m['operation_routes']['icma.'+name].update(certified=True))
        self.rejects(lambda m: m['operation_routes']['icma.price'].update(
            selectors={'operation.kind': 'workout', 'operation.action.kind': 'price'}))

    def test_contextual_government_routes_preserve_full_sources_and_actions(self):
        for name, other in [('spread', 'icma_gov.regular.ExactMaturity.price'),
                            ('price', 'icma_gov.regular.ExactMaturity.spread')]:
            self.rejects(lambda m: m['operation_routes']['icma_government.'+name].update(positive_examples=[other]))
            self.rejects(lambda m: m['operation_routes']['icma_government.'+name].update(certified=True))
        self.rejects(lambda m: m['operation_routes']['icma_government.price'].update(
            negative_examples=['icma_gov.regular.LinearMaturity.price']))

    def test_floating_workout_actions_preserve_original_paths_and_failures(self):
        for name, other in [('price', 'floating.term.dm'),
                            ('discount_margin', 'floating.term.price'),
                            ('paths', 'floating.term.dm')]:
            identity = 'floating_workout.' + name
            self.rejects(lambda m: m['operation_routes'][identity].update(positive_examples=[other]))
            self.rejects(lambda m: m['operation_routes'][identity].update(certified=True))
        self.rejects(lambda m: m['operation_routes']['floating_workout.paths'].update(
            boundary_examples=['floating.overnight.paths']))
        self.rejects(lambda m: m['operation_routes']['floating_workout.discount_margin'].update(
            negative_examples=['floating.term.dm']))
        self.rejects(lambda m: m['operation_routes']['floating_workout.price'].update(
            selectors={'operation.kind': 'workout', 'operation.action.kind': 'price'}))

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

    def test_credit_helper_mapping_requires_actual_full_terms_and_diagnostics(self):
        self.rejects(lambda m: m['operation_routes']['credit_helpers.reprice'].update(
            positive_examples=['rate_helpers.deposit.positive']))
        self.rejects(lambda m: m['operation_routes']['credit_helpers.reprice'].update(
            boundary_examples=['credit_helpers.helper_limit']))
        self.rejects(lambda m: m['operation_routes']['credit_helpers.reprice'].update(certified=True))

    def test_cash_helper_models_cannot_substitute_or_award_acceptance(self):
        for family, other in [('government','repo'), ('repo','government'),
                               ('reference_basis','credit_basis'), ('credit_basis','reference_basis')]:
            self.rejects(lambda m: m['operation_routes']['cash_helpers.'+family].update(
                positive_examples=['cash_helpers.'+other+'.positive']))
            self.rejects(lambda m: m['operation_routes']['cash_helpers.'+family].update(
                boundary_examples=['cash_helpers.'+family+'.missing_curve']))
            self.rejects(lambda m: m['operation_routes']['cash_helpers.'+family].update(certified=True))

    def test_workout_actions_rights_and_failure_examples_cannot_substitute(self):
        for name, other in [('price', 'workout.yield.SemiAnnual.0.04125.exercise.0'),
                            ('yield_call', 'workout.yield.SemiAnnual.0.04125.exercise.1'),
                            ('yield_put', 'workout.yield.SemiAnnual.0.04125.exercise.0'),
                            ('worst', 'workout.yield.SemiAnnual.0.04125.exercise.0')]:
            self.rejects(lambda m: m['operation_routes']['workout.'+name].update(
                positive_examples=[other]))
            self.rejects(lambda m: m['operation_routes']['workout.'+name].update(certified=True))
        self.rejects(lambda m: m['operation_routes']['workout.worst'].update(
            boundary_examples=['workout.failure.unselected_put_no_root']))

    def test_curve_family_and_fitted_models_cannot_substitute_or_certify(self):
        for family, other in [('nelson_siegel', 'sv'), ('svensson', 'ns')]:
            self.rejects(lambda m: m['operation_routes']['curve_family.'+family].update(
                positive_examples=['curve.model.observations.'+other+'.positive']))
            self.rejects(lambda m: m['operation_routes']['parametric_fit.'+family].update(
                positive_examples=['curve.model.fit.'+other+'.positive']))
            for prefix in ['curve_family.', 'parametric_fit.']:
                self.rejects(lambda m: m['operation_routes'][prefix+family].update(certified=True))
        self.rejects(lambda m: m['operation_routes']['curve_family.segmented'].update(
            boundary_examples=['curve.failure.segment129']))

    def test_delegation_policy_needs_actual_bounded_source_examples(self):
        policies = ['Strict', 'Clamp', 'FlatExtrapolation', 'Trust']
        for i, policy in enumerate(policies):
            other = policies[(i+1) % len(policies)]
            self.rejects(lambda m: m['operation_routes']['curve_family.delegated.'+policy].update(
                positive_examples=['curve.delegate.'+other+'.3']))
            self.rejects(lambda m: m['operation_routes']['curve_family.delegated.'+policy].update(
                boundary_examples=['curve.failure.delegate_range.'+policy]))
            self.rejects(lambda m: m['operation_routes']['curve_family.delegated.'+policy].update(certified=True))


    def test_explicit_curve_methods_and_sides_cannot_substitute_or_certify(self):
        for method in ['PiecewiseConstant', 'FlatForward']:
            self.rejects(lambda m: m['operation_routes']['curve_method.'+method].update(
                selectors={'operation.kind':'curves','inputs.curves.0.interpolation':'Linear'}))
            self.rejects(lambda m: m['operation_routes']['curve_method.'+method].update(certified=True))
        for side in ['Left', 'Right']:
            self.rejects(lambda m: m['operation_routes']['curve_method.forward.'+side].update(
                selectors={'operation.kind':'curve_query','operation.query.kind':'FlatForwardRate',
                           'operation.query.side':'Average'}))
            self.rejects(lambda m: m['operation_routes']['curve_method.forward.'+side].update(certified=True))

    def test_ufr_source_and_tail_query_cannot_substitute_or_certify(self):
        self.rejects(lambda m: m['operation_routes']['curve_method.ufr_convergence'].update(
            positive_examples=['method.describe.FlatForward.positive']))
        self.rejects(lambda m: m['operation_routes']['curve_method.ufr_forward'].update(
            boundary_examples=['method.forward.Left.FlatForward.positive.2']))
        for identity in ['curve_method.ufr_convergence','curve_method.ufr_forward']:
            self.rejects(lambda m: m['operation_routes'][identity].update(certified=True))

    def test_monotone_model_and_forward_cannot_substitute_or_certify(self):
        self.rejects(lambda m: m['operation_routes']['curve_method.monotone_convex'].update(
            positive_examples=['method.describe.FlatForward.positive']))
        self.rejects(lambda m: m['operation_routes']['curve_method.monotone_forward'].update(
            boundary_examples=['monotone.last_left.steep']))
        for identity in ['curve_method.monotone_convex','curve_method.monotone_forward']:
            self.rejects(lambda m: m['operation_routes'][identity].update(certified=True))


if __name__ == '__main__': unittest.main()
