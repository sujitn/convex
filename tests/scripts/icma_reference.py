"""Independent Decimal60 supplied-context cash controls; never execute Convex."""
import argparse
import calendar
import copy
from datetime import date, timedelta
from decimal import Decimal as D, getcontext
import json
from pathlib import Path

getcontext().prec = 60
ROOT = Path(__file__).resolve().parents[2]
PER_YEAR = {'Annual': 1, 'SemiAnnual': 2, 'Quarterly': 4, 'Monthly': 12}

def day(s): return date.fromisoformat(s)
def roll(a, count, months, eom):
    n = a.year * 12 + a.month - 1 + count * months
    y, m = divmod(n, 12); m += 1
    last = calendar.monthrange(y, m)[1]
    return date(y, m, last if eom and a.day == calendar.monthrange(a.year, a.month)[1] else min(a.day, last))
def refs(anchor, f, eom):
    points = [roll(anchor, n, 12 // f, eom) for n in range(-5, 9)]
    return [dict(start=str(a), end=str(b)) for a, b in zip(points, points[1:])]
def fraction(a, b, periods, f):
    total = D(0); covered = 0
    for p in periods:
        l, r = max(a, day(p['start'])), min(b, day(p['end']))
        if r > l:
            n = (r-l).days; covered += n
            total += D(n) / (D(f) * D((day(p['end'])-day(p['start'])).days))
    if covered != (b-a).days: raise ValueError('Independent original context does not cover interval')
    return total
def offset(a, value, holidays):
    if 'CalendarDays' in value: return a + timedelta(days=value['CalendarDays'])
    n = value['BusinessDays']; sign = 1 if n >= 0 else -1
    for _ in range(abs(n)):
        a += timedelta(days=sign)
        while a.weekday() >= 5 or str(a) in holidays: a += timedelta(days=sign)
    return a
def following(a, holidays):
    while a.weekday() >= 5 or str(a) in holidays: a += timedelta(days=1)
    return a
def profile(t, name, a):
    return D(next(p['value'] for p in t['profiles'][name] if day(p['start']) <= a < day(p['end'])))
def coupon(t, a, b, mesh, f):
    cuts = {a, b}
    for rows in t['profiles'].values():
        cuts.update(day(p['start']) for p in rows if a < day(p['start']) < b)
    cuts = sorted(cuts); total = D(0)
    for l, r in zip(cuts, cuts[1:]):
        time = fraction(l, r, mesh, f) if t['day_count'] == 'ActActIcma' else D((r-l).days)/D(365)
        total += profile(t, 'notional', l) * (profile(t, 'coupon', l)+profile(t, 'spread', l)) * time
    return total
def discount(y, time, comp):
    if comp == 'Continuous': return (-y*time).exp()
    if comp == 'Simple': return 1/(1+y*time)
    f = D(365 if comp == 'Daily' else PER_YEAR[comp])
    return (-f*time*(1+y/f).ln()).exp()
def price_expected(request, boundaries, decimal_yield):
    op = request['operation']; t = op['input']['security']['bond']['terms']
    mesh = op['input']['reference_context']['periods']; f = PER_YEAR[t['schedule']['frequency']]
    spec = op['cash']; settle = day(spec['settlement'] if spec['kind'] == 'plain' else spec['context']['settlement'])
    entitlement = spec['on_payment'] if spec['kind'] == 'plain' else spec['context']['on_payment']
    holidays = request['inputs']['calendars'][0]['holidays']
    def pays(a): return following(offset(a, t['dates']['payment_delay'], holidays), holidays)
    flows = []; accrued = D(0)
    for a, b in zip(boundaries, boundaries[1:]):
        pay = pays(b)
        owned = pay > settle or (pay == settle and entitlement == 'Include')
        if owned:
            flows.append(dict(date=str(pay), kind='coupon', amount=str(coupon(t, a, b, mesh, f))))
        if a < settle < b: accrued += coupon(t, a, settle, mesh, f)
    p = t['profiles']['notional']; principal = []
    for i, row in enumerate(p):
        effective = day(row['end']); after = D(p[i+1]['value']) if i+1 < len(p) else D(0)
        amount = D(row['value'])-after
        if i+1 == len(p): amount *= D(t['redemption_multiplier'])
        principal.append(dict(effective=str(effective), date=str(pays(effective)), kind='principal', amount=str(amount)))
    if spec['kind'] == 'workout' and spec['selection']['kind'] == 'exercise':
        exercise = day(spec['selection']['date']); provision = t['exercise'][spec['selection']['provision']]
        flows = [c for c in flows if next(b for b in boundaries[1:] if str(pays(b)) == c['date']) <= exercise]
        principal = [c for c in principal if day(c['effective']) <= exercise]
        amount = profile(t, 'notional', exercise)
        partial = sum((coupon(t, a, exercise, mesh, f) for a,b in zip(boundaries,boundaries[1:]) if a < exercise < b),D(0))
        pay = following(offset(exercise, provision['settlement_lag'], holidays), holidays)
        flows.append(dict(date=str(pay), kind='principal', amount=str(amount*D(provision['price_per_100'])/100+partial)))
    flows += [{k:v for k,v in c.items() if k != 'effective'} for c in principal if day(c['date']) > settle or (day(c['date']) == settle and entitlement == 'Include')]
    flows.sort(key=lambda c:(c['date'],c['kind']))
    times = {c['date']: fraction(settle, day(c['date']), mesh, f) for c in flows}
    pv = sum((D(c['amount'])*discount(decimal_yield,times[c['date']],op['convention']['compounding']) for c in flows),D(0))
    dirty = pv/D(t['face_value'])*100; accr = accrued/D(t['face_value'])*100
    position = op['input']['position']; consideration = dirty*D(position['quantity'])/100
    return dict(cashflows=flows,times={k:str(v) for k,v in times.items()},present_value=str(pv),dirty_per100=str(dirty),clean_per100=str(dirty-accr),accrued_per100=str(accr),decimal_yield=str(decimal_yield),consideration=str(consideration))

def generate():
    template = json.loads((ROOT/'tests/fixtures/library/workouts.json').read_bytes())['cases'][0]['request']
    pin = lambda id, version=1:dict(id=id,version=version)
    variants = [
        ('regular','2023-08-31','2025-08-31','SemiAnnual','ShortLast',True,['2023-08-31','2024-02-29','2024-08-31','2025-02-28','2025-08-31']),
        ('short_front','2023-10-12','2025-08-31','SemiAnnual','ShortFirst',True,['2023-10-12','2024-02-29','2024-08-31','2025-02-28','2025-08-31']),
        ('long_front','2023-10-12','2025-08-31','SemiAnnual','LongFirst',True,['2023-10-12','2024-08-31','2025-02-28','2025-08-31']),
        ('short_back','2023-08-31','2025-10-12','SemiAnnual','ShortLast',True,['2023-08-31','2024-02-29','2024-08-31','2025-02-28','2025-08-31','2025-10-12']),
        ('long_back','2023-08-31','2025-10-12','SemiAnnual','LongLast',True,['2023-08-31','2024-02-29','2024-08-31','2025-02-28','2025-10-12']),
        ('annual','2023-08-31','2025-08-31','Annual','ShortLast',True,['2023-08-31','2024-08-31','2025-08-31']),
        ('quarterly','2023-11-30','2024-11-30','Quarterly','ShortLast',True,['2023-11-30','2024-02-29','2024-05-31','2024-08-31','2024-11-30']),
        ('monthly','2024-01-31','2024-05-31','Monthly','ShortLast',True,['2024-01-31','2024-02-29','2024-03-31','2024-04-30','2024-05-31']),
    ]
    cases = []; controls = []; base = None
    def add(id, req, boundary, rate, error=None):
        cases.append(dict(id=id,request=copy.deepcopy(req),expected_error=error))
        if error is None: controls.append(dict(id=id,expected=price_expected(req,boundary,rate)))
    for name,start,end,freq,stub,eom,bound in variants:
        r = copy.deepcopy(template); t = r['operation']['input']['bond']['terms']
        calendar_pin = pin('ICMA.CALENDAR',7)
        r['inputs']['calendars'] = [dict(pin=calendar_pin,holidays=['2024-03-11','2024-06-18'],weekend_days=[5,6])]
        t.update(issue_date=start,day_count='ActActIcma',redemption_multiplier='1.02',settlement_lag={'CalendarDays':0},settlement_adjustment='Unadjusted')
        t['schedule'].update(start_date=start,end_date=end,frequency=freq,stub_type=stub,end_of_month=eom)
        t['dates'].update(payment_delay={'CalendarDays':10},payment_adjustment='Following')
        t['calendars'] = {k:'ICMA.CALENDAR' for k in t['calendars']}
        for k in ['coupon','spread','gearing','notional']:
            t['profiles'][k] = [dict(start=start,end=end,value={'coupon':'0.045','spread':'0.003','gearing':'0','notional':'1000'}[k])]
        t['exercise'] = []
        settle = '2024-02-12' if name == 'monthly' else '2024-01-10'
        bond = r['operation']['input']['bond']; bond['calendars'] = {k:calendar_pin for k in bond['calendars']}
        security = dict(reference=pin('ICMA.'+name,17),quote_revision=pin('ICMA.QUOTE',23),bond=bond)
        mesh = refs(day(end if 'First' in stub else start),PER_YEAR[freq],eom)
        context = dict(revision=pin('ICMA.REFERENCES',31),security=security['reference'],frequency=freq,periods=mesh)
        convention = dict(schema_version=1,reference_context=context['revision'],quotation_policy=pin('ICMA.POLICY',41),frequency=freq,compounding='SemiAnnual',max_reference_evaluations=16777216)
        r['operation'] = dict(kind='icma_quotation',input=dict(security=security,reference_context=context,quotation_policy=convention['quotation_policy'],position=dict(reference_quantity='1000',quantity_unit='notional',currency='USD',quantity='-2500')),convention=convention,cash=dict(kind='plain',settlement=settle,on_payment='Exclude'),action=dict(kind='price',decimal_yield=0.04125))
        boundaries = list(map(day,bound))
        for comp in ['SemiAnnual','Continuous','Simple']:
            r['operation']['convention']['compounding'] = comp
            for y in [D('0.04125'),D('-0.0075')]:
                r['operation']['action'] = dict(kind='price',decimal_yield=float(y)); id = 'icma.'+name+'.'+comp+'.'+str(y)
                add(id,r,boundaries,y)
                answer = controls[-1]['expected']
                r['operation']['action'] = dict(kind='yield',target=dict(kind='clean',value=float(answer['clean_per100'])),settings=dict(lower=-0.5,upper=0.5,price_tolerance=1e-8,parameter_tolerance=1e-12,max_iterations=128))
                add(id+'.inverse',r,boundaries,y)
        if name == 'regular':base=(copy.deepcopy(r),boundaries)
    r,b = copy.deepcopy(base); r['operation']['convention']['compounding']='Quarterly'
    t=r['operation']['input']['security']['bond']['terms']; start=t['schedule']['start_date'];end=t['schedule']['end_date']
    t['profiles']['coupon']=[dict(start=start,end='2024-04-12',value='0.045'),dict(start='2024-04-12',end=end,value='0.065')]
    t['profiles']['notional']=[dict(start=start,end='2024-10-15',value='1000'),dict(start='2024-10-15',end=end,value='700')]
    t['exercise']=[dict(accrued='pay_accrued',notice_offset={'CalendarDays':-5},price_per_100='101.25',protection_end=None,right='call',settlement_adjustment='Following',settlement_lag={'BusinessDays':5},style={'date':'2024-06-17','style':'european'})]
    for label,cash in [('stepped',dict(kind='plain',settlement='2024-01-10',on_payment='Exclude')),('boundary',dict(kind='plain',settlement='2024-02-29',on_payment='Exclude')),('partial',dict(kind='workout',context=dict(reference_date='2024-01-10',trade_date='2024-01-10',settlement='2024-01-10',on_payment='Exclude',notice={'kind':'unnotified','earliest_notice':'2024-01-10'}),selection={'kind':'exercise','provision':0,'date':'2024-06-17'}))]:
        r['operation']['cash']=cash
        for comp in ['Annual','Monthly','Daily']:
            r['operation']['convention']['compounding']=comp
            r['operation']['action']=dict(kind='price',decimal_yield=0.035)
            add('icma.'+label+'.'+comp,r,b,D('0.035'))
            answer=controls[-1]['expected'];r['operation']['action']=dict(kind='yield',target={'kind':'dirty','value':float(answer['dirty_per100'])},settings=dict(lower=-0.5,upper=0.5,price_tolerance=1e-8,parameter_tolerance=1e-12,max_iterations=128))
            add('icma.'+label+'.'+comp+'.inverse',r,b,D('0.035'))
    # Completed unpaid February coupon survives March exercise at its original date.
    r['operation']['input']['security']['bond']['terms']['exercise'][0]['style']['date']='2024-03-05'
    r['operation']['cash']['selection']['date']='2024-03-05'
    r['operation']['action']=dict(kind='price',decimal_yield=0.035)
    add('icma.delayed_claim',r,b,D('0.035'))
    r['operation']['input']['security']['bond']['terms']['day_count']='Act365Fixed'
    add('icma.separate_accrual',r,b,D('0.035'))
    r,b=copy.deepcopy(base); t=r['operation']['input']['security']['bond']['terms']; t['exercise']=[]
    r['operation']['cash']=dict(kind='plain',settlement='2025-09-10',on_payment='Include')
    r['operation']['action']=dict(kind='yield',target={'kind':'dirty','value':float(price_expected(r,b,D(0))['dirty_per100'])},settings=dict(lower=-0.5,upper=0.5,price_tolerance=1e-8,parameter_tolerance=1e-12,max_iterations=128))
    add('icma.non_unique',r,b,D(0),'non_unique')
    # All negative cases preserve the originally declared inverse domain/tolerances.
    r,b=copy.deepcopy(base); r['operation']['action']=dict(kind='price',decimal_yield=0.04125)
    mutations = [
        ('empty_context',lambda o:o['input']['reference_context'].update(periods=[]),'invalid_input'),
        ('stale_context',lambda o:o['convention'].update(reference_context={'id':'ICMA.REFERENCES','version':99}),'invalid_input'),
        ('stale_policy',lambda o:o['convention'].update(quotation_policy={'id':'ICMA.POLICY','version':99}),'invalid_input'),
        ('wrong_security',lambda o:o['input']['reference_context'].update(security={'id':'ICMA.regular','version':99}),'invalid_input'),
        ('zero_frequency',lambda o:o['convention'].update(frequency='Zero'),'unsupported'),
        ('wrong_frequency',lambda o:o['convention'].update(frequency='Annual'),'invalid_input'),
        ('overlap',lambda o:o['input']['reference_context']['periods'].insert(1,copy.deepcopy(o['input']['reference_context']['periods'][0])),'invalid_input'),
        ('gap',lambda o:o['input']['reference_context']['periods'].pop(5),'invalid_input'),
        ('truncated_payment',lambda o:o['input']['reference_context'].update(periods=o['input']['reference_context']['periods'][0:9]),'invalid_input'),
        ('conflicting_date',lambda o:o['input']['reference_context']['periods'][5].update(end='2024-03-01'),'invalid_input'),
        ('reference_work',lambda o:o['convention'].update(max_reference_evaluations=1),'resource_limit'),
        ('wrong_currency',lambda o:o['input']['position'].update(currency='EUR'),'invalid_input'),
        ('wrong_reference_quantity',lambda o:o['input']['position'].update(reference_quantity='700'),'invalid_input'),
        ('stale_calendar',lambda o:o['input']['security']['bond']['calendars'].update(payment={'id':'ICMA.CALENDAR','version':99}),'missing_input'),
        ('wrong_version',lambda o:o['convention'].update(schema_version=2),'unsupported'),
    ]
    for name, mutate, error in mutations:
        c=copy.deepcopy(r);mutate(c['operation']);add('icma.invalid.'+name,c,b,D('0.04125'),error)
    for name,target,iters,error in [('no_root',10000.0,128,'no_root'),('inverse_work',99.12345,1,'no_convergence')]:
        c=copy.deepcopy(r);c['operation']['action']=dict(kind='yield',target={'kind':'dirty','value':target},settings=dict(lower=-0.5,upper=0.5,price_tolerance=1e-8,parameter_tolerance=1e-12,max_iterations=iters));add('icma.invalid.'+name,c,b,D('0.04125'),error)
    c=copy.deepcopy(r);c['operation']['input']['security']['bond']['terms']['profiles']['coupon'][0]['value']='-0.5';c['operation']['input']['security']['bond']['terms']['profiles']['spread'][0]['value']='0';c['operation']['action']=dict(kind='yield',target={'kind':'dirty','value':50.0},settings=dict(lower=-0.5,upper=0.5,price_tolerance=1e-8,parameter_tolerance=1e-12,max_iterations=128));add('icma.invalid.mixed_sign',c,b,D('0.04125'),'unsupported')
    return dict(certified=False,cases=cases),dict(precision=60,absolute_price_cash_invoice_tolerance='1e-8',absolute_yield_tolerance='1e-9',absolute_time_tolerance='1e-12',relative_tolerance='0',certified=False,cases=controls)

if __name__ == '__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args()
    actual=generate()
    paths=['tests/fixtures/library/icma-quotations.json','tests/fixtures/library/icma-reference.json']
    for path,value in zip(paths,actual):
        if a.write:
            if (ROOT/path).exists():raise ValueError('Refusing to overwrite declared controls')
            (ROOT/path).write_text(json.dumps(value,indent=2)+'\n')
        elif json.loads((ROOT/path).read_bytes()) != value:raise ValueError('Original independent controls changed: '+path)
    print(json.dumps(dict(cases=len(actual[0]['cases']),independent_quotations=len(actual[1]['cases']),certified=False)))
