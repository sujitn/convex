"""Independent Decimal60 complete legal-path controls; never execute Convex."""
import argparse
from copy import deepcopy
from datetime import timedelta
from decimal import Decimal as D, getcontext
import json
from pathlib import Path

from icma_government_reference import security, INVERSE
from icma_reference import PER_YEAR, coupon, day, discount, fraction, offset, profile
from workout_quotation_reference import rounding

getcontext().prec = 60
ROOT = Path(__file__).resolve().parents[2]
PATHS = dict(max_dates=16384, max_reference_evaluations=16777216,
             max_cash_evaluations=67108864)


def adjust(value, rule, holidays):
    if rule == 'Unadjusted': return value
    sign = -1 if rule == 'Preceding' else 1
    result = value
    while result.weekday() >= 5 or str(result) in holidays:
        result += timedelta(days=sign)
    if rule == 'ModifiedFollowing' and result.month != value.month:
        return adjust(value, 'Preceding', holidays)
    return result


def calendar(request, role):
    pin = request['operation']['input']['security']['bond']['calendars'][role]
    return next(c['holidays'] for c in request['inputs']['calendars'] if c['pin'] == pin)


def original(request, boundaries):
    op = request['operation']; t = op['input']['security']['bond']['terms']
    mesh = op['input']['reference_context']['periods']; f = PER_YEAR[t['schedule']['frequency']]
    settle = day(op['context']['settlement']); on_payment = op['context']['on_payment']
    holidays = calendar(request, 'payment'); flows = []; accrued = D(0)
    pay = lambda d: adjust(offset(d, t['dates']['payment_delay'], holidays),
                           t['dates']['payment_adjustment'], holidays)
    owned = lambda d: d > settle or d == settle and on_payment == 'Include'
    for a, b in zip(boundaries, boundaries[1:]):
        payment = pay(b); full = coupon(t, a, b, mesh, f)
        if not owned(payment): continue
        partial = coupon(t, a, min(settle, b), mesh, f) if a < settle and payment > settle else D(0)
        entitled = True
        ex = t['dates']['ex_coupon']
        if ex:
            anchor = payment if ex['anchor'] == 'PaymentDate' else b
            ex_date = offset(anchor, ex['offset'], holidays)
            excluded = settle >= ex_date if ex['boundary'] == 'OnDate' else settle > ex_date
            if excluded:
                entitled = False
                partial = partial-full if t['ex_coupon_accrual'] == 'SubtractCoupon' else D(0)
        accrued += rounding(partial, t['rounding'])
        if entitled:
            flows.append(dict(effective=str(b), date=str(payment), kind='coupon',
                              amount=str(rounding(full, t['rounding']))))
    rows = t['profiles']['notional']
    for i, row in enumerate(rows):
        effective = day(row['end']); payment = pay(effective)
        if not owned(payment): continue
        after = D(rows[i+1]['value']) if i+1 < len(rows) else D(0)
        repayment = D(row['value'])-after
        price = next((p for p in t['principal_prices'] if day(p['effective_date']) == effective), None)
        if price:
            allocation = D(price['amount']) if price['amount'] is not None else repayment
            amount = allocation*D(price['multiplier']); repayment -= allocation
            flows.append(dict(effective=str(effective), date=str(payment), kind='principal',
                              amount=str(rounding(amount, t['rounding']))))
        if repayment:
            amount = repayment*(D(t['redemption_multiplier']) if i+1 == len(rows) else 1)
            flows.append(dict(effective=str(effective), date=str(payment), kind='principal',
                              amount=str(rounding(amount, t['rounding']))))
    return flows, accrued


def legal_paths(request):
    op = request['operation']; t = op['input']['security']['bond']['terms']; c = op['context']
    notice = c['notice']
    if notice['kind'] == 'notified':
        return [dict(kind='exercise', provision=notice['provision'], date=notice['exercise'])], 1, True
    result = [dict(kind='maturity')]; examined = 0
    holidays = calendar(request, 'exercise')
    for i, p in enumerate(t['exercise']):
        s = p['style']
        if s['style'] == 'european': dates = [day(s['date'])]
        elif s['style'] == 'bermudan': dates = list(map(day, s['dates']))
        else:
            first = max(day(s['start']), day(c['settlement'])+timedelta(days=1))
            last = min(day(s['end']), day(t['schedule']['end_date'])-timedelta(days=1))
            dates = [first+timedelta(days=n) for n in range((last-first).days+1)]
        for d in dates:
            if not day(c['settlement']) < d < day(t['schedule']['end_date']): continue
            examined += 1
            if p['protection_end'] and d < day(p['protection_end']): continue
            if offset(d, p['notice_offset'], holidays) < day(notice['earliest_notice']): continue
            result.append(dict(kind='exercise', provision=i, date=str(d)))
    return result, examined, False


