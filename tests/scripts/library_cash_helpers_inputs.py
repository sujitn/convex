#!/usr/bin/env python3
"""Complete cash-helper inputs from unchanged independent cash/ISDA controls."""
import argparse
import copy
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PATH = ROOT / 'tests/fixtures/library/cash-helpers.json'


def corpus():
    original = json.loads((ROOT / 'tests/fixtures/library/cases.json').read_bytes())
    reference = json.loads((ROOT / 'tests/fixtures/curves/cash-curves.json').read_bytes())
    cases = []

    def direct(recipe, parameters):
        curve = copy.deepcopy(recipe['curve'])
        curve.update(calibration='Direct', dependencies=[], source={'Nodes': [
            {'pillar': {'Date': date}, 'quote': {'value': float(value), 'unit': 'DecimalRate'}}
            for date, value in zip([curve['reference_date']] + recipe['pillars'],
                                   [parameters[0]] + parameters)]})
        return curve

    def rec(r):
        return r['operation']['input']['input']['recipe']

    def helpers(r):
        return rec(r)['helpers']

    def observed(r, i=-1):
        return helpers(r)[i]['input']

    def output_curve(r):
        return next(c for c in r['inputs']['curves'] if c['key'] == rec(r)['curve']['key'])

    def global_mode(r):
        c = rec(r)
        c.update(bootstrap=None)
        c['curve']['calibration'] = {'Global': dict(
            initial_zeros=[0.0] * len(c['pillars']), lower_zero=-0.05, upper_zero=0.1,
            quote_scales=[1.0] * len(c['helpers']), weights=[1.0] * len(c['helpers']),
            objective_tolerance=1e-9, max_iterations=100)}

    def add(base, suffix, mutate=None, error=None):
        r = copy.deepcopy(base)
        if mutate:
            mutate(r)
        cases.append(dict(id='cash_helpers.' + suffix, request=r, expected_error=error))

    bases = {}
    for row in reference['cases']:
        request = next(c['request'] for c in original['cases']
                       if c['id'] == 'calibration.cash.' + row['id'])
        inputs = copy.deepcopy(request['inputs'])
        inputs['curves'] += [direct(recipe, row['expected_parameters'][recipe['curve']['key']['id']])
                             for recipe in row['recipes']]
        for input_ in request['operation']['input']['cash']:
            role = input_['recipe']['curve']['role']['kind']
            family = {'Government': 'government', 'IssuerSpread': 'reference_basis', 'Repo': 'repo'}[role]
            base = dict(schema_version=1, inputs=copy.deepcopy(inputs),
                        operation=dict(kind='helper', input=dict(kind='cash_reprice', input=copy.deepcopy(input_))))
            bases[(row['id'], family)] = base
            add(base, family + '.' + row['id'])
            add(base, family + '.' + row['id'] + '.global', global_mode)

    # Existing original USD ISDA capture supplies the credit dependency only.
    credit = reference['credit_case']
    pack = ROOT / 'tests/fixtures/comparisons/credit-isda-1.8.3'
    source = json.loads((pack / 'inputs.json').read_bytes())
    survival = json.loads((pack / 'isda-reference.json').read_bytes())['experiments'][0]
    recipe = copy.deepcopy(credit['recipe'])
    for helper in recipe['helpers']:
        for profile in helper['input']['terms']['profiles'].values():
            for entry in profile:
                # Native Decimal serialization preserves these digits and scale,
                # and canonically writes the leading zero in .04/.05.
                if entry['value'].startswith('.'):
                    entry['value'] = '0' + entry['value']
    template = source['template']
    curve = copy.deepcopy(source['discount_recipe'])
    curve.update(key=copy.deepcopy(template['survival_curve']),
                 role=dict(kind='Credit', entity_id=template['reference']['entity_id'],
                           seniority=recipe['basis_model']['seniority'],
                           recovery=template['reference']['recovery_fraction']),
                 representation='SurvivalProbabilities', source={'Nodes': [
                     {'pillar': {'Date': date}, 'quote': {'value': value, 'unit': 'SurvivalProbability'}}
                     for date, value in zip([curve['reference_date']] + source['pillars'],
                                            [1.0] + survival['survival_probabilities'])]})
    # Bond helpers retain their own original calendar identity; its supplied
    # holiday values are the unchanged official NYM capture used by the fixture.
    pin = dict(id=recipe['helpers'][0]['input']['terms']['calendars']['payment'], version=1)
    inputs = dict(calendars=[dict(pin=pin, holidays=source['calendar_holidays'], weekend_days=[5, 6])],
                  helper_calendars=[], fixings=[], publications=[], calibration=None,
                  curves=[copy.deepcopy(source['discount_recipe']), curve,
                          direct(recipe, credit['expected_parameters'])])
    for node in inputs['curves'][0]['source']['Nodes']:
        node['quote']['value'] = float(node['quote']['value'])
    credit_base = dict(schema_version=1, inputs=inputs,
                       operation=dict(kind='helper', input=dict(kind='cash_reprice', input=dict(
                           recipe=recipe, calendars=[dict(kind='bond', pins={k:copy.deepcopy(pin)
                           for k in ('payment', 'reset', 'observation', 'exercise')}) for _ in recipe['helpers']]))))
    bases[('positive', 'credit_basis')] = credit_base
    add(credit_base, 'credit_basis.positive')
    add(credit_base, 'credit_basis.global', global_mode)

    for family in ['government', 'reference_basis', 'credit_basis', 'repo']:
        base = bases[('positive', family)]
        def reverse(r):
            helpers(r).reverse()
            r['operation']['input']['input']['calendars'].reverse()
            rec(r)['curve']['source']['Helpers'].reverse()
        add(base, family + '.reversed', reverse)
        add(base, family + '.off_market', lambda r: observed(r, 0)['quote'].update(
            value=observed(r, 0)['quote']['value'] + (1.0 if family != 'repo' else 0.01)))
        add(base, family + '.missing_curve', lambda r: r['inputs']['curves'].remove(output_curve(r)), 'missing_input')
        add(base, family + '.wrong_version', lambda r: rec(r)['curve']['key'].update(version='missing'), 'missing_input')
        add(base, family + '.missing_calendar', lambda r: r['inputs'].update(calendars=[]), 'missing_input')
        add(base, family + '.calendar_revision', lambda r: r['operation']['input']['input']['calendars'][-1][
            'pin' if family == 'repo' else 'pins'].update(version=2) if family == 'repo' else
            r['operation']['input']['input']['calendars'][-1]['pins']['payment'].update(version=2), 'missing_input')
        add(base, family + '.calendar_count', lambda r: r['operation']['input']['input']['calendars'].pop(), 'invalid_input')
        add(base, family + '.wrong_method', lambda r: output_curve(r).update(extrapolation='Flat'), 'invalid_input')
        add(base, family + '.wrong_origin', lambda r: rec(r)['curve'].update(reference_date='2022-06-20'
            if family == 'credit_basis' else '2026-10-06'), 'invalid_input')
        add(base, family + '.zero_tolerance', lambda r: observed(r).update(tolerance=0.0), 'invalid_input')
        add(base, family + '.wrong_unit', lambda r: observed(r)['quote'].update(unit='UpfrontFraction'), 'invalid_input')
        add(base, family + '.source_order', lambda r: rec(r)['curve']['source']['Helpers'].reverse(), 'invalid_input')
        add(base, family + '.missing_bootstrap', lambda r: rec(r).update(bootstrap=None), 'invalid_input')
        add(base, family + '.global_work', lambda r: (global_mode(r), rec(r)['curve']['calibration']['Global'].update(max_iterations=0)), 'invalid_input')

    govt = bases[('positive', 'government')]
    def single(r, index):
        c = rec(r); i = r['operation']['input']['input']
        c['helpers'] = [c['helpers'][index]]; i['calendars'] = [i['calendars'][index]]
        h = c['helpers'][0]['input']
        c['pillars'] = [h.get('pillar', h.get('end_date'))]
        c['curve']['source'] = {'Helpers': [c['helpers'][0]['input']['id']]}
    for name, index in [('bill', 0), ('stepped_amortizing', 1), ('par', 2)]:
        add(govt, 'government.' + name, lambda r, i=index: single(r, i))
    add(govt, 'government.dirty', lambda r: (single(r, 1), observed(r).update(
        quote_kind=dict(kind='DirtyPrice'), quote=dict(unit='PricePer100', value=float(reference['cases'][0]['government_expected'][1]['dirty_per100'])))))
    add(govt, 'government.wrong_benchmark', lambda r: output_curve(r)['role'].update(benchmark_id='Other'), 'invalid_input')
    add(govt, 'government.settlement', lambda r: observed(r).update(settlement='2026-10-14'), 'invalid_input')
    add(govt, 'government.payment_horizon', lambda r: observed(r).update(pillar='2028-10-07'), 'invalid_input')
    add(govt, 'government.insufficient_curve_horizon', lambda r: output_curve(r)['source']['Nodes'].pop(), 'invalid_input')
    add(govt, 'government.contextual_yield', lambda r: (single(r, 1), observed(r).update(
        quote_kind=dict(kind='CashflowYield', convention=dict(day_count='ActActIcma', compounding='SemiAnnual',
        lower_yield=-0.9, upper_yield=1.0, max_iterations=256, price_tolerance=1e-11)),
        quote=dict(unit='DecimalRate', value=0.04))), 'unsupported')
    for family in ['reference_basis', 'credit_basis']:
        base = bases[('positive', family)]
        add(base, family + '.missing_dependency', lambda r: r['inputs']['curves'].remove(next(
            c for c in r['inputs']['curves'] if c['key'] == rec(r)['curve']['dependencies'][0])), 'missing_input')
        add(base, family + '.wrong_dependencies', lambda r: rec(r)['curve'].update(dependencies=[]), 'invalid_input')
        add(base, family + '.wrong_model', lambda r: rec(r)['curve']['role'].update(model_id='Other'), 'invalid_input')
        add(base, family + '.wrong_entity', lambda r: observed(r)['credit_identity'].update(entity_id='Other'), 'invalid_input')
    add(credit_base, 'credit_basis.wrong_recovery', lambda r: rec(r)['basis_model'].update(recovery=0.5), 'invalid_input')
    add(credit_base, 'credit_basis.wrong_seniority', lambda r: observed(r)['credit_identity'].update(seniority='Subordinated'), 'invalid_input')
    add(credit_base, 'credit_basis.wrong_credit_method', lambda r: r['inputs']['curves'][1].update(interpolation='Linear'), 'invalid_input')
    add(credit_base, 'credit_basis.delayed_principal', lambda r: observed(r)['terms']['dates'].update(payment_delay={'CalendarDays':1}), 'invalid_input')
    repo = bases[('positive', 'repo')]
    add(repo, 'repo.wrong_collateral', lambda r: observed(r).update(collateral_id='Other'), 'invalid_input')
    add(repo, 'repo.nonbusiness_cash', lambda r: observed(r).update(end_date='2027-04-10'), 'invalid_input')
    add(repo, 'repo.wrong_resource_family', lambda r: r['operation']['input']['input']['calendars'].__setitem__(
        -1, copy.deepcopy(govt['operation']['input']['input']['calendars'][0])), 'invalid_input')
    def repeated(r, count):
        single(r, 0); first = copy.deepcopy(helpers(r)[0])
        calendar = copy.deepcopy(r['operation']['input']['input']['calendars'][0])
        rec(r)['helpers'] = []
        for i in range(count):
            h = copy.deepcopy(first); h['input'].update(id=f'cash-original-{i:03d}', quote_version=1001 + i)
            rec(r)['helpers'].append(h)
        r['operation']['input']['input']['calendars'] = [copy.deepcopy(calendar) for _ in range(count)]
        rec(r)['curve']['source'] = {'Helpers':[h['input']['id'] for h in helpers(r)]}
        rec(r)['overlap'] = 'RetainAllGlobal'; global_mode(r)
    for family, base in [('government', govt), ('repo', repo)]:
        add(base, family + '.max_helpers', lambda r: repeated(r, 128))
        add(base, family + '.helper_limit', lambda r: repeated(r, 129), 'resource_limit')
    return dict(schema_version=1, certified=False,
                qualification='Original full USD cash/issuer/repo recipes and unchanged independent cash/ISDA dependency controls; fresh native outputs are transport expectations only.', cases=cases)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__); parser.add_argument('--write', action='store_true')
    data = corpus(); content = (json.dumps(data, separators=(',', ':')) + '\n').encode()
    if parser.parse_args().write: PATH.write_bytes(content)
    elif PATH.read_bytes() != content: raise ValueError('Original supplied cash helper terms changed')
    print(json.dumps(dict(status='passed', cases=len(data['cases']), certified=False)))
