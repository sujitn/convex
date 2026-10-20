"""Predeclared Decimal60 full original cash/time/five-price ICMA risk controls.

Reads immutable independent cash controls, never candidate output. Existing
financial references, source inputs, solver settings and bounds are unchanged.
"""
import argparse
from copy import deepcopy as clone
from decimal import Decimal as D, getcontext
import hashlib
import json
from pathlib import Path
from icma_helpers_reference import quote_price, MULTIPLIER

getcontext().prec = 60
ROOT = Path(__file__).resolve().parents[2]
DEST = ROOT / 'tests/fixtures/library'
SOURCES = ['icma-helpers-final-api.json', 'icma-helpers-reference.json',
           'icma-helpers-additional-reference.json', 'icma-helpers-terms-reference.json',
           'icma-helpers-ready-reference.json', 'icma-helpers-boundaries.json',
           'icma-helpers-final-reference.json']


def generate():
    originals = json.loads((DEST / SOURCES[0]).read_bytes())['cases']
    references = {}
    for source in SOURCES[1:]:
        references.update({c['id']: c['expected'] for c in json.loads((DEST / source).read_bytes())['cases']})
    cases, answers = [], []
    pin = lambda name, version=1: dict(id=name, version=version)

    def add(name, request, helper=None, original=None, error=None):
        cases.append(dict(id='icma_risk.' + name, request=clone(request), expected_error=error))
        if error is not None:
            return
        op = request['operation']; ri = op['action']['input']
        y = D(str(ri['observed_quote']['value'])) / MULTIPLIER[ri['observed_quote']['unit']]
        bump = D(str(ri['settings']['bump']))
        quotes = [quote_price(helper, original['cashflows'], D(original['accrued_per100']), y + delta)
                  for delta in [D(0), bump, -bump, bump / 2, -bump / 2]]
        scale = D(helper['quote_kind']['input']['position']['quantity']) / D(helper['terms']['face_value'])
        pvs = [D(q['present_value']) * scale for q in quotes]
        risk = dict(zip(['base_pv', 'plus_pv', 'minus_pv', 'half_plus_pv', 'half_minus_pv'], map(str, pvs)))
        risk.update(value=str((pvs[1] - pvs[2]) / (2 * bump) * D('.0001')),
                    half_value=str((pvs[3] - pvs[4]) / bump * D('.0001')))
        answers.append(dict(id='icma_risk.' + name, expected=dict(valuations=quotes, sensitivity=risk)))

    def request(h, calendars, supplied, action='yield_sensitivity'):
        i = h['quote_kind']['input']; s = i['sources']
        key = dict(factor=dict(family='interest_rate', target=dict(kind='instrument', instrument_id=s['security']['id']),
                              currency=h['terms']['currency'], tenor='spot', quote_type='yield', basis='raw_quote',
                              convention_version=s['quotation_policy']['version'], model_version=s['quotation_policy']['id']),
                   coordinate=dict(kind='raw', input_id=s['quote_revision']['id'], version=s['quote_revision']['version']),
                   native_unit=h['quote']['unit'], response='frozen_curves', unit='currency_per_bp')
        ri = dict(key=key, observed_quote=h['quote'], snapshot_id='synthetic-ICMA-yield-risk',
                  settings=dict(bump=.0001, stability=dict(absolute=.0001, relative=.000001), policy_version=11),
                  max_cash_evaluations=100000)
        identity = dict(id='ICMA.HOLDING', economics=pin('ICMA.FULL.ECONOMICS', 51),
                        calculation=pin('ICMA.FULL.CALCULATION', 61), entity='ICMA.ENTITY')
        a = dict(kind=action, input=ri)
        if action == 'hedge_vector':
            a['identity'] = identity
        return dict(schema_version=1, inputs=clone(supplied), operation=dict(
            kind='icma_quotation', input=dict(security=dict(reference=s['security'], quote_revision=s['quote_revision'],
            bond=dict(terms=h['terms'], calendars=calendars)), reference_context=i['reference_context'],
            quotation_policy=s['quotation_policy'], position=i['position']), convention=i['convention'],
            cash=dict(kind='plain', settlement=h['settlement'], on_payment=h['on_payment']), action=a))

    selected = [c for c in originals if c['expected_error'] is None
                and c['request']['operation']['kind'] == 'helper'
                and c['request']['operation']['input']['kind'] == 'cash_quote']
    for source in selected:
        entry = source['request']['operation']['input']; h = clone(entry['helper'])
        # The existing actual-payment control has a separately retained corrected
        # accrued answer; all other original answers retain their declared values.
        e = (references[source['id']] if source['id'] == 'icma_helpers.boundary.payment_include'
             else source.get('expected') or references[source['id']])['observed']
        r = request(h, entry['calendars'], source['request']['inputs'])
        add(source['id'].removeprefix('icma_helpers.'), r, h, e)
    first = selected[0]; entry = first['request']['operation']['input']; original = references[first['id']]['observed']
    for comp in ['Simple', 'Annual', 'SemiAnnual', 'Quarterly', 'Monthly', 'Daily', 'Continuous']:
        for unit in MULTIPLIER:
            for quantity in ['1000', '-2500', '0']:
                h = clone(entry['helper']); h['quote_kind']['input']['convention']['compounding'] = comp
                h['quote_kind']['input']['position']['quantity'] = quantity
                h['quote'] = dict(value=float(D('-.0075') * MULTIPLIER[unit]), unit=unit)
                r = request(h, entry['calendars'], first['request']['inputs'])
                add('units.' + comp + '.' + unit + '.' + quantity, r, h, original)
    for unit in MULTIPLIER:
        h = clone(entry['helper']); h['quote_kind']['input']['position']['quantity'] = '1000'
        h['quote'] = dict(value=float(D('.04125') * MULTIPLIER[unit]), unit=unit)
        r = request(h, entry['calendars'], first['request']['inputs'], 'hedge_vector')
        add('hedge.' + unit, r, h, original)
    base = cases[0]['request']
    mutations = [
        ('target', lambda o: o['action']['input']['key']['factor']['target'].update(instrument_id='OTHER'), 'invalid_input'),
        ('quote_identity', lambda o: o['action']['input']['key']['coordinate'].update(input_id='OTHER'), 'invalid_input'),
        ('quote_revision', lambda o: o['action']['input']['key']['coordinate'].update(version=99), 'invalid_input'),
        ('policy_version', lambda o: o['action']['input']['key']['factor'].update(convention_version=99), 'invalid_input'),
        ('policy_identity', lambda o: o['action']['input']['key']['factor'].update(model_version='OTHER'), 'invalid_input'),
        ('wrong_axis', lambda o: o['action']['input']['key']['factor'].update(quote_type='z_spread', family='credit_spread'), 'invalid_input'),
        ('native_unit', lambda o: o['action']['input']['observed_quote'].update(unit='PercentRate'), 'invalid_input'),
        ('price_unit', lambda o: o['action']['input']['observed_quote'].update(unit='PricePer100'), 'invalid_input'),
        ('whole_quote_domain', lambda o: o['action']['input']['observed_quote'].update(value=10.0), 'invalid_input'),
        ('cash_work', lambda o: o['action']['input'].update(max_cash_evaluations=1), 'resource_limit'),
        ('cash_cap', lambda o: o['action']['input'].update(max_cash_evaluations=67108865), 'resource_limit'),
        ('reference_work', lambda o: o['convention'].update(max_reference_evaluations=1), 'resource_limit'),
        ('empty_context', lambda o: o['input']['reference_context'].update(periods=[]), 'invalid_input'),
        ('context_revision', lambda o: o['convention']['reference_context'].update(version=99), 'invalid_input'),
        ('incomplete_context', lambda o: o['input']['reference_context']['periods'].pop(), 'invalid_input'),
        ('frequency', lambda o: o['convention'].update(frequency='Annual'), 'invalid_input'),
        ('currency', lambda o: o['input']['position'].update(currency='EUR'), 'invalid_input'),
        ('reference_face', lambda o: o['input']['position'].update(reference_quantity='700'), 'invalid_input'),
        ('fractional_contracts', lambda o: o['input']['position'].update(quantity_unit='contracts', quantity='1.5'), 'invalid_input'),
        ('stability', lambda o: o['action']['input']['settings'].update(stability=dict(absolute=1e-18, relative=0)), 'invalid_input'),
        ('bump', lambda o: o['action']['input']['settings'].update(bump=0), 'invalid_input'),
        ('compounding_domain', lambda o: (o['convention'].update(compounding='Simple'), o['action']['input']['observed_quote'].update(value=-1)), 'no_convergence'),
    ]
    for name, change, error in mutations:
        r = clone(base); change(r['operation']); add('invalid.' + name, r, error=error)
    for name, change in [
        ('missing_calendar', lambda r: r['inputs']['calendars'].pop()),
        ('stale_calendar', lambda r: r['operation']['input']['security']['bond']['calendars']['payment'].update(version=99)),
    ]:
        r = clone(base); change(r); add('invalid.' + name, r, error='missing_input')
    r = clone(base); del r['operation']['action']['input']['snapshot_id']; add('invalid.missing_snapshot', r, error='invalid_input')
    r = clone(base); r['operation']['action']['input']['inferred_context'] = True; add('invalid.unknown_input', r, error='invalid_input')
    r = clone(base); r['operation']['input']['security']['bond']['terms']['exercise'] = [dict(
        accrued='pay_accrued', notice_offset={'CalendarDays': -5}, price_per_100='101.25', protection_end=None,
        right='call', settlement_adjustment='Following', settlement_lag={'BusinessDays': 5},
        style=dict(date='2024-06-17', style='european'))]
    add('invalid.optional', r, error='invalid_input')
    r = clone(base); r['operation']['cash'] = dict(kind='workout', context=dict(
        settlement=r['operation']['cash']['settlement'], on_payment='Exclude', information_date='2024-01-10', notice=None),
        selection=dict(kind='maturity'))
    add('invalid.conditional_workout', r, error='not_applicable')
    r = clone(next(c['request'] for c in cases if c['id'] == 'icma_risk.hedge.DecimalRate'))
    r['operation']['input']['position']['quantity'] = '-2500'; add('invalid.signed_hedge_reference', r, error='invalid_input')
    return dict(certified=False, cases=cases), dict(certified=False, precision=60,
        tolerances=dict(price_cash_invoice='1e-8', time='1e-12', risk='1e-7', native_quote='1e-10', relative='0'),
        source_reads={p: hashlib.sha256((DEST / p).read_bytes()).hexdigest() for p in SOURCES},
        hedge_control=dict(current_quantity='2500', pending_quantity='-500', reference_quantity='1000',
                           trade_quantity='-2000', final_holding='0', reporting_fx='0.9',
                           native_sell_cost_per_reference='1', native_cost='2', reporting_cost='1.8'), cases=answers)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(); parser.add_argument('--write', action='store_true'); args = parser.parse_args()
    values = generate()
    for name, value in zip(['icma-risk.json', 'icma-risk-reference.json'], values):
        path = DEST / name; data = (json.dumps(value, indent=2) + '\n').encode()
        if args.write:
            if path.exists(): raise ValueError('Refusing to overwrite declared original controls')
            path.write_bytes(data)
        elif path.read_bytes() != data:
            raise ValueError('Original independent controls changed: ' + name)
    print(json.dumps(dict(requests=len(values[0]['cases']), full_successes=len(values[1]['cases']), certified=False)))
