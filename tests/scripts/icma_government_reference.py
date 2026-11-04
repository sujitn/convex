"""Independent Decimal60 full-security controls; never import or execute Convex."""
import argparse
from copy import deepcopy
from datetime import timedelta
from decimal import Decimal as D
import json
from pathlib import Path

from icma_reference import PER_YEAR, coupon, day, discount, following, fraction, offset, roll

ROOT = Path(__file__).resolve().parents[2]
INVERSE = dict(lower=-0.5, upper=0.5, price_tolerance=1e-8,
               parameter_tolerance=1e-12, max_iterations=128)
POLICY = dict(id='ICMA.GOV.COMMON', version=41)
CALENDAR = dict(id='ICMA.GOV.CALENDAR', version=7)
HOLIDAYS = ['2024-03-11', '2024-06-18']


def security(name, end, frequency='SemiAnnual', stub='ShortLast', start='2023-08-31',
             settle='2024-01-10', compounding='SemiAnnual', stepped=False,
             separate_accrual=False, ex=False, on_payment='Exclude'):
    template = json.loads((ROOT/'tests/fixtures/library/icma-qualified.json').read_bytes())['cases'][0]['request']
    t = deepcopy(template['operation']['input']['security']['bond']['terms'])
    t.update(issue_date=start, day_count='Act365Fixed' if separate_accrual else 'ActActIcma',
             exercise=[], principal_prices=[], face_value='1000', redemption_multiplier='1.02')
    t['schedule'].update(start_date=start, end_date=end, frequency=frequency,
                         stub_type=stub, end_of_month=True)
    t['calendars'] = {k: CALENDAR['id'] for k in t['calendars']}
    t['dates']['ex_coupon'] = (dict(offset=dict(CalendarDays=-7), anchor='PaymentDate', boundary='OnDate') if ex else None)
    for k, v in [('coupon', '0.045'), ('spread', '0.003'), ('gearing', '0'), ('notional', '1000')]:
        t['profiles'][k] = [dict(start=start, end=end, value=v)]
    if stepped:
        t['profiles']['coupon'] = [dict(start=start, end='2024-04-12', value='0.045'),
                                  dict(start='2024-04-12', end=end, value='0.065')]
        t['profiles']['notional'] = [dict(start=start, end='2024-10-15', value='1000'),
                                    dict(start='2024-10-15', end=end, value='700')]
        t['principal_prices'] = [dict(effective_date='2024-10-15', multiplier='1.015', amount='200')]
    f = PER_YEAR[frequency]; months = 12//f
    anchor = day(end if 'First' in stub else start)
    points = [roll(anchor, n, months, True) for n in range(-40, 41)]
    interior = [p for p in points if day(start) < p < day(end)]
    if stub == 'LongFirst': interior = interior[1:]
    if stub == 'LongLast': interior = interior[:-1]
    boundaries = [day(start), *interior, day(end)]
    mesh = [dict(start=str(a), end=str(b)) for a, b in zip(points, points[1:])
            if b >= day(start)-timedelta(days=400) and a <= day(end)+timedelta(days=400)]
    reference = dict(id=name, version=17)
    context_pin = dict(id=name+'.NOMINAL', version=31)
    icma = dict(reference_context=dict(revision=context_pin, security=reference,
                                       frequency=frequency, periods=mesh),
                convention=dict(schema_version=1, reference_context=context_pin,
                                quotation_policy=POLICY, frequency=frequency,
                                compounding=compounding, max_reference_evaluations=16777216),
                quotation_policy=POLICY, calendars={k: CALENDAR for k in t['calendars']},
                position=dict(reference_quantity='1000', quantity_unit='notional',
                              currency='USD', quantity='-2500'))
    helper = dict(id=name, quote_version=23, pillar=str(following(offset(day(end),
                  t['dates']['payment_delay'], HOLIDAYS), HOLIDAYS)), terms=t,
                  trade_date=settle, settlement=settle, on_payment=on_payment,
                  credit_identity=None, quote_kind=dict(kind='DirtyPrice'),
                  quote=dict(unit='PricePer100', value=100.0), tolerance=1e-8)
    return dict(security=reference, helper=helper, icma=icma), boundaries


