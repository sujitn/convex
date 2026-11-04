#!/usr/bin/env python3
"""Independent Decimal60 supplied floating-workout controls; no native imports.

The fictional contracts deliberately use one actual coupon and monthly term
resets or a short overnight window. Dates, source rates, prefix factors, profile
segments, original principal claims and independent discounting are retained.
"""
import argparse
import copy
from datetime import date, timedelta
from decimal import Decimal, localcontext, ROUND_HALF_EVEN
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CASES = ROOT / 'tests/fixtures/library/floating-workouts.json'
REFERENCE = ROOT / 'tests/fixtures/library/floating-workout-reference.json'
D = Decimal


def day(s): return date.fromisoformat(s)
def wire(v): return format(v, 'f')
def fraction(a, b, basis='Act360'):
    return D((b-a).days) / (365 if basis == 'Act365Fixed' else 360)
def business(d, calendar):
    return d.weekday() not in calendar['weekend_days'] and d.isoformat() not in calendar['holidays']
def offset(d, spec, cal):
    kind, n = next(iter(spec.items()))
    if kind == 'CalendarDays': return d + timedelta(days=n)
    sign = 1 if n >= 0 else -1
    for _ in range(abs(n)):
        d += timedelta(days=sign)
        while not business(d, cal): d += timedelta(days=sign)
    return d
def adjust(d, rule, cal):
    if rule == 'Unadjusted': return d
    result = d
    sign = -1 if rule == 'Preceding' else 1
    while not business(result, cal): result += timedelta(days=sign)
    if rule == 'ModifiedFollowing' and result.month != d.month: return adjust(d, 'Preceding', cal)
    return result
def next_business(d, cal):
    d += timedelta(days=1)
    while not business(d, cal): d += timedelta(days=1)
    return d
def cal(request, role):
    pin = request['operation']['input']['security']['bond']['calendars'][role]
    return next(c for c in request['inputs']['calendars'] if c['pin'] == pin)
def terms(request): return request['operation']['input']['security']['bond']['terms']
def value(t, name, d):
    return D(next(p['value'] for p in t['profiles'][name] if day(p['start']) <= d < day(p['end'])))
def rounding(v, t):
    r = t['rounding']
    return v if r == 'Unrounded' else v.quantize(D(1).scaleb(-r['NearestEven']), rounding=ROUND_HALF_EVEN)
def rate(request, observation, start, end):
    op = request['operation']; ctx = op['input']['projection_context']['fixing']
    index = terms(request)['coupon']['index_id']
    pub = next(p for p in request['inputs']['publications'] if p['index_id'] == index and p['observation_date'] == observation.isoformat())
    eligible = [r for r in request['inputs']['fixings'] if r['index_id'] == index and r['observation_date'] == observation.isoformat()
        and r['published_at'] <= ctx['known_at'] and r['known_at'] <= ctx['known_at']
        and (not ctx['final_only'] or r['status'] == 'Final')]
    if observation <= day(ctx['valuation_date']) and eligible:
        selected = max(eligible, key=lambda r:r['revision'])
        return D(selected['rate']), {'Published': selected}
    if observation <= day(ctx['valuation_date']) and pub['expected_publication'] <= ctx['known_at']:
        raise ValueError('missing due original history')
    z = D(str(request['inputs']['curves'][1]['source']['Nodes'][0]['quote']['value']))
    projected = ((z*fraction(start, end, 'Act365Fixed')).exp()-1) / fraction(start, end)
    return projected, {'Projected': 'FutureObservation' if observation > day(ctx['valuation_date']) else 'AwaitingPublication'}
def resets(request):
    t = terms(request); a = day(t['schedule']['start_date']); end = day(t['schedule']['end_date']); result = []
    while a < end:
        b = date(a.year + (a.month == 12), a.month % 12 + 1, a.day)
        observation = offset(a, t['coupon']['fixing_offset'], cal(request, 'reset'))
        r, origin = rate(request, observation, a, b)
        result.append(dict(start=a, end=b, observation=observation, rate=r, origin=origin)); a = b
    return result
