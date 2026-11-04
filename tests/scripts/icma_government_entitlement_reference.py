"""Separate same-payment ownership/accrual controls; original answers stay raw."""
import argparse
from copy import deepcopy
from decimal import Decimal as D
import json
from pathlib import Path
from icma_government_reference import HOLIDAYS, economics, security
from icma_reference import PER_YEAR, coupon, day, following, offset

ROOT = Path(__file__).resolve().parents[2]


def boundaries(source):
    h = source['helper']; t = h['terms']; s = t['schedule']
    return security(h['id'], s['end_date'], frequency=s['frequency'], stub=s['stub_type'],
                    start=s['start_date'], settle=h['settlement'])[1]


def cash(source, boundary, rate):
    result = economics(source, boundary, rate)
    h = source['helper']; t = h['terms']; settle = day(h['settlement']); same_payment = D(0)
    for a, b in zip(boundary, boundary[1:]):
        pay = following(offset(b, t['dates']['payment_delay'], HOLIDAYS), HOLIDAYS)
        # Coupon cash ownership on payment day does not restore accrued interest.
        if b <= settle and pay == settle and h['on_payment'] == 'Include':
            same_payment += coupon(t, a, b, source['icma']['reference_context']['periods'], PER_YEAR[t['schedule']['frequency']])
    accrued = D(result['accrued_per100'])-same_payment/D(t['face_value'])*100
    result.update(accrued_per100=str(accrued), clean_per100=str(D(result['dirty_per100'])-accrued))
    return result


def inverse(source, boundary):
    h = source['helper']; accrued = D(cash(source, boundary, D(0))['accrued_per100'])
    dirty = D(str(h['quote']['value']))+(accrued if h['quote_kind']['kind'] == 'CleanPrice' else 0)
    low, high = D('-0.5'), D('0.5')
    for _ in range(180):
        middle = (low+high)/2
        if D(cash(source, boundary, middle)['dirty_per100']) > dirty: low = middle
        else: high = middle
    result = cash(source, boundary, (low+high)/2)
    result.update(observed_dirty_per100=str(dirty), observed_clean_per100=str(dirty-accrued),
                  consideration=str(dirty*D(source['icma']['position']['quantity'])/100))
    return result


def references():
    cases = json.loads((ROOT/'tests/fixtures/library/icma-government-qualified.json').read_bytes())['cases']
    original = json.loads((ROOT/'tests/fixtures/library/icma-government-reference.json').read_bytes())['cases']
    controls = []
    for case in cases:
        if not case['id'].startswith('icma_gov.payment_include.'): continue
        op = case['request']['operation']; price = op['kind'] == 'icma_government_price'
        expected = deepcopy(next(c['expected'] for c in original if c['id'] == case['id']))
        sources = [dict(security=b['quote']['security'], helper=b['quote']['helper'], icma=b['icma']) for b in op['benchmarks']]
        expected['benchmarks'] = [inverse(s, boundaries(s)) for s in sources]
        selected = expected['selected']; low = selected['lower_index']; high = selected['upper_index']; w = D(selected['upper_weight'])
        yields = [D(r['decimal_yield']) for r in expected['benchmarks']]
        benchmark = yields[low]+w*((yields[high] if high is not None else yields[low])-yields[low])
        target = op['target']
        if price:
            t = target['target']
            source = dict(security=t['security'], helper=dict(terms=t['terms'], settlement=t['settlement'], on_payment=t['on_payment'], id=t['security']['id']), icma=target['icma'])
            spread = D(str(op['quote']['spread']['value']))/(10000 if op['quote']['spread']['unit'] == 'BasisPoints' else 1)
            expected['target'] = cash(source, boundaries(source), benchmark+spread)
        else:
            expected['target'] = inverse(target, boundaries(target)); spread = D(expected['target']['decimal_yield'])-benchmark
        expected.update(benchmark_decimal_yield=str(benchmark), decimal_spread=str(spread), basis_points=str(spread*10000))
        controls.append(dict(id=case['id'], expected=expected))
    return dict(certified=False, qualification='Four separate actual-payment-date controls. Original supplied requests/observations/cash/times/bounds remain. Payment-day owned coupons no longer contribute completed-unpaid accrued. Original failed raw accrued/clean and consequent clean-benchmark conversion/yield/composition answers stay immutable diagnostics; actual cash-helper ownership is unchanged.', cases=controls)


if __name__ == '__main__':
    p = argparse.ArgumentParser(); p.add_argument('--write', action='store_true'); a = p.parse_args(); value = references()
    file = ROOT/'tests/fixtures/library/icma-government-entitlement-reference.json'
    if a.write:
        if file.exists(): raise ValueError('Refusing to overwrite declared boundary controls')
        file.write_text(json.dumps(value, indent=2)+'\n')
    elif json.loads(file.read_bytes()) != value: raise ValueError('Original boundary controls changed')
    print(json.dumps(dict(separate_full_payment_boundary_controls=len(value['cases']), certified=False)))