def path_cash(request, boundaries, selection):
    op = request['operation']; t = op['input']['security']['bond']['terms']
    source, accrued = original(request, boundaries); cash = deepcopy(source); exercise = None
    if selection['kind'] == 'exercise':
        d = day(selection['date']); p = t['exercise'][selection['provision']]
        holidays = calendar(request, 'exercise')
        payment = adjust(offset(d, p['settlement_lag'], holidays), p['settlement_adjustment'], holidays)
        cash = [r for r in cash if day(r['effective']) <= d]
        outstanding = profile(t, 'notional', d)
        redemption = outstanding*D(p['price_per_100'])/100
        partial = D(0)
        if p['accrued'] == 'pay_accrued':
            for a, b in zip(boundaries, boundaries[1:]):
                if a < d < b:
                    partial = rounding(coupon(t, a, d, op['input']['reference_context']['periods'],
                                               PER_YEAR[t['schedule']['frequency']]), t['rounding'])
        total = rounding(redemption+partial, t['rounding'])
        if partial:
            cash.append(dict(effective=str(d), date=str(payment), kind='coupon', amount=str(partial)))
        cash.append(dict(effective=str(d), date=str(payment), kind='principal', amount=str(total-partial)))
        exercise = dict(dates=dict(notice=str(offset(d, p['notice_offset'], holidays)),
                                   exercise=str(d), settlement=str(payment)), right=p['right'],
                        outstanding_principal=str(outstanding), redemption=str(redemption),
                        accrued_cash=str(partial), settlement_cash=str(total))
    cash.sort(key=lambda r: (r['date'], r['kind'], D(r['amount'])))
    source.sort(key=lambda r: (r['date'], r['kind'], D(r['amount'])))
    strip = lambda rows: [{k: v for k, v in r.items() if k != 'effective'} for r in rows]
    return strip(cash), accrued, exercise, strip(source)


def value(request, boundaries, selection, rate=None):
    op = request['operation']; t = op['input']['security']['bond']['terms']; action = op['action']
    cash, accrued, exercise, source = path_cash(request, boundaries, selection)
    f = PER_YEAR[t['schedule']['frequency']]; mesh = op['input']['reference_context']['periods']
    settle = day(op['context']['settlement'])
    times = {r['date']: fraction(settle, day(r['date']), mesh, f) for r in cash}
    def pv(y):
        return sum((D(r['amount'])*discount(y, times[r['date']], op['convention']['compounding']) for r in cash), D(0))
    accrual = accrued/D(t['face_value'])*100
    if action['kind'] == 'worst':
        target = action['target']; observed_dirty = D(str(target['value']))+(accrual if target['kind'] == 'clean' else 0)
        lo, hi = map(lambda k: D(str(action['settings']['inverse'][k])), ['lower', 'upper'])
        if abs(pv(lo)/D(t['face_value'])*100-observed_dirty) < D('1e-40'): rate = lo
        else:
            for _ in range(180):
                mid = (lo+hi)/2
                if pv(mid)/D(t['face_value'])*100 > observed_dirty: lo = mid
                else: hi = mid
            rate = (lo+hi)/2
    else: rate = D(str(action['decimal_yield'])) if rate is None else rate
    present_value = pv(rate); dirty = present_value/D(t['face_value'])*100
    invoice_price = observed_dirty if action['kind'] == 'worst' else dirty
    return dict(selection=selection, cashflows=cash, source_cashflows=source, exercise=exercise,
                accrued_per100=str(accrual), dirty_per100=str(dirty), clean_per100=str(dirty-accrual),
                present_value=str(present_value), decimal_yield=str(rate),
                consideration=str(invoice_price*D(op['input']['position']['quantity'])/100),
                times={k: str(v) for k, v in times.items()},
                original_dirty_per100=str(invoice_price))