def observations(request):
    t = terms(request); convention = t['coupon']['convention']; c = cal(request, 'observation')
    a = day(t['schedule']['start_date']); end = day(t['schedule']['end_date']); result = []
    while a < end:
        fixing_day = adjust(a, 'Preceding', c)
        next_day = next_business(fixing_day, c)
        b = min(next_day, end)
        observed = offset(fixing_day, {'BusinessDays':-convention['lookback_business_days']}, c)
        observed_end = next_business(observed, c)
        if convention['observation_shift']:
            left = offset(a, {'BusinessDays':-convention['lookback_business_days']}, c)
            right = offset(b, {'BusinessDays':-convention['lookback_business_days']}, c)
        else: left, right = a, b
        result.append(dict(observation=observed, rate_end=observed_end, left=left, right=right)); a = b
    lockout = convention['lockout_business_days']
    if lockout:
        anchor = result[-lockout-1]
        for o in result[-lockout:]: o['observation'], o['rate_end'] = anchor['observation'], anchor['rate_end']
    for o in result: o['rate'], o['origin'] = rate(request, o['observation'], o['observation'], o['rate_end'])
    return result
def coupon(request, end, weight_end=None):
    t = terms(request); start = day(t['schedule']['start_date']); basis = t['day_count']
    if end <= start: return D(0)
    cuts = {start,end}
    for name in ['coupon','spread','gearing','notional']:
        for p in t['profiles'][name]:
            if start < day(p['start']) < end: cuts.add(day(p['start']))
    simple = D(0)
    if t['coupon']['kind'] == 'term':
        rs = resets(request)
        cuts.update(r['start'] for r in rs if start < r['start'] < end)
        cuts = sorted(cuts)
        for a,b in zip(cuts,cuts[1:]):
            r = next(r['rate'] for r in rs if r['start'] <= a < r['end'])
            simple += value(t,'notional',a) * (value(t,'coupon',a)+value(t,'spread',a)+value(t,'gearing',a)*r) * fraction(a,b,basis)
        return rounding(simple,t)
    full_end = day(t['schedule']['end_date']); os = observations(request)
    cutoff = os[-1]['right'] if end == full_end else (weight_end if t['coupon']['convention']['observation_shift'] else end)
    if cutoff is None: raise ValueError('shifted partial prefix absent')
    factor = D(1)
    for o in os:
        b = min(o['right'],cutoff)
        if b > o['left']: factor *= 1+o['rate']*fraction(o['left'],b,basis)
    cuts = sorted(cuts)
    for a,b in zip(cuts,cuts[1:]):
        simple += value(t,'notional',a)*(value(t,'coupon',a)+value(t,'spread',a))*fraction(a,b,basis)
    return rounding(value(t,'notional',start)*value(t,'gearing',start)*(factor-1)+simple,t)
def paths(request):
    op = request['operation']; ctx = op['context']; t = terms(request)
    if ctx['notice']['kind'] == 'notified':
        n = ctx['notice']; return [dict(kind='exercise',provision=n['provision'],date=n['exercise'])], 1
    result = [dict(kind='maturity')]; examined = 0
    for i,p in enumerate(t['exercise']):
        s = p['style']; dates = [day(s['date'])] if s['style']=='european' else [day(d) for d in s['dates']] if s['style']=='bermudan' else [day(s['start'])+timedelta(days=j) for j in range((day(s['end'])-day(s['start'])).days+1)]
        for d in dates:
            if not day(ctx['settlement']) < d < day(t['schedule']['end_date']): continue
            examined += 1
            if p['protection_end'] and d < day(p['protection_end']): continue
            notice = offset(d,p['notice_offset'],cal(request,'exercise'))
            if notice < day(ctx['notice']['earliest_notice']): continue
            result.append(dict(kind='exercise',provision=i,date=d.isoformat()))
    return result,examined
