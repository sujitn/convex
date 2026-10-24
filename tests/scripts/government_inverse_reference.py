"""Independent Decimal60 actual-security spread/dated-cash controls.

No candidate executable or Convex price is read. Synthetic annual ACT/365F
coupon accruals are integrated across independent coupon/notional boundaries;
principal allocations, delayed cash and ex-coupon entitlement stay separate.
"""
import argparse
import copy
import datetime as dt
from decimal import Decimal as D, getcontext
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
getcontext().prec = 60
START = '2024-01-01'
INVERSE = dict(lower=-0.1, upper=0.5, parameter_tolerance=1e-13,
               price_tolerance=1e-9, max_iterations=128)


def day(value):
    return dt.date.fromisoformat(value)


def discount(rate, days, convention):
    t = D(days) / D(360 if convention['day_count'] == 'Act360' else 365)
    comp = convention['compounding']
    if comp == 'Simple':
        return 1 / (1 + rate*t)
    if comp == 'Continuous':
        return (-rate*t).exp()
    n = D(dict(Annual=1, SemiAnnual=2, Quarterly=4, Monthly=12, Daily=365)[comp])
    return (1 + rate/n) ** (-n*t)


def cash(target):
    t = target['terms']; start = day(t['schedule']['start_date'])
    end = day(t['schedule']['end_date']); settle = day(target['settlement'])
    delay = dt.timedelta(days=t['dates']['payment_delay']['CalendarDays'])
    points = [start] + [dt.date(y, 1, 1) for y in range(start.year+1, end.year+1)
                        if dt.date(y, 1, 1) < end] + [end]
    flows = []; accrued = D(0)
    def entitled(pay):
        return pay > settle or pay == settle and target['on_payment'] == 'Include'
    for a, b in zip(points, points[1:]):
        full = D(0); prefix = D(0)
        for cp in t['profiles']['coupon']:
            for np in t['profiles']['notional']:
                lo = max(a, day(cp['start']), day(np['start']))
                hi = min(b, day(cp['end']), day(np['end']))
                if hi > lo:
                    scale = D(cp['value'])*D(np['value'])/365
                    full += scale*D((hi-lo).days)
                    if settle > lo:
                        prefix += scale*D((min(hi, settle)-lo).days)
        pay = b+delay; rule = t['dates']['ex_coupon']; ex = False
        if rule:
            anchor = pay if rule['anchor'] == 'PaymentDate' else b
            record = anchor+dt.timedelta(days=rule['offset']['CalendarDays'])
            ex = settle >= record if rule['boundary'] == 'OnDate' else settle > record
        if entitled(pay) and not ex:
            flows.append((pay, full))
        if a < settle < pay:
            accrued += prefix-full if ex else prefix
    notionals = t['profiles']['notional']
    prices = {p['effective_date']:p for p in t['principal_prices']}
    for i, np in enumerate(notionals):
        date = np['end']; final = i == len(notionals)-1
        repayment = D(np['value'])-(D(0) if final else D(notionals[i+1]['value']))
        pay = day(date)+delay
        if repayment and entitled(pay):
            multiplier = D(t['redemption_multiplier']) if final else D(1)
            if date in prices:
                p = prices[date]; allocation = repayment if p['amount'] is None else D(p['amount'])
                flows.append((pay, allocation*D(p['multiplier'])))
                if allocation != repayment:
                    flows.append((pay, (repayment-allocation)*multiplier))
            else:
                flows.append((pay, repayment*multiplier))
    return flows, accrued*100/D(t['face_value'])


def price(target, yield_rate, convention):
    flows, accrued = cash(target); settle = day(target['settlement'])
    dirty = sum((amount*discount(yield_rate, (date-settle).days, convention)
                 for date, amount in flows), D(0))*100/D(target['terms']['face_value'])
    return dict(dirty_per100=str(dirty), clean_per100=str(dirty-accrued),
                accrued_per100=str(accrued),
                cash=[dict(date=date.isoformat(), amount=str(amount)) for date, amount in flows])