def make(label, options=None):
    options = options or {}
    s, b = security('ICMA.WORKOUT.'+label, **{'end':'2025-08-31', **options})
    template = deepcopy(json.loads((ROOT/'tests/fixtures/library/icma-qualified.json').read_bytes())['cases'][0]['request'])
    pins = {role: dict(id='ICMA.WORKOUT.'+role.upper(), version=7+i)
            for i, role in enumerate(['payment', 'reset', 'observation', 'exercise'])}
    t = s['helper']['terms']; t['calendars'] = {role: p['id'] for role, p in pins.items()}
    template['inputs']['calendars'] = [dict(pin=pin, holidays=['2024-03-11'] if role == 'payment' else
                                           (['2024-06-18', '2024-10-16'] if role == 'exercise' else []),
                                           weekend_days=[5, 6]) for role, pin in pins.items()]
    settle = s['helper']['settlement']
    input = dict(security=dict(reference=s['security'], quote_revision=dict(id='ICMA.WORKOUT.QUOTE', version=23),
                               bond=dict(terms=t, calendars=pins)), reference_context=s['icma']['reference_context'],
                 quotation_policy=s['icma']['quotation_policy'], position=s['icma']['position'])
    c = dict(reference_date=settle, trade_date=settle, settlement=settle,
             on_payment=s['helper']['on_payment'], notice=dict(kind='unnotified', earliest_notice=settle))
    call = dict(right='call', style=dict(style='european', date='2024-03-05'), price_per_100='101.25',
                notice_offset=dict(BusinessDays=-2), settlement_lag=dict(CalendarDays=0),
                settlement_adjustment='Unadjusted', accrued='pay_accrued', protection_end=None)
    put = dict(right='put', style=dict(style='bermudan', dates=['2024-06-17', '2024-10-20']), price_per_100='99.5',
               notice_offset=dict(CalendarDays=-5), settlement_lag=dict(BusinessDays=5),
               settlement_adjustment='Following', accrued='pay_accrued', protection_end=None)
    american = deepcopy(call); american.update(style=dict(style='american', start='2024-06-20', end='2024-06-30'),
                                               protection_end='2024-06-22', settlement_lag=dict(BusinessDays=3),
                                               settlement_adjustment='Following')
    if day(settle) >= day('2024-03-05'):
        call['style']['date'] = '2024-06-17'; call['settlement_lag'] = dict(BusinessDays=5)
    if t['schedule']['end_date'] == '2024-05-31':
        call['style']['date'] = '2024-03-17'; put['style']['dates'] = ['2024-03-25', '2024-04-20']
        american.update(style=dict(style='american', start='2024-04-12', end='2024-04-15'), protection_end='2024-04-13')
    t['exercise'] = [call, put, american]
    template['operation'] = dict(kind='icma_workout', input=input, convention=s['icma']['convention'], context=c,
                                 action=dict(kind='price_paths', decimal_yield=0.04125, settings=PATHS))
    return template, b