def cash(request, selection):
    t = terms(request); op = request['operation']; ctx = op['context']; settle = day(ctx['settlement'])
    start = day(t['schedule']['start_date']); end = day(t['schedule']['end_date']); payment = adjust(offset(end,t['dates']['payment_delay'],cal(request,'payment')),t['dates']['payment_adjustment'],cal(request,'payment'))
    accrual_cut = next((day(c['weight_end']) for c in op['input']['projection_context']['overnight_accrual_cutoffs'] if day(c['period_start'])==start),None)
    accrued = coupon(request,min(end,max(start,settle)),accrual_cut) if start < settle < payment else D(0)
    flows = []; principal = t['profiles']['notional']; detail = None
    choice = selection['path']; stop = end if choice['kind']=='maturity' else day(choice['date'])
    # One actual original coupon; full earned cash survives its delayed payment.
    if end <= stop and (payment > settle or payment == settle and ctx['on_payment']=='Include'):
        flows.append(dict(date=payment.isoformat(),amount=wire(coupon(request,end)),kind='coupon'))
    for i,p in enumerate(principal):
        effective = day(p['end']); before = D(p['value']); after = D(principal[i+1]['value']) if i+1 < len(principal) else D(0)
        pay = adjust(offset(effective,t['dates']['payment_delay'],cal(request,'payment')),t['dates']['payment_adjustment'],cal(request,'payment'))
        if effective <= stop and (pay > settle or pay == settle and ctx['on_payment']=='Include'):
            amount = rounding((before-after) * (D(t['redemption_multiplier']) if effective == end else D(1)),t)
            flows.append(dict(date=pay.isoformat(),amount=wire(amount),kind='principal'))
    if choice['kind']=='exercise':
        provision = t['exercise'][choice['provision']]; c = cal(request,'exercise')
        notice = offset(stop,provision['notice_offset'],c)
        pay = adjust(offset(stop,provision['settlement_lag'],c),provision['settlement_adjustment'],c)
        outstanding = value(t,'notional',stop)
        partial = coupon(request,stop,day(selection['weight_end']) if selection['weight_end'] else None) if provision['accrued']=='pay_accrued' else D(0)
        redemption = outstanding*D(provision['price_per_100'])/100
        total = rounding(redemption+partial,t)
        if partial: flows.append(dict(date=pay.isoformat(),amount=wire(partial),kind='coupon'))
        flows.append(dict(date=pay.isoformat(),amount=wire(total-partial),kind='principal'))
        detail = dict(notice=notice.isoformat(),exercise=stop.isoformat(),settlement=pay.isoformat(),outstanding=wire(outstanding),redemption=wire(redemption),partial=wire(partial),total=wire(total))
    flows.sort(key=lambda c:c['date'])
    return flows,accrued,detail
def price(request, selection, dm):
    t = terms(request); ctx = request['operation']['context']; flows,accrued,detail = cash(request,selection)
    z = D(str(request['inputs']['curves'][0]['source']['Nodes'][0]['quote']['value']))
    settle = day(ctx['settlement']); pv = sum((D(c['amount'])*(-(z+dm)*fraction(settle,day(c['date']),'Act365Fixed')).exp() for c in flows),D(0))
    face = D(t['face_value']); dirty = pv/face*100; ai = accrued/face*100
    quantity = D(request['operation']['input']['position']['signed_quantity'])
    return dict(cashflows=flows,exercise=detail,accrued_per100=wire(ai),dirty_per100=wire(dirty),clean_per100=wire(dirty-ai),pv=wire(pv),consideration=wire(dirty*quantity/100),decimal_dm=wire(dm))
def solve(request, selection, target, settings):
    low,high = D(str(settings['lower'])),D(str(settings['upper']))
    ai = D(price(request,selection,D(0))['accrued_per100']); dirty = D(str(target['value']))+(ai if target['kind']=='clean' else 0)
    for _ in range(180):
        mid = (low+high)/2
        if D(price(request,selection,mid)['dirty_per100']) > dirty: low = mid
        else: high = mid
    result = price(request,selection,(low+high)/2)
    result['consideration'] = wire(dirty*D(request['operation']['input']['position']['signed_quantity'])/100)
    return result