def economics(source, boundaries, rate):
    """Exact supplied coupon/principal ownership, accrual, times and cash PV."""
    h = source['helper']; t = h['terms']; context = source['icma']; mesh = context['reference_context']['periods']
    f = PER_YEAR[t['schedule']['frequency']]; settle = day(h['settlement']); flows = []; accrued = D(0)
    pays = lambda a: following(offset(a, t['dates']['payment_delay'], HOLIDAYS), HOLIDAYS)
    owned = lambda p: p > settle or p == settle and h['on_payment'] == 'Include'
    for a, b in zip(boundaries, boundaries[1:]):
        pay = pays(b); amount = coupon(t, a, b, mesh, f)
        entitled = owned(pay)
        if a < settle < b: accrued += coupon(t, a, settle, mesh, f)
        if b <= settle and owned(pay): accrued += amount
        if t['dates']['ex_coupon'] is not None and owned(pay):
            ex_date = pay - timedelta(days=7)
            if settle >= ex_date:
                entitled = False
                accrued -= amount
        if entitled: flows.append(dict(date=str(pay), kind='coupon', amount=str(amount)))
    notional = t['profiles']['notional']
    for i, row in enumerate(notional):
        effective = day(row['end']); after = D(notional[i+1]['value']) if i+1 < len(notional) else D(0)
        amount = D(row['value'])-after; pay = pays(effective)
        if not owned(pay): continue
        parts = []
        allocation = next((p for p in t['principal_prices'] if day(p['effective_date']) == effective), None)
        if allocation:
            allocated = D(allocation['amount']); parts.append(allocated*D(allocation['multiplier'])); amount -= allocated
        if amount: parts.append(amount*(D(t['redemption_multiplier']) if i+1 == len(notional) else 1))
        flows.extend(dict(date=str(pay), kind='principal', amount=str(p)) for p in parts)
    flows.sort(key=lambda c: (c['date'], c['kind'], D(c['amount'])))
    times = {c['date']: fraction(settle, day(c['date']), mesh, f) for c in flows}
    pv = sum((D(c['amount'])*discount(rate, times[c['date']], context['convention']['compounding']) for c in flows), D(0))
    dirty = pv/D(t['face_value'])*100; accr = accrued/D(t['face_value'])*100
    return dict(cashflows=flows, times={k: str(v) for k, v in times.items()}, present_value=str(pv),
                dirty_per100=str(dirty), clean_per100=str(dirty-accr), accrued_per100=str(accr),
                decimal_yield=str(rate), consideration=str(dirty*D(context['position']['quantity'])/100))


def quote(source, boundaries, rate, clean=False):
    result = economics(source, boundaries, rate)
    source['helper']['quote_kind'] = dict(kind='CleanPrice' if clean else 'DirtyPrice')
    source['helper']['quote']['value'] = float(result['clean_per100' if clean else 'dirty_per100'])
    return source


def invert(source, boundaries):
    h = source['helper']; observed = D(str(h['quote']['value']))
    accr = D(economics(source, boundaries, D(0))['accrued_per100'])
    dirty = observed + (accr if h['quote_kind']['kind'] == 'CleanPrice' else 0)
    # Fixed independent bisection domain and count; no Convex solver or output.
    low, high = D('-0.5'), D('0.5')
    for _ in range(180):
        mid = (low+high)/2
        if D(economics(source, boundaries, mid)['dirty_per100']) > dirty: low = mid
        else: high = mid
    result = economics(source, boundaries, (low+high)/2)
    result['observed_dirty_per100'] = str(dirty)
    result['observed_clean_per100'] = str(dirty-accr)
    result['consideration'] = str(dirty*D(source['icma']['position']['quantity'])/100)
    return result