def generate():
    cases = []; references = []
    def add(identifier, request, boundary, error=None):
        request = deepcopy(request)
        cases.append(dict(id='icma_workout.'+identifier, request=request, expected_error=error))
        if error is not None: return
        selections, examined, forced = legal_paths(request)
        quotes = [value(request, boundary, s) for s in selections]
        worst = request['operation']['action']['kind'] == 'worst'
        eligible = [i for i, q in enumerate(quotes) if forced or q['exercise'] is None or q['exercise']['right'] == 'call']
        selected = min(eligible, key=lambda i: (D(quotes[i]['decimal_yield']), i)) if worst else None
        references.append(dict(id='icma_workout.'+identifier, expected=dict(quotes=quotes,
            examined_dates=examined, forced_by_notice=forced, selected_index=selected)))
    variants = [
        ('regular', {}), ('short_front', dict(start='2023-10-12', stub='ShortFirst')),
        ('long_front', dict(start='2023-10-12', stub='LongFirst')),
        ('short_back', dict(end='2025-10-12')),
        ('long_back', dict(end='2025-10-12', stub='LongLast')),
        ('annual', dict(frequency='Annual')), ('quarterly', dict(frequency='Quarterly')),
        ('monthly', dict(end='2024-05-31', start='2024-01-31', settle='2024-02-12', frequency='Monthly')),
        ('stepped', dict(stepped=True)), ('separate_accrual', dict(separate_accrual=True)),
        ('delayed_claim', dict(settle='2024-02-29', stepped=True)),
        ('payment_include', dict(settle='2024-03-12', on_payment='Include')),
        ('payment_exclude', dict(settle='2024-03-12', on_payment='Exclude')),
        ('ex_coupon', dict(settle='2024-03-06', ex=True)),
    ]
    # The independent control domains and solver/quote bounds are fixed here,
    # before candidate valuation; raw controls are never overwritten on failure.
    for label, options in variants:
        request, boundary = make(label, options); op = request['operation']; t = op['input']['security']['bond']['terms']
        if label == 'ex_coupon': t['exercise'][0]['accrued'] = 'included_in_price'
        compounding = ['SemiAnnual', 'Continuous', 'Simple', 'Monthly'] if label == 'regular' else ['Continuous' if label in ['long_front', 'monthly'] else 'Quarterly']
        rates = ['0.04125', '-0.0075'] if label in ['regular', 'quarterly', 'ex_coupon'] else ['0.035']
        for comp in compounding:
            op['convention']['compounding'] = comp
            for y in rates:
                op['action'] = dict(kind='price_paths', decimal_yield=float(y), settings=PATHS)
                id = label+'.'+comp+'.'+y
                add(id+'.price_paths', request, boundary)
                target = value(request, boundary, dict(kind='maturity'))['clean_per100']
                op['action'] = dict(kind='worst', target=dict(kind='clean', value=float(target)),
                                    settings=dict(inverse=INVERSE, paths=PATHS))
                add(id+'.worst', request, boundary)
    for label in ['rounded', 'principal_boundary', 'notice_filter', 'forced_call', 'forced_put']:
        r, b = make(label, dict(stepped=True)); op = r['operation']; t = op['input']['security']['bond']['terms']
        if label == 'rounded':
            t['rounding'] = dict(NearestEven=2); t['exercise'][0]['price_per_100'] = '101.251234'
        if label == 'principal_boundary':
            t['exercise'][0]['style']['date'] = '2024-10-15'
            t['exercise'][2]['style'].update(start='2024-11-20', end='2024-11-25')
            t['exercise'][2]['protection_end'] = '2024-11-22'
        if label == 'notice_filter':
            op['context']['notice']['earliest_notice'] = '2024-06-25'
        if label in ['forced_call', 'forced_put']:
            provision = 0 if label == 'forced_call' else 1
            exercise = t['exercise'][provision]['style'].get('date', '2024-06-17')
            op['context'].update(reference_date='2024-01-10', notice=dict(kind='notified', provision=provision,
                exercise=exercise, notice_date='2024-01-10'))
        for action in ['price_paths', 'worst']:
            if action == 'worst':
                op['action'] = dict(kind='worst', target=dict(kind='dirty', value=103.125), settings=dict(inverse=INVERSE, paths=PATHS))
            add(label+'.'+action, r, b)
    r, b = make('tie'); op = r['operation']; t = op['input']['security']['bond']['terms']
    t['profiles']['coupon'][0]['value'] = '0'; t['profiles']['spread'][0]['value'] = '0'; t['redemption_multiplier'] = '1'
    t['exercise'] = [dict(right=right, style=dict(style='european', date='2025-08-30'), price_per_100='100',
                          notice_offset=dict(CalendarDays=-1), settlement_lag=dict(CalendarDays=11),
                          settlement_adjustment='Following', accrued='included_in_price', protection_end=None)
                     for right in ['call', 'put']]
    # Maturity and both early-redemption paths carry identical positive cash at
    # the same payment date. Endpoint roots make the native numerical tie exact.
    op['action']['decimal_yield'] = 0.035
    add('tie.price_paths', r, b)
    price = value(r, b, dict(kind='maturity'))['dirty_per100']
    inverse = dict(INVERSE, lower=0.035)
    op['action'] = dict(kind='worst', target=dict(kind='dirty', value=float(price)), settings=dict(inverse=inverse, paths=PATHS))
    add('tie.worst', r, b)
    r, b = make('negative'); op = r['operation']
    def mutation(label, fn, error):
        c = deepcopy(r); fn(c['operation']); add('invalid.'+label, c, b, error)
    settings = lambda o: o['action']['settings']['paths'] if o['action']['kind'] == 'worst' else o['action']['settings']
    mutations = [
        ('empty_context', lambda o: o['input']['reference_context'].update(periods=[]), 'invalid_input'),
        ('stale_context', lambda o: o['convention'].update(reference_context=dict(o['convention']['reference_context'],version=99)), 'invalid_input'),
        ('stale_policy', lambda o: o['convention'].update(quotation_policy=dict(o['convention']['quotation_policy'],version=99)), 'invalid_input'),
        ('wrong_security', lambda o: o['input']['reference_context'].update(security=dict(o['input']['reference_context']['security'],version=99)), 'invalid_input'),
        ('zero_frequency', lambda o: o['convention'].update(frequency='Zero'), 'unsupported'),
        ('wrong_frequency', lambda o: o['convention'].update(frequency='Annual'), 'invalid_input'),
        ('overlap', lambda o: o['input']['reference_context']['periods'].insert(1, deepcopy(o['input']['reference_context']['periods'][0])), 'invalid_input'),
        ('gap', lambda o: o['input']['reference_context']['periods'].pop(2), 'invalid_input'),
        ('reference_work', lambda o: o['convention'].update(max_reference_evaluations=1), 'resource_limit'),
        ('total_reference_work', lambda o: settings(o).update(max_reference_evaluations=1), 'resource_limit'),
        ('total_cash_work', lambda o: settings(o).update(max_cash_evaluations=1), 'resource_limit'),
        ('date_work', lambda o: settings(o).update(max_dates=1), 'resource_limit'),
        ('zero_date_bound', lambda o: settings(o).update(max_dates=0), 'invalid_input'),
        ('high_date_bound', lambda o: settings(o).update(max_dates=16385), 'invalid_input'),
        ('high_reference_bound', lambda o: settings(o).update(max_reference_evaluations=16777217), 'invalid_input'),
        ('high_cash_bound', lambda o: settings(o).update(max_cash_evaluations=67108865), 'invalid_input'),
        ('wrong_currency', lambda o: o['input']['position'].update(currency='EUR'), 'invalid_input'),
        ('wrong_units', lambda o: o['input']['position'].update(quantity_unit='contracts'), 'invalid_input'),
        ('wrong_reference_quantity', lambda o: o['input']['position'].update(reference_quantity='700'), 'invalid_input'),
        ('stale_calendar', lambda o: o['input']['security']['bond']['calendars'].update(exercise=dict(o['input']['security']['bond']['calendars']['exercise'],version=99)), 'missing_input'),
        ('wrong_version', lambda o: o['convention'].update(schema_version=2), 'unsupported'),
        ('bad_purchase', lambda o: o['context'].update(trade_date='2024-01-09'), 'invalid_input'),
        ('bad_reference_date', lambda o: o['context'].update(reference_date='2024-01-11'), 'invalid_input'),
        ('early_notice_policy', lambda o: o['context']['notice'].update(earliest_notice='2024-01-09'), 'invalid_input'),
        ('late_forced_notice', lambda o: o['context'].update(reference_date='2024-03-05', trade_date='2024-03-05',settlement='2024-03-05', notice=dict(kind='notified',provision=1,exercise='2024-06-17',notice_date='2024-03-05')), 'invalid_input'),
        ('unsupported_style', lambda o: o['input']['security']['bond']['terms']['exercise'][0]['style'].update(style='make_whole'), 'invalid_input'),
        ('cached_paths', lambda o: o.update(prepared_cash=[]), 'invalid_input'),
        ('out_of_domain_yield', lambda o: o['action'].update(decimal_yield=10.1), 'invalid_input'),
    ]
    # The late-forced-notice control must actually be late under its own original
    # notice deadline, not merely early relative to a future contractual date.
    mutations = [m for m in mutations if m[0] != 'late_forced_notice']
    for action in ['price_paths', 'worst']:
        op['action'] = dict(kind='price_paths', decimal_yield=0.04125, settings=PATHS) if action == 'price_paths' else dict(kind='worst',target=dict(kind='dirty',value=103.125),settings=dict(inverse=INVERSE,paths=PATHS))
        for label, fn, error in mutations:
            if label == 'out_of_domain_yield' and action != 'price_paths': continue
            mutation(label+'.'+action, fn, error)
    op['action'] = dict(kind='worst', target=dict(kind='dirty', value=10000.0), settings=dict(inverse=INVERSE,paths=PATHS))
    add('invalid.no_root', r, b, 'no_root')
    op['action']['target']['value'] = 103.125; op['action']['settings']['inverse'] = dict(INVERSE,max_iterations=1)
    add('invalid.inverse_work', r, b, 'no_convergence')
    op['action']['settings']['inverse'] = dict(INVERSE, lower=0.035, upper=0.03500000000001)
    first = deepcopy(r); first['operation']['action'] = dict(kind='price_paths',decimal_yield=0.035,settings=PATHS)
    op['action']['target']['value'] = float(value(first,b,dict(kind='maturity'))['dirty_per100'])
    add('invalid.non_identification', r, b, 'non_unique')
    op['action']['settings']['inverse'] = INVERSE; op['action']['target']['value'] = 50.0
    op['input']['security']['bond']['terms']['profiles']['coupon'][0]['value'] = '-0.5'
    op['input']['security']['bond']['terms']['profiles']['spread'][0]['value'] = '0'
    add('invalid.mixed_sign', r, b, 'unsupported')
    r,b = make('late_put'); op=r['operation']; t=op['input']['security']['bond']['terms']
    t['exercise'][1]['style']['dates'] = ['2025-08-30'];t['exercise'][1]['settlement_lag']=dict(CalendarDays=366)
    op['input']['reference_context']['periods'] = [p for p in op['input']['reference_context']['periods'] if day(p['end'])<=day('2026-02-28')]
    add('invalid.unselected_put_coverage.price_paths',r,b,'invalid_input')
    op['action']=dict(kind='worst',target=dict(kind='dirty',value=10000.0),settings=dict(inverse=INVERSE,paths=PATHS))
    add('invalid.unselected_put_coverage_before_inverse.worst',r,b,'invalid_input')
    r,b=make('ex_ambiguous',dict(settle='2024-03-06',ex=True));op=r['operation'];t=op['input']['security']['bond']['terms']
    t['exercise'][0]['style']['date']='2024-03-07'
    add('invalid.ex_coupon_partial.price_paths',r,b,'unsupported')
    op['action']=dict(kind='worst',target=dict(kind='dirty',value=10000.0),settings=dict(inverse=INVERSE,paths=PATHS))
    add('invalid.ex_coupon_partial_before_inverse.worst',r,b,'unsupported')
    for label, notice in [
        ('unknown_provision',dict(kind='notified',provision=99,exercise='2024-06-17',notice_date='2024-01-10')),
        ('illegal_date',dict(kind='notified',provision=0,exercise='2024-03-06',notice_date='2024-01-10')),
        ('unknown_notice',dict(kind='notified',provision=0,exercise='2024-03-05',notice_date='2024-01-11')),
    ]:
        r,b=make('notice_error');r['operation']['context']['notice']=notice
        add('invalid.'+label,r,b,'invalid_input')
    requests=dict(schema_version=1,certified=False,qualification='Complete predeclared supplied ICMA legal-path engineering controls; no market/Street/model/host certificate.',cases=cases)
    controls=dict(precision=60,absolute_price_cash_invoice_tolerance='1e-8',absolute_yield_tolerance='1e-9',absolute_time_tolerance='1e-12',relative_tolerance='0',certified=False,cases=references)
    return requests,controls


if __name__ == '__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args()
    values=generate()
    for path,value in zip(['icma-workout.json','icma-workout-reference.json'],values):
        file=ROOT/'tests/fixtures/library'/path
        if a.write:
            if file.exists():raise ValueError('Refusing to overwrite predeclared controls: '+path)
            file.write_text(json.dumps(value,indent=2)+'\n')
        elif json.loads(file.read_bytes()) != value:raise ValueError('Original predeclared control changed: '+path)
    print(json.dumps(dict(requests=len(values[0]['cases']),successful_requests=len(values[1]['cases']),
                         independent_complete_paths=sum(len(c['expected']['quotes']) for c in values[1]['cases']),certified=False)))