def base(overnight=False, shift=False, lookback=0, lockout=0):
    old = json.loads((ROOT/'tests/fixtures/library/workouts.json').read_text())['cases'][0]['request']
    old = copy.deepcopy(old); input_ = old['operation']['input']; t = input_['bond']['terms']
    a,b = ('2024-09-27','2024-10-04') if overnight else ('2024-01-02','2024-07-02')
    ref = a if overnight else '2024-01-03'; settle = ref if overnight else '2024-01-05'
    input_['reference'] = dict(id='floating.original.security',version=17)
    input_['quote_revision'] = dict(id='floating.original.quote',version=29)
    t.update(issue_date=a,face_value='1000',day_count='Act360',redemption_multiplier='1.02',rounding='Unrounded',coupon_principal='AccrualProfile')
    t['schedule'] = dict(start_date=a,end_date=b,frequency='Annual' if overnight else 'SemiAnnual',stub_type='ShortLast',end_of_month=False,first_regular_date=None,penultimate_date=None)
    t['dates'].update(accrual_adjustment='Unadjusted',payment_adjustment='Following',payment_delay={'BusinessDays':3} if overnight else {'CalendarDays':10},ex_coupon=None)
    t['settlement_lag'] = {'BusinessDays':0 if overnight else 2};t['settlement_adjustment']='Following'
    t['profiles'] = {name:[dict(start=a,end=b,value=v)] for name,v in [('coupon','0.002'),('spread','0.003'),('gearing','1.2'),('notional','1000')]}
    if not overnight:
        t['profiles']['notional'] = [dict(start=a,end='2024-02-02',value='1000'),dict(start='2024-02-02',end='2024-04-02',value='850'),dict(start='2024-04-02',end=b,value='700')]
        t['profiles']['gearing'] = [dict(start=a,end='2024-04-01',value='1.25'),dict(start='2024-04-01',end=b,value='0.8')]
        t['profiles']['spread'] = [dict(start=a,end='2024-02-15',value='0.001'),dict(start='2024-02-15',end=b,value='0.004')]
    else:
        t['profiles']['spread'] = [dict(start=a,end='2024-09-30',value='0.001'),dict(start='2024-09-30',end=b,value='0.004')]
    original_term = next(c for c in json.loads((ROOT/'tests/fixtures/library/cases.json').read_text())['cases'] if c['id']=='frn.construct')['request']['operation']['bond']['terms']['coupon']
    t['coupon'] = dict(kind='overnight',index_id='USD.SUPPLIED.ON',convention=dict(lookback_business_days=lookback,observation_shift=shift,lockout_business_days=lockout)) if overnight else copy.deepcopy(original_term)
    if not overnight: t['coupon']['resets'].update(start_date=a,end_date=b);t['coupon']['index_id']='USD.SUPPLIED.TERM'
    dates = ['2024-09-29','2024-10-02','2024-09-30','2024-10-03'] if overnight else ['2024-04-02','2024-05-15','2024-04-15','2024-04-18']
    def provision(right,style):
        return dict(right=right,style=style,price_per_100='100' if overnight else '101.25',notice_offset={'BusinessDays':-1},settlement_lag={'BusinessDays':2},settlement_adjustment='Following',accrued='pay_accrued',protection_end=None)
    t['exercise'] = [provision('call',dict(style='european',date=dates[0])),provision('put',dict(style='bermudan',dates=[dates[0],dates[1]])),provision('call',dict(style='american',start=dates[2],end=dates[3]))]
    if not overnight: t['exercise'][2]['protection_end']='2024-04-16'
    t['principal_prices']=[]
    old['inputs']['calendars']=[]
    for i,role in enumerate(['payment','reset','observation','exercise']):
        pin=dict(id='FLOAT.'+role.upper(),version=i+11)
        input_['bond']['calendars'][role]=pin;t['calendars'][role]=pin['id']
        holidays={'payment':['2024-04-02','2024-10-07'],'reset':['2024-03-29'],'observation':['2024-10-02'],'exercise':['2024-04-01','2024-10-01']}[role]
        old['inputs']['calendars'].append(dict(pin=pin,holidays=holidays,weekend_days=[5,6]))
    def curve(id_,role,z):
        return dict(key=dict(id=id_,version='original-r17'),reference_date=ref,currency='USD',role=role,representation={'ZeroRates':{'compounding':'Continuous'}},time_basis='Act365Fixed',interpolation='Linear',extrapolation='Flat',calibration='Direct',dependencies=[],source={'Nodes':[dict(pillar={'Tenor':0.0},quote=dict(unit='DecimalRate',value=z)),dict(pillar={'Tenor':2.0},quote=dict(unit='DecimalRate',value=z))]})
    old['inputs']['curves']=[curve('FLOAT.DISCOUNT',dict(kind='Discount',collateral_currency='USD'),0.028),curve('FLOAT.PROJECTION',dict(kind='Projection',index_id=t['coupon']['index_id']),0.032)]
    old['inputs']['fixings']=[];old['inputs']['publications']=[]
    fixing=dict(valuation_date=ref,known_at=ref+'T18:00:00Z',unpublished='Forecast',final_only=True)
    old['operation']=dict(kind='floating_workout',input=dict(security=input_,projection_context=dict(settlement=settle,on_payment='Exclude',fixing=fixing,overnight_accrual_cutoffs=[]),projection=old['inputs']['curves'][1]['key'],discount=old['inputs']['curves'][0]['key'],position=dict(signed_quantity='-2500',reference_quantity='1000',quantity_unit='Notional',currency='USD'),quotation_policy=dict(id='FLOAT.POLICY',version=5)),context=dict(reference_date=ref,trade_date=ref,settlement=settle,on_payment='Exclude',notice=dict(kind='unnotified',earliest_notice=ref)),action=dict(kind='price',selection=dict(path=dict(kind='exercise',provision=0,date=dates[0]),weight_end='2024-09-27' if shift else None),decimal_dm=0.00625))
    # Generous supplied publication manifest; operation consumes only actual requirements.
    index=t['coupon']['index_id']; d=day(a)-timedelta(days=10)
    while d < day(b):
        old['inputs']['publications'].append(dict(index_id=index,observation_date=d.isoformat(),expected_publication=d.isoformat()+'T16:00:00Z'))
        if d <= day(ref) and (not overnight or business(d,cal(old,'observation'))):
            old['inputs']['fixings'].append(dict(index_id=index,observation_date=d.isoformat(),rate='0.04',published_at=d.isoformat()+'T16:00:00Z',known_at=d.isoformat()+'T16:01:00Z',revision=1,status='Final'))
        d+=timedelta(days=1)
    old['schema_version']=1
    return old