def generate():
    cases = []; references = []
    inputs = dict(calendars=[dict(pin=CALENDAR, holidays=HOLIDAYS, weekend_days=[5, 6])],
                  curves=[], fixings=[], credit_history=[])
    # Retain the actual existing manifest shape, including disjoint unused fields.
    inputs = deepcopy(json.loads((ROOT/'tests/fixtures/library/icma-qualified.json').read_bytes())['cases'][0]['request']['inputs'])
    inputs['calendars'] = [dict(pin=CALENDAR, holidays=HOLIDAYS, weekend_days=[5, 6])]
    variants = [
        ('regular', {}),
        ('short_front', dict(start='2023-10-12', stub='ShortFirst')),
        ('long_front', dict(start='2023-10-12', stub='LongFirst')),
        ('short_back', dict(end='2025-10-12')),
        ('long_back', dict(end='2025-10-12', stub='LongLast')),
        ('annual', dict(frequency='Annual')),
        ('quarterly', dict(frequency='Quarterly')),
        ('monthly', dict(frequency='Monthly')),
        ('stepped', dict(stepped=True)),
        ('separate_accrual', dict(separate_accrual=True)),
        ('delayed_claim', dict(settle='2024-02-29', stepped=True)),
        ('ex_coupon', dict(settle='2024-03-06', ex=True)),
        ('payment_include', dict(settle='2024-03-12', on_payment='Include')),
    ]
    for label, options in variants:
        for selection in ['ExactMaturity', 'LinearMaturity']:
            opts = dict(end='2025-08-31', **{k: v for k, v in options.items() if k != 'end'})
            opts['end'] = options.get('end', opts['end'])
            comp = 'Continuous' if label in ['long_front', 'monthly'] else ('Annual' if label == 'annual' else 'SemiAnnual')
            rate = D('-0.0075') if label in ['quarterly', 'ex_coupon'] else D('0.04125')
            target, tb = security('ICMA.GOV.TARGET.'+label, compounding=comp, **opts)
            quote(target, tb, rate, clean=label in ['stepped', 'delayed_claim', 'ex_coupon'])
            ends = [opts['end']] if selection == 'ExactMaturity' else ['2025-02-28', '2026-02-28', '2027-02-28']
            bs = []; bb = []
            for i, end in enumerate(ends):
                s, boundary = security('ICMA.GOV.BENCH.'+str(i), end, frequency=['Annual', 'Quarterly', 'SemiAnnual'][i],
                                       settle=target['helper']['settlement'], on_payment=target['helper']['on_payment'], compounding=comp)
                br = D('-0.0125')+D(i)*D('0.004') if rate < 0 else D('0.0225')+D(i)*D('0.007')
                quote(s, boundary, br, clean=i % 2 == 1)
                bs.insert(0, dict(quote=dict(security=s['security'], benchmark_id='ICMA.GOV', helper=s['helper']), icma=s['icma']))
                bb.insert(0, boundary)
            results = [invert(dict(security=b['quote']['security'], helper=b['quote']['helper'], icma=b['icma']), boundary) for b, boundary in zip(bs, bb)]
            yields = [D(r['decimal_yield']) for r in results]
            maturities = [day(b['quote']['helper']['terms']['schedule']['end_date']) for b in bs]
            maturity = day(opts['end']); exact = next((i for i, m in enumerate(maturities) if m == maturity), None)
            if exact is not None: low, high, w = exact, None, D(0)
            else:
                order = sorted(range(len(bs)), key=lambda i: maturities[i])
                low, high = next((l, h) for l, h in zip(order, order[1:]) if maturities[l] < maturity < maturities[h])
                w = D((maturity-maturities[low]).days)/D((maturities[high]-maturities[low]).days)
            benchmark = yields[low]+w*((yields[high] if high is not None else yields[low])-yields[low])
            settings = dict(benchmark_id='ICMA.GOV', quotation_policy=POLICY, compounding=comp,
                            inverse=INVERSE, selection=selection, max_reference_evaluations=16777216)
            selected = dict(lower_index=low, upper_index=high, lower_weight=str(1-w), upper_weight=str(w),
                            target_maturity=str(maturity), target_remaining_days=(maturity-day(target['helper']['settlement'])).days)
            target_quote = invert(target, tb); spread = D(target_quote['decimal_yield'])-benchmark
            for kind in ['spread', 'price']:
                operation = dict(kind='icma_government_'+kind, target=deepcopy(target), benchmarks=bs,
                                 reference_date=target['helper']['settlement'], settings=settings)
                if kind == 'spread': answer = target_quote
                else:
                    h = target['helper']
                    operation['target'] = dict(target=dict(security=target['security'], terms=h['terms'], trade_date=h['trade_date'],
                                                          settlement=h['settlement'], on_payment=h['on_payment']), icma=target['icma'])
                    operation['settings'] = dict(government=settings, decimal_spread_tolerance=1e-10)
                    unit = 'DecimalRate' if label in ['monthly', 'stepped'] else 'BasisPoints'
                    value = float(spread if unit == 'DecimalRate' else spread*10000)
                    operation['quote'] = dict(quote_version=71, spread=dict(unit=unit, value=value))
                    spread = D(str(value)) if unit == 'DecimalRate' else D(str(value))/10000
                    answer = economics(target, tb, benchmark+spread)
                id = 'icma_gov.'+label+'.'+selection+'.'+kind
                cases.append(dict(id=id, request=dict(schema_version=1, inputs=inputs, operation=deepcopy(operation)), expected_error=None))
                references.append(dict(id=id, expected=dict(target=answer, benchmarks=results, selected=selected,
                                       benchmark_decimal_yield=str(benchmark), decimal_spread=str(spread), basis_points=str(spread*10000))))
    # Original supplied domains, units and bounds stay fixed in all failure controls.
    base = next(c for c in cases if c['id'] == 'icma_gov.regular.LinearMaturity.price')
    def failure(label, change, code, forward=False):
        row = deepcopy(next(c for c in cases if c['id'] == 'icma_gov.regular.LinearMaturity.spread') if forward else base)
        row['id'] = 'icma_gov.invalid.'+label+('.spread' if forward else '.price')
        change(row['request']['operation']); row['expected_error'] = code; cases.append(row)
    def common(o): return o['settings'].get('government', o['settings'])
    mutations = [
        ('empty_context', lambda o: o['target']['icma']['reference_context'].update(periods=[]), 'invalid_input'),
        ('stale_context', lambda o: o['target']['icma']['convention']['reference_context'].update(version=99), 'invalid_input'),
        ('wrong_security', lambda o: o['target']['icma']['reference_context']['security'].update(version=99), 'invalid_input'),
        ('common_policy', lambda o: common(o)['quotation_policy'].update(version=99), 'invalid_input'),
        ('different_compounding', lambda o: o['benchmarks'][0]['icma']['convention'].update(compounding='Annual'), 'invalid_input'),
        ('normal_frequency', lambda o: o['benchmarks'][0]['icma']['reference_context'].update(frequency='Annual'), 'invalid_input'),
        ('unselected_context', lambda o: o['benchmarks'][0]['icma']['reference_context'].update(periods=[]), 'invalid_input'),
        ('unselected_calendar', lambda o: o['benchmarks'][0]['icma']['calendars']['payment'].update(version=99), 'missing_input'),
        ('unselected_quote', lambda o: o['benchmarks'][0]['quote']['helper']['quote'].update(value=10000.0), 'no_root'),
        ('reference_work', lambda o: o['benchmarks'][0]['icma']['convention'].update(max_reference_evaluations=1), 'resource_limit'),
        ('total_work', lambda o: common(o).update(max_reference_evaluations=1), 'resource_limit'),
        ('out_of_interval', lambda o: o.update(benchmarks=o['benchmarks'][:1]), 'invalid_input'),
        ('duplicate_identity', lambda o: o['benchmarks'].append(deepcopy(o['benchmarks'][0])), 'invalid_input'),
        ('wrong_benchmark', lambda o: o['benchmarks'][0]['quote'].update(benchmark_id='OTHER'), 'invalid_input'),
        ('wrong_entitlement', lambda o: o['benchmarks'][0]['quote']['helper'].update(on_payment='Include'), 'invalid_input'),
        ('original_face', lambda o: o['target']['icma']['position'].update(reference_quantity='700'), 'invalid_input'),
        ('quote_unit', lambda o: o['benchmarks'][0]['quote']['helper']['quote'].update(unit='BasisPoints'), 'invalid_input'),
        ('inverse_work', lambda o: common(o)['inverse'].update(max_iterations=1), 'no_convergence'),
        ('unsupported_context_version', lambda o: o['target']['icma']['convention'].update(schema_version=2), 'unsupported'),
        ('truncated_payment', lambda o: o['benchmarks'][0]['icma']['reference_context'].update(periods=[p for p in o['benchmarks'][0]['icma']['reference_context']['periods'] if p['end'] <= '2027-02-28']), 'invalid_input'),
        ('overlap', lambda o: o['target']['icma']['reference_context']['periods'].insert(1, deepcopy(o['target']['icma']['reference_context']['periods'][0])), 'invalid_input'),
        ('non_unique', lambda o: common(o)['inverse'].update(lower=0.0225, upper=0.02250000000000001, price_tolerance=1), 'non_unique'),
        ('mixed_sign', lambda o: o['benchmarks'][0]['quote']['helper']['terms']['profiles']['coupon'][0].update(value='-0.5'), 'unsupported'),
    ]
    for label, mutate, code in mutations:
        failure(label, mutate, code)
        if label in ['unselected_context', 'unselected_calendar', 'unselected_quote', 'reference_work', 'common_policy', 'different_compounding', 'normal_frequency', 'inverse_work', 'mixed_sign']:
            failure(label, mutate, code, True)
    failure('spread_residual', lambda o: (o['settings'].update(decimal_spread_tolerance=1e-18), common(o)['inverse'].update(parameter_tolerance=1e-6, price_tolerance=0.05)), 'no_convergence')
    failure('target_no_root', lambda o: o['quote']['spread'].update(value=10000.0), 'no_root')
    failure('spread_unit', lambda o: o['quote']['spread'].update(unit='PricePer100'), 'invalid_input')
    return dict(certified=False, cases=cases), dict(precision=60, absolute_price_cash_invoice_tolerance='1e-8',
                absolute_yield_spread_tolerance='1e-9', absolute_time_weight_tolerance='1e-12', relative_tolerance='0',
                qualification='Independent supplied contextual cash and maturity composition; no executed market/Street comparator or approval.', certified=False, cases=references)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(); parser.add_argument('--write', action='store_true'); args = parser.parse_args()
    values = generate()
    for path, value in zip(['tests/fixtures/library/icma-government.json', 'tests/fixtures/library/icma-government-reference.json'], values):
        file = ROOT/path
        if args.write:
            if file.exists(): raise ValueError('Refusing to overwrite original declared controls')
            file.write_text(json.dumps(value, indent=2)+'\n')
        elif json.loads(file.read_bytes()) != value: raise ValueError('Original independent control changed: '+path)
    print(json.dumps(dict(cases=len(values[0]['cases']), independent_full_compositions=len(values[1]['cases']), certified=False)))
