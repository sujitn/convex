#!/usr/bin/env python3
"""Full supplied CDS-helper requests; reuse unchanged actual ISDA input controls."""
import argparse
import copy
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PATH = ROOT / 'tests/fixtures/library/credit-helpers.json'


def corpus():
    original = json.loads((ROOT / 'tests/fixtures/library/cases.json').read_bytes())
    base = next(c['request'] for c in original['cases'] if c['id'] == 'credit.baseline')
    recipe = copy.deepcopy(base['operation']['input']['recipe'])
    pack = ROOT / 'tests/fixtures/comparisons/credit-isda-1.8.3'
    mixed = json.loads((pack / 'mixed-inputs.json').read_bytes())
    reference = json.loads((pack / 'isda-reference.json').read_bytes())['experiments'][0]
    for m in mixed['helpers']:
        h = recipe['helpers'][m['pillar_index']]
        h['terms']['coupon_decimal'] = m['coupon_decimal']
        h.update(quote_kind=m['quote_kind'], quote=copy.deepcopy(m['quote']), tolerance=m['tolerance'])
    curve = copy.deepcopy(recipe['curve'])
    curve.update(calibration='Direct', dependencies=[], source={'Nodes': [
        {'pillar': {'Date': d}, 'quote': {'value': v, 'unit': 'SurvivalProbability'}}
        for d, v in zip([curve['reference_date']] + recipe['pillars'],
                        [1.0] + reference['survival_probabilities'])]})
    inputs = copy.deepcopy(base['inputs'])
    inputs['curves'].append(curve)
    request = dict(schema_version=1, inputs=inputs,
                   operation={'kind': 'helper', 'input': {'kind': 'credit_reprice', 'recipe': recipe}})
    cases = []

    def rec(r):
        return r['operation']['input']['recipe']

    def helpers(r):
        return rec(r)['helpers']

    def credit(r):
        return r['inputs']['curves'][1]

    def global_mode(r):
        c = rec(r)
        c['bootstrap'] = None
        c['curve']['calibration'] = {'CreditGlobal': dict(
            initial_hazards=[0.02] * len(c['pillars']), lower_hazard=0.0, upper_hazard=1.0,
            quote_scales=[1.0 if h['quote']['unit'] == 'BasisPoints' else
                          0.01 if h['quote']['unit'] == 'PercentRate' else 0.0001 for h in c['helpers']],
            weights=[1.0] * len(c['helpers']), objective_tolerance=1e-9, max_iterations=100)}

    def single(r, index):
        c = rec(r)
        h = c['helpers'][index]
        c['helpers'] = [h]
        c['pillars'] = [h['pillar']]
        c['curve']['source'] = {'Helpers': [h['id']]}

    def add(suffix, mutation=None, error=None):
        r = copy.deepcopy(request)
        if mutation:
            mutation(r)
        cases.append(dict(id='credit_helpers.' + suffix, request=r, expected_error=error))

    add('mixed')
    add('global', global_mode)
    def reverse(r):
        helpers(r).reverse()
        rec(r)['curve']['source']['Helpers'].reverse()
    add('reversed', reverse)
    add('seller', lambda r: [h['terms'].update(side='ProtectionSeller') for h in helpers(r)])
    for kind, index in [('running', 0), ('clean', 1), ('dirty', 2), ('positive_clean', 4), ('negative_dirty', 5)]:
        add(kind, lambda r, i=index: single(r, i))
    add('running_decimal', lambda r: (single(r, 0), helpers(r)[0]['quote'].update(value=0.004, unit='DecimalRate')))
    add('running_percent', lambda r: (single(r, 0), helpers(r)[0]['quote'].update(value=0.4, unit='PercentRate')))
    add('off_market', lambda r: helpers(r)[0]['quote'].update(value=41.0))
    add('zero_hazard', lambda r: [n['quote'].update(value=1.0) for n in credit(r)['source']['Nodes']])
    add('negative_discount', lambda r: [n['quote'].update(value=1.01)
        for n in r['inputs']['curves'][0]['source']['Nodes'][1:]])
    def cash_state(r, state):
        single(r, 0)
        h = helpers(r)[0]
        # Explicit same-day settlement permits both states on the actual cash
        # date, without rolling the frozen reference curves or valuation date.
        h['terms'].update(initial_clean_upfront_fraction='0.001',
                          settlement_lag_business_days=0,
                          cash_settlement_date=h['context']['valuation_date'])
        h['context'].update(initial_payment_state=state,
                            cash_settlement_date=h['terms']['cash_settlement_date'])
    add('pending_cash', lambda r: cash_state(r, 'Pending'))
    add('settled_cash', lambda r: cash_state(r, 'Settled'))
    def repeated(r, count):
        single(r, 0)
        first = copy.deepcopy(helpers(r)[0])
        rec(r)['helpers'] = []
        for i in range(count):
            h = copy.deepcopy(first)
            h.update(id=f'original-cds-{i:03d}', quote_version=1001 + i)
            rec(r)['helpers'].append(h)
        rec(r)['curve']['source'] = {'Helpers': [h['id'] for h in helpers(r)]}
        global_mode(r)
    add('max_helpers', lambda r: repeated(r, 128))
    add('helper_limit', lambda r: repeated(r, 129), 'resource_limit')
    add('missing_credit', lambda r: r['inputs']['curves'].pop(), 'missing_input')
    add('missing_discount', lambda r: r['inputs']['curves'].pop(0), 'missing_input')
    add('wrong_credit_version', lambda r: credit(r)['key'].update(version='unavailable'), 'missing_input')
    add('wrong_discount_version', lambda r: r['inputs']['curves'][0]['key'].update(version='unavailable'), 'missing_input')
    add('missing_calendar', lambda r: r['inputs'].update(calendars=[]), 'missing_input')
    add('calendar_revision', lambda r: helpers(r)[-1]['terms']['premium_calendar'].update(version=2), 'missing_input')
    add('wrong_credit_role', lambda r: credit(r)['role'].update(entity_id='Different.Entity'), 'invalid_input')
    add('wrong_recovery', lambda r: credit(r)['role'].update(recovery=0.5), 'invalid_input')
    add('wrong_currency', lambda r: credit(r).update(currency='EUR'), 'invalid_input')
    add('wrong_origin', lambda r: rec(r)['curve'].update(reference_date='2022-06-20'), 'invalid_input')
    add('wrong_time_basis', lambda r: credit(r).update(time_basis='Act360'), 'invalid_input')
    add('wrong_method', lambda r: credit(r).update(interpolation='Linear'), 'invalid_input')
    add('wrong_discount_role', lambda r: r['inputs']['curves'][0].update(role={'kind': 'Government', 'benchmark_id': 'TEST'}), 'invalid_input')
    add('wrong_discount_method', lambda r: r['inputs']['curves'][0].update(interpolation='Linear'), 'unsupported')
    add('insufficient_horizon', lambda r: credit(r)['source']['Nodes'].pop(), 'invalid_input')
    add('zero_tolerance', lambda r: helpers(r)[-1].update(tolerance=0.0), 'invalid_input')
    add('negative_running_quote', lambda r: helpers(r)[0]['quote'].update(value=-1.0), 'invalid_input')
    add('wrong_running_unit', lambda r: helpers(r)[0]['quote'].update(unit='UpfrontFraction'), 'invalid_input')
    add('wrong_upfront_unit', lambda r: helpers(r)[1]['quote'].update(unit='BasisPoints'), 'invalid_input')
    add('wrong_document', lambda r: helpers(r)[-1]['terms']['reference']['document'].update(version=2), 'invalid_input')
    add('wrong_survival_mapping', lambda r: helpers(r)[-1]['terms']['survival_curve'].update(version='missing'), 'invalid_input')
    add('wrong_maturity', lambda r: helpers(r)[-1].update(pillar='2032-06-19'), 'invalid_input')
    add('wrong_valuation_date', lambda r: helpers(r)[-1]['context'].update(valuation_date='2022-06-20'), 'invalid_input')
    add('wrong_initial_cash_state', lambda r: helpers(r)[-1]['context'].update(initial_payment_state='Pending'), 'invalid_input')
    def premature_settlement(r):
        single(r, 0)
        helpers(r)[0]['terms']['initial_clean_upfront_fraction'] = '0.001'
        helpers(r)[0]['context']['initial_payment_state'] = 'Settled'
    add('premature_settlement', premature_settlement, 'invalid_input')
    add('duplicate_helper', lambda r: (helpers(r)[-1].update(id=helpers(r)[0]['id']),
        rec(r)['curve']['source']['Helpers'].__setitem__(-1, helpers(r)[0]['id'])), 'invalid_input')
    add('wrong_source_order', lambda r: rec(r)['curve']['source']['Helpers'].reverse(), 'invalid_input')
    add('missing_dependencies', lambda r: rec(r)['curve'].update(dependencies=[]), 'invalid_input')
    add('missing_bootstrap', lambda r: rec(r).update(bootstrap=None), 'invalid_input')
    add('invalid_global_work', lambda r: (global_mode(r), rec(r)['curve']['calibration']['CreditGlobal'].update(max_iterations=0)), 'invalid_input')
    add('invalid_global_objective', lambda r: (global_mode(r), rec(r)['curve']['calibration']['CreditGlobal'].update(objective_tolerance=0.0)), 'invalid_input')
    add('nonmonotone_survival', lambda r: credit(r)['source']['Nodes'][-1]['quote'].update(value=1.01), 'invalid_input')
    return dict(schema_version=1, certified=False,
                qualification='Original full USD CDS recipe/quote/calendar inputs and unchanged actual ISDA survival nodes; native results are transport expectations only.',
                cases=cases)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--write', action='store_true')
    args = parser.parse_args()
    data = corpus()
    content = (json.dumps(data, separators=(',', ':')) + '\n').encode()
    if args.write:
        PATH.write_bytes(content)
    elif PATH.read_bytes() != content:
        raise ValueError('Original supplied CDS helper terms changed')
    print(json.dumps(dict(status='passed', cases=len(data['cases']), certified=False)))


if __name__ == '__main__':
    main()