SETTINGS = dict(lower=-0.5,upper=0.5,price_tolerance=1e-9,parameter_tolerance=1e-13,max_iterations=128)
def controls():
    cases=[]; expected=[]
    def add(id_,request,error=None):
        cases.append(dict(id=id_,request=copy.deepcopy(request),expected_error=error))
        if error: return
        action=request['operation']['action']
        if action['kind']=='price': result=price(request,action['selection'],D(str(action['decimal_dm'])))
        elif action['kind']=='discount_margin': result=solve(request,action['selection'],action['target'],action['settings'])
        else:
            choices,examined=paths(request); quotes=[]
            for choice in choices:
                prefix=next((p['weight_end'] for p in action['shifted_prefixes'] if p['path']==choice),None)
                selection=dict(path=choice,weight_end=prefix)
                quotes.append(dict(selection=selection,expected=solve(request,selection,action['target'],action['settings']['inverse'])))
            result=dict(quotes=quotes,examined_dates=examined)
        expected.append(dict(id=id_,expected=result))
    for name,request in [('term',base()),('overnight',base(True)),('lookback',base(True,False,2)),('lockout',base(True,False,0,2)),('shifted',base(True,True,2,1))]:
        add(name+'.price',request)
        for choice in ['maturity','included','rounded','negative']:
            r=copy.deepcopy(request);t=terms(r)
            if choice=='maturity':r['operation']['action']['selection']=dict(path=dict(kind='maturity'),weight_end=None)
            if choice=='included':t['exercise'][0]['accrued']='included_in_price';r['operation']['action']['selection']['weight_end']=None
            if choice=='rounded':t['rounding']={'NearestEven':2}
            if choice=='negative':
                r['operation']['action']['decimal_dm']=-0.0175
                for f in r['inputs']['fixings']:f['rate']='-0.005'
                for node in r['inputs']['curves'][1]['source']['Nodes']:node['quote']['value']=-0.001
            add(name+'.'+choice,r)
        inverse=copy.deepcopy(request);p=price(request,request['operation']['action']['selection'],D('0.00625'))
        inverse['operation']['action']=dict(kind='discount_margin',selection=request['operation']['action']['selection'],target=dict(kind='dirty',value=float(p['dirty_per100'])),settings=SETTINGS)
        add(name+'.dm',inverse)
        r=copy.deepcopy(inverse);r['operation']['action']['target']=dict(kind='clean',value=float(p['clean_per100']));add(name+'.clean_dm',r)
    # Later lockout prefix retains the original frozen observation, never new lockout.
    r=base(True,False,0,2);r['operation']['action']['selection']['path']=dict(kind='exercise',provision=2,date='2024-10-03');add('lockout.late_prefix',r)
    # Known early notice forces its exact future path, without optimal-exercise prediction.
    r=base(True);r['operation']['context']['notice']=dict(kind='notified',provision=0,exercise='2024-09-29',notice_date='2024-09-27');add('notice.forced',r)
    # Purchase inside the actual coupon; accrued uses its own shifted cutoff.
    r=base(True,True,2,0);r['operation']['context'].update(reference_date='2024-09-29',trade_date='2024-09-29',settlement='2024-09-29',notice=dict(kind='unnotified',earliest_notice='2024-09-29'))
    terms(r)['settlement_adjustment']='Unadjusted';r['operation']['input']['projection_context'].update(settlement='2024-09-29',fixing=dict(valuation_date='2024-09-29',known_at='2024-09-29T18:00:00Z',unpublished='Forecast',final_only=True),overnight_accrual_cutoffs=[dict(period_start='2024-09-27',weight_end='2024-09-27')])
    for c in r['inputs']['curves']:c['reference_date']='2024-09-29'
    r['operation']['action']['selection']=dict(path=dict(kind='exercise',provision=2,date='2024-10-03'),weight_end='2024-10-01');add('shifted.purchase_accrued',r)
    for name,request in [('term',base()),('overnight',base(True)),('shifted',base(True,True,2,1))]:
        r=copy.deepcopy(request);choices,_=paths(r);prefixes=[]
        if name=='shifted':
            for choice in choices:
                if choice['kind']=='exercise':
                    # Supplied mapping is explicit, including nonbusiness exercise.
                    ends={'2024-09-29':'2024-09-27','2024-09-30':'2024-09-27','2024-10-01':'2024-09-30','2024-10-02':'2024-10-01','2024-10-03':'2024-10-01'}
                    prefixes.append(dict(path=choice,weight_end=ends[choice['date']]))
        r['operation']['action']=dict(kind='paths',target=dict(kind='dirty',value=100.0),settings=dict(inverse=SETTINGS,max_dates=32,max_cash_evaluations=1_000_000),shifted_prefixes=prefixes);add(name+'.paths',r)
    # Completed unpaid coupon survives a following-period exercise before cash date.
    r=base();t=terms(r);t['schedule']['frequency']='Quarterly';t['exercise'][0]['style']['date']='2024-04-05';r['operation']['action']['selection']['path']['date']='2024-04-05'
    # This two-coupon variant has a separately hand-derived original first claim.
    # Its numerical control is declared separately below rather than inferred by
    # the one-coupon calculator above.
    cases.append(dict(id='term.completed_claim',request=copy.deepcopy(r),expected_error=None))
    first=copy.deepcopy(r);terms(first)['schedule']['end_date']='2024-04-02'
    earned=coupon(first,day('2024-04-02'))
    total=coupon(r,day('2024-04-05'))
    original=price(r,r['operation']['action']['selection'],D('0.00625'))
    delta=total-earned
    detail=original['exercise'];detail['partial']=wire(delta);detail['total']=wire(rounding(D(detail['redemption'])+delta,t))
    flows=[c for c in original['cashflows'] if c['kind']!='coupon']
    flows.extend([dict(date='2024-04-12',amount=wire(earned),kind='coupon'),dict(date=detail['settlement'],amount=wire(delta),kind='coupon')]);flows.sort(key=lambda c:c['date'])
    pv=sum((D(c['amount'])*(-D('0.03425')*fraction(day('2024-01-05'),day(c['date']),'Act365Fixed')).exp() for c in flows),D(0));dirty=pv/10
    original.update(cashflows=flows,pv=wire(pv),dirty_per100=wire(dirty),clean_per100=wire(dirty-D(original['accrued_per100'])),consideration=wire(dirty*D('-2500')/100))
    expected.append(dict(id='term.completed_claim',expected=original))
    def bad(id_,mutate,error='invalid_input',request=None):
        r=copy.deepcopy(request or base());mutate(r);add('invalid.'+id_,r,error)
    bad('missing_history',lambda r:r['inputs'].update(fixings=[]),'missing_input')
    bad('missing_publications',lambda r:r['inputs'].update(publications=[]),'missing_input')
    bad('missing_projection',lambda r:r['inputs']['curves'].pop(),'missing_input')
    bad('stale_projection',lambda r:r['operation']['input']['projection'].update(version='other'),'missing_input')
    bad('stale_calendar',lambda r:r['operation']['input']['security']['bond']['calendars']['exercise'].update(version=99),'missing_input')
    bad('wrong_projection_role',lambda r:r['inputs']['curves'][1].update(role=dict(kind='Discount',collateral_currency='USD')))
    bad('wrong_index',lambda r:r['inputs']['curves'][1]['role'].update(index_id='OTHER'))
    bad('wrong_discount_role',lambda r:r['inputs']['curves'][0].update(role=dict(kind='Projection',index_id='USD.SUPPLIED.TERM')))
    bad('wrong_currency',lambda r:r['operation']['input']['position'].update(currency='EUR'))
    bad('wrong_units',lambda r:r['operation']['input']['position'].update(quantity_unit='Contracts'))
    bad('wrong_reference_face',lambda r:r['operation']['input']['position'].update(reference_quantity='850'))
    bad('wrong_projection_date',lambda r:r['inputs']['curves'][1].update(reference_date='2024-01-02'))
    bad('wrong_fixing_date',lambda r:r['operation']['input']['projection_context']['fixing'].update(valuation_date='2024-01-04'))
    bad('wrong_settlement',lambda r:r['operation']['input']['projection_context'].update(settlement='2024-01-04'))
    bad('wrong_purchase',lambda r:r['operation']['context'].update(trade_date='2024-01-04'))
    bad('late_notice',lambda r:r['operation']['context']['notice'].update(earliest_notice='2024-04-02'))
    bad('unknown_provision',lambda r:r['operation']['action']['selection']['path'].update(provision=9))
    bad('uncontracted_date',lambda r:r['operation']['action']['selection']['path'].update(date='2024-04-03'))
    bad('unused_prefix',lambda r:r['operation']['action']['selection'].update(weight_end='2024-04-01'))
    bad('shift_missing_prefix',lambda r:r['operation']['action']['selection'].update(weight_end=None),request=base(True,True,2))
    bad('shift_bad_prefix',lambda r:r['operation']['action']['selection'].update(weight_end='2024-10-08'),request=base(True,True,2))
    bad('fixed_coupon',lambda r:terms(r).update(coupon=dict(kind='fixed')))
    bad('negative_reject',lambda r:(terms(r).update(negative_coupons='Reject'),r['inputs']['fixings'][0].update(rate='-0.5')))
    bad('intracoupon_overnight_gearing',lambda r:terms(r)['profiles'].update(gearing=[dict(start='2024-09-27',end='2024-09-30',value='1.2'),dict(start='2024-09-30',end='2024-10-04',value='1.4')]),'unsupported',base(True))
    inv=next(c['request'] for c in cases if c['id']=='term.dm')
    bad('no_root',lambda r:r['operation']['action'].update(target=dict(kind='dirty',value=200)),'no_root',inv)
    bad('work',lambda r:r['operation']['action']['settings'].update(max_iterations=1),'no_convergence',inv)
    p=next(c['request'] for c in cases if c['id']=='term.paths')
    bad('date_budget',lambda r:r['operation']['action']['settings'].update(max_dates=1),'resource_limit',p)
    bad('cash_budget',lambda r:r['operation']['action']['settings'].update(max_cash_evaluations=1),'resource_limit',p)
    bad('unused_paths_prefix',lambda r:r['operation']['action']['shifted_prefixes'].append(dict(path=dict(kind='exercise',provision=0,date='2024-04-03'),weight_end='2024-04-01')),request=p)
    p=next(c['request'] for c in cases if c['id']=='shifted.paths')
    bad('duplicate_paths_prefix',lambda r:r['operation']['action']['shifted_prefixes'].append(copy.deepcopy(r['operation']['action']['shifted_prefixes'][0])),request=p)
    bad('missing_paths_prefix',lambda r:r['operation']['action']['shifted_prefixes'].pop(),request=p)
    return dict(schema_version=1,certified=False,qualification='Independent supplied fictional term/overnight controls, not market/model certification.',cases=cases),dict(precision=60,certified=False,absolute_cash_price_tolerance='0.00000001',absolute_dm_tolerance='0.000000001',relative_tolerance='0',cases=expected)


def main():
    parser=argparse.ArgumentParser();parser.add_argument('action',choices=['generate','check']);args=parser.parse_args()
    with localcontext() as ctx:
        ctx.prec=60;cases,reference=controls()
    for path,value in [(CASES,cases),(REFERENCE,reference)]:
        raw=(json.dumps(value,indent=2)+'\n').encode()
        if args.action=='generate':
            if path.exists(): raise ValueError('Refuse to overwrite declared controls: '+str(path))
            path.write_bytes(raw)
        elif path.read_bytes()!=raw: raise ValueError('Independent original control changed: '+str(path))
    print(json.dumps(dict(status='passed',requests=len(cases['cases']),independent_cases=len(reference['cases']),certified=False)))
if __name__=='__main__': main()
