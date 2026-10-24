"""Independent Decimal60 recovery scenarios and complete five-factor books.

Reads only original synthetic terms and independent reference primitives. It
never executes Convex or reads native captures. New controls are written once
before candidate execution; normal/CI execution checks exact bytes only.
"""
import argparse
from calendar import monthrange
from copy import deepcopy as clone
from datetime import date, timedelta
from decimal import Decimal as D, getcontext
import hashlib
import json
from pathlib import Path

import cash_basis_hedge_reference as cash
import cash_basis_hedge_book_reference as book
import graph_hedge_reference as graph

getcontext().prec = 60
ROOT = Path(__file__).resolve().parents[2]
FOLDER = ROOT / 'tests/fixtures/library'
KINDS = ['zero', 'hazard', 'basis', 'unrelated', 'recovery']
SOURCES = book.SOURCES + ['tests/scripts/cash_basis_hedge_book_reference.py',
                         'tests/scripts/recovery_hedge_reference.py']
SNAPSHOT = 'recovery-hedge.synthetic.snapshot.1'
MOVES = list(map(D, ['0', '.0001', '-.0001', '.00005', '-.00005']))


def cds_price(t, context, holidays, discount, hazard, recovery):
    """Full dated premium, half-day default accrual, protection and pending cash."""
    ref = date.fromisoformat(context['valuation_date'])
    step = date.fromisoformat(context['step_in_date'])
    settle = date.fromisoformat(context['cash_settlement_date'])
    first = date.fromisoformat(t['premium_schedule']['start_date'])
    last = date.fromisoformat(t['premium_schedule']['end_date'])
    denominator = D(360 if t['premium_day_count'] == 'Act360' else 365)
    n = D(t['notional']); coupon = D(t['coupon_decimal'])
    sign = D(1 if t['side'] == 'ProtectionBuyer' else -1)
    k = (discount + hazard) / 365
    def primitive(a, b):
        if k == 0:
            return b-a, (b*b-a*a)/2
        ea = (-k*a).exp(); eb = (-k*b).exp()
        return (ea-eb)/k, ((a+1/k)*ea-(b+1/k)*eb)/k
    def df(on): return (-discount*D((on-ref).days)/365).exp()
    def following(on):
        while on.weekday() in [5, 6] or str(on) in holidays:
            on += timedelta(days=1)
        return on
    grid = [first]; i = 1
    while grid[-1] < last:
        key = first.year*12 + first.month-1 + 3*i
        grid.append(min(last, date(key//12, key%12+1,
                                  min(first.day, monthrange(key//12, key%12+1)[1]))))
        i += 1
        if i > 4096: raise ValueError('Independent premium bound exhausted')
    rows = []; scheduled = default = D(0); start = first; accrued = D(0)
    protection_start = max(D((date.fromisoformat(t['protection_start_date'])-ref).days-1),
                           D((step-ref).days-1), D(0))
    for end in grid[1:]:
        payment = following(end)
        actual_end = end + timedelta(days=1) if end == last else payment
        if start <= step < actual_end: accrued = D((step-start).days)/denominator
        if actual_end > step:
            origin = D((start-ref).days-1); stop = D((actual_end-ref).days-1)
            yf = D((actual_end-start).days)/denominator
            survival = (-hazard*stop/365).exp()
            s = yf*survival*df(payment)
            i0, i1 = primitive(max(origin, protection_start), stop)
            ad = hazard/365*(i1+(-origin+D('.5'))*i0)/denominator if t['pay_accrual_on_default'] else D(0)
            scheduled += s; default += ad
            rows.append(dict(accrual_start=str(start), accrual_end=str(actual_end),
                payment_date=str(payment), year_fraction=str(yf), survival_probability=str(survival),
                conditional_coupon_amount=str(-sign*n*coupon*yf), scheduled_pv=str(-sign*n*coupon*s),
                accrual_on_default_pv=str(-sign*n*coupon*ad)))
        start = actual_end
    i0, _ = primitive(protection_start, D((last-ref).days))
    protection = sign*n*(1-recovery)*hazard/365*i0
    premium = -sign*n*coupon*(scheduled+default)
    clean_annuity = scheduled+default-accrued*df(settle)
    initial = D(0)
    if context['initial_payment_state'] == 'Pending':
        initial = -sign*n*(D(t['initial_clean_upfront_fraction'])-coupon*accrued)*df(date.fromisoformat(t['cash_settlement_date']))
    clean = ((protection/sign/n)-coupon*clean_annuity)/df(settle)
    dirty = clean-coupon*accrued
    return dict(pv=str(protection+premium+initial), protection=str(protection), premium=str(premium),
        initial=str(initial), premiums=rows, accrued_days=str(accrued*denominator), accrued_year_fraction=str(accrued),
        accrued_premium=str(n*coupon*accrued), dirty_risky_annuity=str(scheduled+default),
        clean_risky_annuity=str(clean_annuity), par_spread_decimal=str((protection/sign/n)/clean_annuity),
        clean_upfront_fraction=str(clean), dirty_upfront_fraction=str(dirty),
        cash_settlement_amount=str(n*dirty), clean_price_per100=str((1-clean)*100))


def bisect(fn, target, lo, hi):
    a = fn(lo)-target; b = fn(hi)-target
    if a*b > 0: raise ValueError('Independent root not bracketed')
    for _ in range(180):
        mid = (lo+hi)/2; value = fn(mid)-target
        if value == 0: return mid
        if a*value > 0: lo = mid; a = value
        else: hi = mid
    return (lo+hi)/2


def financial(req, discount=D('.032'), recovery=D('.4'), hazard=D('.02'), rebuilt=False):
    op = req['operation']; is_cash = op['kind'] == 'cash_basis_hedge_risk'
    hedge = op['input']['hedge'] if is_cash else op['input']
    helper = op['security']['helper'] if is_cash else None
    if is_cash:
        holidays = next(c['holidays'] for c in req['inputs']['calendars'] if c['pin'] == op['security']['calendars']['payment'])
        projected = cash.project(helper, holidays)
    else:
        product = op['product']; kind = product['kind']
        history = {(x['index_id'], date.fromisoformat(x['observation_date'])): D(x['rate']) for x in req['inputs']['fixings']}
        if kind == 'cds':
            holidays = next(c['holidays'] for c in req['inputs']['calendars'] if c['pin'] == product['terms']['premium_calendar'])
    def price(d, h, s, p, r):
        if is_cash: return cash.value(helper, projected, d, h, s, r)
        if kind == 'cds': return cds_price(product['terms'], product['context'], holidays, d, h, r)
        if kind == 'swap': return graph.swap_price(product['terms'], d, p, history)
        return graph.future_price(product['input'], p, history)
    factors = []
    for f in hedge['factors']:
        coordinate = f['key']['coordinate']; axis = coordinate['kind']
        if axis == 'zero' and coordinate['curve']['id'] == 'P.3M': axis = 'unrelated'
        amounts = []
        for move in MOVES:
            d = discount + (move if axis == 'zero' else 0)
            h = hazard + (move if axis == 'hazard' else 0)
            s = D('.0025') + (move if axis == 'basis' else 0)
            p = D('.041') + (move if axis == 'unrelated' else 0)
            r = recovery + (move if axis == 'recovery' else 0)
            if rebuilt:
                if op['graph']['credit']:
                    calibration = op['graph']['credit'][0]['helpers'][0]
                    calholidays = next(c['holidays'] for c in req['inputs']['calendars'] if c['pin'] == calibration['terms']['premium_calendar'])
                    target = D(str(calibration['quote']['value']))/10000
                    h = bisect(lambda x: D(cds_price(calibration['terms'], calibration['context'], calholidays, d, x, r)['par_spread_decimal']), target, D(0), D('.1'))
                if op['graph']['cash']:
                    ch = op['graph']['cash'][0]['recipe']['helpers'][0]['input']
                    cholidays = next(c['holidays'] for c in req['inputs']['calendars'] if c['pin'] == op['graph']['cash'][0]['calendars'][0]['pins']['payment'])
                    cp = cash.project(ch, cholidays); target = D(str(ch['quote']['value']))*D(ch['terms']['face_value'])/100
                    s = bisect(lambda x: D(cash.value(ch, cp, d, h, x, r)['present_value']), target, D('-.05'), D('.1'))
            amounts.append(price(d, h, s, p, r))
        pv = [D(v['present_value' if is_cash else 'pv']) for v in amounts]
        unit = D(1) if axis in ['recovery', 'hazard'] else D('.0001')
        factors.append(dict(valuations=amounts, prices=list(map(str, pv)),
            value=str((pv[1]-pv[2])/(2*MOVES[1])*unit), half_value=str((pv[3]-pv[4])/MOVES[1]*unit)))
    result = dict(factors=factors)
    if is_cash:
        v = factors[0]['valuations'][0]; native, icma = cash.quote(helper, projected, D(v['dirty_per100']))
        result.update(cash=projected, valuation=v, observed=cash.quote(helper, projected), model_quote=str(native), model_icma=icma)
    return result


def generate():
    columns, _, templates = book.generate()
    factors = [cash.factor(k) for k in KINDS]
    cases = []; answers = []
    def add(id, req, expected=None, error=None):
        cases.append(dict(id=id, request=clone(req), expected_error=error))
        if expected is not None: answers.append(dict(id=id, expected=expected))
    for row in columns['cases']:
        row['id'] = row['id'].replace('cash_basis_hedge.book', 'recovery_hedge.book')
        op = row['request']['operation']; hedge = op['input']['hedge'] if 'hedge' in op['input'] else op['input']
        hedge.update(factors=clone(factors), max_factor_evaluations=25, snapshot_id=SNAPSHOT)
        hedge['identity']['id'] = row['id']
        add(row['id'], row['request'], financial(row['request']))
    base = clone(cases[1]['request'])
    for side in ['ProtectionBuyer', 'ProtectionSeller']:
        for label, discount in [('positive', D('.032')), ('negative', D('-.005'))]:
            for cash_state in ['NotContracted', 'Pending']:
                r = clone(base); op = r['operation']; op['product']['terms']['side'] = side
                op['product']['context']['initial_payment_state'] = cash_state
                op['product']['terms']['initial_clean_upfront_fraction'] = '.015' if cash_state == 'Pending' else None
                r['inputs']['curves'][0] = cash.curve('D.OIS', discount, '2026-10-07', op['product']['terms']['reference'], cash.MODEL)
                id = 'recovery_hedge.cds.'+'.'.join([side, label, cash_state])
                add(id, r, financial(r, discount))
    for recovery in [D(0), D(1)]:
        r = clone(base); op = r['operation']; op['product']['terms']['reference']['recovery_fraction'] = float(recovery)
        next(c for c in r['inputs']['curves'] if c['key']['id'] == 'H.ACME')['role']['recovery'] = float(recovery)
        op['input']['factors'] = clone(factors[:4]); op['input']['max_factor_evaluations'] = 20
        add('recovery_hedge.cds.endpoint.'+str(recovery), r, financial(r, recovery=recovery))
        n = clone(r); n['operation']['input']['factors'] = [clone(factors[4])]; n['operation']['input']['max_factor_evaluations'] = 5
        add('recovery_hedge.invalid.endpoint_bump.'+str(recovery), n, error='invalid_input')
    for label in ['supplied', 'calibrated']:
        for family in ['cash', 'cds']:
            r = clone(cases[0 if family == 'cash' else 1]['request']); op = r['operation']
            hedge = op['input']['hedge'] if family == 'cash' else op['input']
            hedge['factors'] = [clone(factors[4])]; hedge['factors'][0]['key']['response'] = 'rebuild_dependents'; hedge['max_factor_evaluations'] = 5
            cash_req = clone(cases[0]['request']); cop = cash_req['operation']; h = clone(cop['security']['helper'])
            expected = financial(cash_req); h['quote_kind'] = dict(kind='DirtyPrice'); h['quote'] = dict(value=float(expected['valuation']['dirty_per100']), unit='PricePer100'); h['tolerance'] = 1e-12
            basis = clone(next(c for c in r['inputs']['curves'] if c['key']['id'] == 'B.ACME'))
            r['inputs']['curves'] = [c for c in r['inputs']['curves'] if c['key']['id'] != 'B.ACME']
            basis.update(calibration='Piecewise', source=dict(Helpers=[h['id']]), dependencies=[cop['input']['model']['discount'], cop['input']['model']['survival']])
            op['graph']['cash'] = [dict(recipe=dict(curve=basis, pillars=[h['pillar']], helpers=[dict(kind='Bond', input=h)], bootstrap=dict(lower_parameter=-.05, upper_parameter=.1, max_iterations=128), basis_model=clone(cop['input']['model']), overlap='Reject'), calendars=[dict(kind='bond', pins=clone(cop['security']['calendars']))])]
            if family == 'cash': op['input']['credit']['basis']['dated_spreads'][-1][0] = h['pillar']; op['input']['credit']['cds']['premium_schedule']['end_date'] = '2027-09-20'
            if label == 'calibrated':
                credit = clone(next(c for c in r['inputs']['curves'] if c['key']['id'] == 'H.ACME'))
                r['inputs']['curves'] = [c for c in r['inputs']['curves'] if c['key']['id'] != 'H.ACME']
                product = clone(base['operation']['product']); t = product['terms']; t['premium_schedule']['end_date'] = '2029-12-20'
                holidays = next(c['holidays'] for c in r['inputs']['calendars'] if c['pin'] == t['premium_calendar'])
                running = D(cds_price(t, product['context'], holidays, D('.032'), D('.02'), D('.4'))['par_spread_decimal'])*10000
                credit.update(calibration='Piecewise', source=dict(Helpers=['EXPLICIT.FULL.CDS.HELPER']), dependencies=[t['discount_curve']])
                op['graph']['credit'] = [dict(curve=credit, pillars=[t['premium_schedule']['end_date']], helpers=[dict(id='EXPLICIT.FULL.CDS.HELPER', quote_version=2, pillar=t['premium_schedule']['end_date'], terms=t, context=product['context'], quote_kind='RunningSpread', quote=dict(value=float(running), unit='BasisPoints'), tolerance=1e-10)], bootstrap=dict(lower_hazard=0.0, upper_hazard=.1, max_iterations=128))]
            id = 'recovery_hedge.rebuilt.'+label+'.'+family
            add(id, r, financial(r, rebuilt=True))
    for label, mutate in [
        ('entity', lambda o: o['input']['factors'][4]['key']['factor']['target'].update(entity_id='OTHER')),
        ('role', lambda o: o['product']['terms']['reference'].update(entity_id='OTHER')),
        ('seniority', lambda o: o['product']['terms']['reference'].update(seniority='SeniorSecured')),
        ('baseline_recovery', lambda o: o['product']['terms']['reference'].update(recovery_fraction=.3)),
        ('calendar_version', lambda o: o['product']['terms']['premium_calendar'].update(version=2)),
        ('native_quantity', lambda o: o['input']['reference_position'].update(reference_quantity='100')),
        ('factor_work', lambda o: o['input'].update(max_factor_evaluations=24)),
        ('duplicate_factor', lambda o: o['input']['factors'].__setitem__(4, clone(o['input']['factors'][0]))),
    ]:
        r = clone(base); mutate(r['operation']); add('recovery_hedge.invalid.'+label, r, error='invalid_input' if label != 'calendar_version' else 'missing_input')
    values = [[D(f['value']) for f in row['expected']['factors']] for row in answers[:4]]
    current = [D(0)]*5; pending = current[:]; refs = list(map(D, ['5000', '1000000', '1000000', '1']))
    qs = [('10000', '-2000'), ('200000', '50000'), ('100000', '50000')]
    for j, (q, pend) in enumerate(qs):
        for i, value in enumerate(values[j]): current[i] += value*D(q)/refs[j]; pending[i] += value*D(pend)/refs[j]
    before = [x+y for x, y in zip(current, pending)]; trades = list(map(D, ['-.6', '.08', '-.2', '2']))
    after = [before[i]+sum((trades[j]*values[j][i] for j in range(4)), D(0)) for i in range(5)]
    for template in templates['cases']:
        fx = D(1) if template['reporting_currency'] == 'USD' else D('.9')
        template['id'] = template['id'].replace('cash_basis_hedge', 'recovery_hedge'); template['snapshot_id'] = SNAPSHOT
        for rows in [template['holdings'], template['candidates']]:
            for row in rows: row['column'] = row['column'].replace('cash_basis_hedge.book', 'recovery_hedge.book')
        if template['fx']: template['fx']['snapshot_id'] = SNAPSHOT
        template['factors'] = []
        reference = base['operation']['product']['terms']['reference']
        for i, f in enumerate(factors):
            template['factors'].append(dict(key=clone(f['key']), risk_settings=clone(f['settings']), credit=clone(reference) if i in [1,2,4] else None, scale=1000000.0 if i in [1,4] else 100.0, weight=1.0, target=float(after[i]*fx), lower=-1000000.0 if i in [1,4] else -1000.0, upper=1000000.0 if i in [1,4] else 1000.0))
        template['expected'].update(current=list(map(str, (x*fx for x in current))), pending=list(map(str, (x*fx for x in pending))), before=list(map(str, (x*fx for x in before))), after=list(map(str, (x*fx for x in after))))
    provenance = dict(certified=False, source_reads={p: hashlib.sha256((ROOT/p).read_bytes()).hexdigest() for p in SOURCES}, qualification='Independent full Decimal60 cash/CDS/IRS/futures five-factor columns and mixed books. Actual CDS premium calendar, both sides, pending initial cash, recovery endpoint/domain and supplied/calibrated dependent recipes. No native captures, original oracle changes, independent human approval or production/order certificate.')
    return dict(schema_version=1, **provenance, cases=cases), dict(**provenance, tolerances=dict(pv='1e-7', cash='1e-8', rate_risk='1e-6', hazard_risk='1e-5', recovery_risk='1e-5'), cases=answers), dict(**provenance, tolerances=templates['tolerances'], cases=templates['cases'])


if __name__ == '__main__':
    parser = argparse.ArgumentParser(); parser.add_argument('--write', action='store_true'); args = parser.parse_args()
    for name, document in zip(['recovery-hedge-api.json', 'recovery-hedge-reference.json', 'recovery-hedge-problems.json'], generate()):
        path = FOLDER/name; raw = (json.dumps(document, indent=2, default=str)+'\n').encode()
        if args.write:
            if path.exists(): raise ValueError('Refusing to overwrite original controls')
            path.write_bytes(raw)
        elif path.read_bytes() != raw: raise ValueError('Independent controls differ: '+name)
        print(json.dumps(dict(status='passed', path=name, cases=len(document['cases']), certified=False)))