def inverse(helper, convention):
    observed = D(str(helper['quote']['value']))
    if helper['quote_kind']['kind'] == 'CleanPrice':
        observed += cash(helper)[1]
    low = D('-0.1'); high = D('0.5')
    for _ in range(220):
        middle = (low+high)/2
        if D(price(helper, middle, convention)['dirty_per100']) > observed:
            low = middle
        else:
            high = middle
    return (low+high)/2


def build():
    old = json.loads((ROOT/'tests/fixtures/library/cash.json').read_bytes())['cases']
    template = next(c['request'] for c in old if c['id'] == 'government.linear')
    inputs = copy.deepcopy(template['inputs']); pins = template['operation']['target']['calendars']
    base = template['operation']['target']['helper']; cases = []; references = []

    def terms(end, stepped=False, delay=0, zero=False, ex=False):
        t = copy.deepcopy(base['terms']); t['schedule']['end_date'] = end
        t['dates']['payment_delay'] = dict(CalendarDays=delay)
        t['dates']['ex_coupon'] = (dict(offset=dict(CalendarDays=-7), anchor='PaymentDate', boundary='OnDate') if ex else None)
        for name, value in [('coupon','0' if zero else '0.035'), ('spread','0'), ('gearing','0'), ('notional','5000')]:
            t['profiles'][name] = [dict(start=START, end=end, value=value)]
        if stepped:
            t['profiles']['coupon'] = [dict(start=START, end='2024-08-01', value='0.02125'),
                                      dict(start='2024-08-01', end=end, value='0.05775')]
            t['profiles']['notional'] = [dict(start=START, end='2025-03-01', value='5000'),
                                        dict(start='2025-03-01', end=end, value='3200')]
            t['principal_prices'] = [dict(effective_date='2025-03-01', multiplier='1.015', amount='700')]
            t['redemption_multiplier'] = '1.02'
        return t

    def target(end='2027-01-01', settle='2024-06-01', **kwargs):
        return dict(security=dict(id='synthetic.inverse.target', version=41), terms=terms(end, **kwargs),
                    trade_date=settle, settlement=settle, on_payment='Exclude')

    def benchmark(end, rate, conv, settle, index):
        h = copy.deepcopy(base); h.update(id='synthetic.inverse.gov.'+str(index), quote_version=51+index,
            terms=terms(end), trade_date=settle, settlement=settle, on_payment='Exclude', pillar=end,
            quote_kind=dict(kind='CleanPrice' if index % 2 else 'DirtyPrice'))
        computed = price(h, D(rate), conv)
        h['quote'] = dict(unit='PricePer100', value=float(computed['clean_per100' if index % 2 else 'dirty_per100']))
        return dict(quote=dict(security=dict(id=h['id'], version=61+index), benchmark_id='synthetic.government', helper=h), calendars=pins)

    def add(name, t=None, comp='SemiAnnual', spread=12.3456789, unit='BasisPoints',
            selection='LinearMaturity', sign=1, quantity='15000', endpoints=None):
        t = copy.deepcopy(t or target()); conv = dict(day_count='Act365Fixed', compounding=comp)
        maturity = t['terms']['schedule']['end_date']; settle = t['settlement']
        dates = endpoints or (['2025-01-01', '2029-01-01', '2030-01-01'] if selection == 'LinearMaturity' else [maturity])
        bs = [benchmark(end, str(sign*(0.02+0.01*i)), conv, settle, i) for i, end in enumerate(dates)]
        bs.reverse()
        for b in bs: b['quote']['helper']['on_payment'] = t['on_payment']
        operation = dict(kind='government_spread_price', target=dict(target=t, calendars=pins), benchmarks=bs,
            reference_date=settle, settings=dict(government=dict(benchmark_id='synthetic.government',
                convention=conv, inverse=INVERSE, selection=selection), decimal_spread_tolerance=1e-10),
            quote=dict(quote_version=71, spread=dict(unit=unit, value=spread)),
            position=dict(currency=t['terms']['currency'], quantity=quantity, reference_quantity='5000', quantity_unit='notional'))
        row = dict(id='gov_inverse.'+name, request=dict(schema_version=1, inputs=inputs, operation=operation), expected_error=None)
        cases.append(copy.deepcopy(row)); yields = [inverse(b['quote']['helper'], conv) for b in bs]
        ordered = sorted(range(len(bs)), key=lambda i:bs[i]['quote']['helper']['terms']['schedule']['end_date'])
        exact = next((i for i in ordered if bs[i]['quote']['helper']['terms']['schedule']['end_date'] == maturity), None)
        if exact is not None:
            lower = exact; upper = None; weight = D(0)
        else:
            lower, upper = next((lo, hi) for lo, hi in zip(ordered, ordered[1:])
                if bs[lo]['quote']['helper']['terms']['schedule']['end_date'] < maturity < bs[hi]['quote']['helper']['terms']['schedule']['end_date'])
            low_day = day(bs[lower]['quote']['helper']['terms']['schedule']['end_date'])
            high_day = day(bs[upper]['quote']['helper']['terms']['schedule']['end_date'])
            weight = D((day(maturity)-low_day).days)/D((high_day-low_day).days)
        government = yields[lower] + (weight*(yields[upper]-yields[lower]) if upper is not None else 0)
        decimal_spread = D(str(spread))/(10000 if unit == 'BasisPoints' else 1)
        y = government+decimal_spread; result = price(t, y, conv)
        result.update(benchmark_decimal_yield=str(government), target_decimal_yield=str(y),
            decimal_spread=str(decimal_spread), basis_points=str(decimal_spread*10000),
            consideration=str(D(result['dirty_per100'])*D(quantity)/100),
            benchmark_yields=[str(v) for v in yields], lower_index=lower, upper_index=upper,
            lower_weight=str(1-weight), upper_weight=str(weight), target_remaining_days=(day(maturity)-day(settle)).days)
        references.append(dict(id=row['id'], expected=result, price_absolute=1e-8, accrued_absolute=1e-10,
            rate_absolute=1e-10, basis_points_absolute=1e-6, money_absolute=1e-6, cash_absolute=1e-20))
        return row

    for comp in ['SemiAnnual', 'Continuous', 'Simple', 'Annual', 'Quarterly', 'Monthly', 'Daily']:
        add('linear.'+comp, comp=comp)
        add('negative.'+comp, comp=comp, sign=-1, spread=-7.512345)
    add('decimal', unit='DecimalRate', spread=0.00123456789)
    add('zero', spread=0)
    add('exact', selection='ExactMaturity')
    add('stepped', t=target(stepped=True, delay=3), quantity='-15000')
    add('zero_coupon', t=target(zero=True), sign=-1)
    add('ex_coupon', t=target(settle='2024-12-27', ex=True))
    add('delayed_completed', t=target(settle='2025-01-02', delay=3))
    for state in ['Include','Exclude']:
        t=target(settle='2025-01-01'); t['on_payment']=state
        add('on_payment.'+state, t=t, quantity='0' if state=='Exclude' else '5000')
    for end in ['2025-01-01','2029-01-01']:
        add('endpoint.'+end, t=target(end=end), endpoints=['2025-01-01','2029-01-01','2030-01-01'])
    for currency in ['EUR','GBP']:
        t=target();t['terms']['currency']=currency
        row=add(currency,t=t)
        for b in cases[-1]['request']['operation']['benchmarks']:b['quote']['helper']['terms']['currency']=currency
    source=copy.deepcopy(cases[0])
    def bad(name, mutate, error='invalid_input'):
        row=copy.deepcopy(source);row['id']='gov_inverse.failure.'+name;row['expected_error']=error
        mutate(row['request']['operation'], row['request']['inputs']);cases.append(row)
    bad('unit',lambda o,i:o['quote']['spread'].update(unit='PercentRate'))
    bad('spread_domain',lambda o,i:o['quote']['spread'].update(value=10001))
    bad('quote_version',lambda o,i:o['quote'].update(quote_version=0))
    bad('target_version',lambda o,i:o['target']['target']['security'].update(version=0))
    bad('empty_benchmarks',lambda o,i:o.update(benchmarks=[]))
    bad('exact_count',lambda o,i:o['settings']['government'].update(selection='ExactMaturity'))
    bad('benchmark_id',lambda o,i:o['benchmarks'][0]['quote'].update(benchmark_id='other'))
    bad('duplicate',lambda o,i:o['benchmarks'].append(copy.deepcopy(o['benchmarks'][0])))
    bad('currency',lambda o,i:o['benchmarks'][0]['quote']['helper']['terms'].update(currency='EUR'))
    bad('settlement',lambda o,i:o['benchmarks'][0]['quote']['helper'].update(settlement='2024-06-02'))
    bad('entitlement',lambda o,i:o['benchmarks'][0]['quote']['helper'].update(on_payment='Include'))
    bad('reference_future',lambda o,i:o.update(reference_date='2024-06-02'))
    bad('trade_future',lambda o,i:o['target']['target'].update(trade_date='2024-06-02'))
    bad('lag',lambda o,i:o['target']['target']['terms'].update(settlement_lag=dict(CalendarDays=2)))
    bad('position_currency',lambda o,i:o['position'].update(currency='EUR'))
    bad('position_unit',lambda o,i:o['position'].update(quantity_unit='contracts'))
    bad('position_reference',lambda o,i:o['position'].update(reference_quantity='100'))
    bad('numeric_quantity',lambda o,i:o['position'].update(quantity=15000))
    bad('numeric_face',lambda o,i:o['target']['target']['terms'].update(face_value=5000))
    bad('missing_calendar',lambda o,i:i.update(calendars=[]),'missing_input')
    bad('wrong_calendar',lambda o,i:o['target']['calendars']['payment'].update(version=2),'missing_input')
    bad('extra_target_price',lambda o,i:o['target']['target'].update(observed_price=100))
    bad('cached_benchmark',lambda o,i:o['benchmarks'][0]['quote'].update(cached_yield=0.04))
    bad('missing_spread_bound',lambda o,i:o['settings'].pop('decimal_spread_tolerance'))
    bad('bad_spread_bound',lambda o,i:o['settings'].update(decimal_spread_tolerance=0))
    bad('unselected_no_root',lambda o,i:o['benchmarks'][0]['quote']['helper']['quote'].update(value=0.01),'no_root')
    bad('unselected_pillar',lambda o,i:o['benchmarks'][0]['quote']['helper'].update(pillar='2029-01-01'))
    bad('target_no_root',lambda o,i:o['quote']['spread'].update(value=9000),'no_root')
    bad('inverse_work',lambda o,i:o['settings']['government']['inverse'].update(max_iterations=0))
    bad('inverse_limit',lambda o,i:o['settings']['government']['inverse'].update(max_iterations=1),'no_convergence')
    bad('icma',lambda o,i:o['settings']['government']['convention'].update(day_count='ActActIcma'),'unsupported')
    bad('bad_profile',lambda o,i:o['target']['target']['terms']['profiles']['coupon'][0].update(end='2026-01-01'))
    bad('unknown_policy',lambda o,i:o['settings']['government'].update(selection='Nearest'))
    def outside(o,i):
        t=o['target']['target']['terms'];t['schedule']['end_date']='2031-01-01'
        for profile in t['profiles'].values():profile[-1]['end']='2031-01-01'
    bad('outside',outside)
    def too_many(o,i):
        o['benchmarks']=[copy.deepcopy(o['benchmarks'][0]) for _ in range(129)]
    bad('129_benchmarks',too_many,'resource_limit')
    return (dict(schema_version=1, certified=False, qualification='Synthetic supplied requests; independent finance is separate.', cases=cases),
            dict(schema_version=1, certified=False, qualification='Independent standard-library Decimal60 before candidate execution; no human/model/library certificate.', cases=references))


def main():
    parser=argparse.ArgumentParser();parser.add_argument('--write',action='store_true');args=parser.parse_args()
    outputs=build()
    for name, document in zip(['government-inverse.json','government-inverse-reference.json'], outputs):
        path=ROOT/'tests/fixtures/library'/name
        raw=(json.dumps(document,sort_keys=True,separators=(',',':'))+'\n').encode()
        if args.write:
            if path.exists():raise ValueError('Refusing to overwrite frozen control: '+name)
            path.write_bytes(raw)
        elif path.read_bytes()!=raw:raise ValueError('Independent control changed: '+name)
    print(json.dumps(dict(status='passed', requests=len(outputs[0]['cases']), independent_cases=len(outputs[1]['cases']), certified=False)))


if __name__=='__main__':
    main()
