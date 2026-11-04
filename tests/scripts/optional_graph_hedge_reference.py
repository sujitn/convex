"""Independent dated optional cash/notice/FRN/model/five-PV/hedge controls.

No Convex imports/execution/captures. Decimal60 calendar/profile/cash mechanics,
closed Gaussian truncated moments of original full cash portfolios at notice,
and deterministic Bermudan stopping. This is a scoped engineering reference.
"""
import argparse
from calendar import monthrange
from copy import deepcopy as clone
from datetime import date, timedelta
from decimal import Decimal as D, getcontext
import hashlib
import json
import math
from pathlib import Path

getcontext().prec = 60
ROOT = Path(__file__).resolve().parents[2]
FOLDER = ROOT/'tests/fixtures/library'
SOURCES = ['tests/fixtures/library/cases.json', 'tests/fixtures/library/graph-hedge-api.json',
           'crates/convex-analytics/tests/fixtures/optional-bonds.json']
SNAPSHOT = 'optional-graph-hedge.synthetic.snapshot.1'
DISC = dict(id='D.OPTIONAL', version='synthetic-1')
PROJ = dict(id='P.OPTIONAL', version='1')
MODEL = dict(id='M.OPTIONAL', version='original-1')
PIN = dict(id='WEEKEND', version=1)
POLICIES = dict(rates=dict(absolute_duration_tolerance=.02, relative_duration_tolerance=.01,
    absolute_convexity_tolerance=.5, relative_convexity_tolerance=.01),
    model=dict(absolute_first_tolerance=.5, relative_first_tolerance=.02,
    absolute_second_tolerance=200., relative_second_tolerance=.02))
TOLERANCES = dict(price_per100=.006, rate_risk_per100=.0001, mean_reversion_per100=1.,
    volatility_per100=2., native_risk_first=.5, native_risk_second=200.,
    cash=1e-10, hedge_quantity=1e-5, hedge_cost=1e-8)


def day(s): return date.fromisoformat(s)
def tau(a,b,denominator=365): return D((b-a).days)/denominator
def profile(t,name,on):
    return D(next(r['value'] for r in t['profiles'][name] if day(r['start'])<=on<day(r['end'])))
def span(a,b,v): return [dict(start=a,end=b,value=v)]
def grid(t):
    s=t['schedule']; a=day(s['start_date']); end=day(s['end_date']); step={'Annual':12,'SemiAnnual':6}[s['frequency']]
    rows=[a]; i=1
    while rows[-1]<end:
        m=a.year*12+a.month-1+step*i
        rows.append(min(end,date(m//12,m%12+1,min(a.day,monthrange(m//12,m%12+1)[1]))));i+=1
        if i>48: raise ValueError('Independent dated schedule ceiling')
    return rows

def shifted(on,adjustment,holidays):
    if adjustment=='Unadjusted': return on
    if adjustment!='Following': raise ValueError('Undeclared independent adjustment')
    while on.weekday()>=5 or str(on) in holidays: on+=timedelta(days=1)
    return on

def cash_date(t,on,holidays):
    return shifted(on+timedelta(days=t['dates']['payment_delay']['CalendarDays']),
                   t['dates']['payment_adjustment'],holidays)

def fixed_coupon(t,a,b):
    cuts=sorted({a,b}|{day(r[k]) for rows in t['profiles'].values() for r in rows for k in ['start','end'] if a<day(r[k])<b})
    return sum((profile(t,'notional',x)*(profile(t,'coupon',x)+profile(t,'spread',x))*tau(x,y) for x,y in zip(cuts,cuts[1:])),D(0))

def projected(input_,resources,discount,projection,oas):
    t=input_['bond']['terms']; root=day(input_['context']['settlement']); holidays=resources['calendars'][0]['holidays']
    index=t['coupon'].get('index_id'); history=[x for x in resources['fixings'] if x['index_id']==index
        and x['published_at']<=input_['projection_context']['fixing']['known_at']
        and x['known_at']<=input_['projection_context']['fixing']['known_at'] and x['status']=='Final']
    coupons=[]; principal=[]; coeff=[]; accrued=D(0); known=[]; future=[]
    def add(on,amount): coeff.append((on,float(amount)))
    for a,b in zip(grid(t),grid(t)[1:]):
        pay=cash_date(t,b,holidays)
        if pay<=root: continue
        n=profile(t,'notional',a)
        if not index:
            amount=fixed_coupon(t,a,b); ai=fixed_coupon(t,a,min(root,b)) if a<root else D(0);add(pay,amount)
            coupons.append(dict(start=str(a),end=str(b),payment_date=str(pay),amount=str(amount),accrued=str(ai),known=True))
        else:
            if a<=root:
                obs=max((x for x in history if x['observation_date']==str(a)),key=lambda x:x['revision'])
                rate=D(obs['rate']);known.append(obs); amount=n*(profile(t,'gearing',a)*rate+profile(t,'coupon',a)+profile(t,'spread',a))*tau(a,b,360)
                ai=n*(profile(t,'gearing',a)*rate+profile(t,'coupon',a)+profile(t,'spread',a))*tau(a,min(root,b),360)
                add(pay,amount); coupons.append(dict(start=str(a),end=str(b),payment_date=str(pay),amount=str(amount),accrued=str(ai),known=True))
            else:
                future.append(str(a)); g=profile(t,'gearing',a); component=(profile(t,'coupon',a)+profile(t,'spread',a))*n*tau(a,b,360)
                basis=(-D(str(projection-discount))*tau(a,b)).exp()
                # Conditional stochastic reset cash, integrated under the original
                # common discount state. OAS discounts to PAY, not to reset.
                add(a,n*g/basis*(-D(str(oas))*tau(a,b)).exp());add(b,-n*g+component)
                rate=((D(str(projection))*tau(a,b)).exp()-1)/tau(a,b,360)
                amount=n*(g*rate+profile(t,'coupon',a)+profile(t,'spread',a))*tau(a,b,360)
                ai=D(0);coupons.append(dict(start=str(a),end=str(b),payment_date=str(pay),amount=str(amount),accrued='0',known=False))
        accrued+=ai
    rows=t['profiles']['notional']
    for i,r in enumerate(rows):
        end=day(r['end']); after=D(rows[i+1]['value']) if i+1<len(rows) else D(0)
        amount=(D(r['value'])-after)*(D(t['redemption_multiplier']) if i+1==len(rows) else 1)
        pay=cash_date(t,end,holidays)
        if amount and pay>root: principal.append(dict(effective_date=str(end),payment_date=str(pay),amount=str(amount)));add(pay,amount)
    return dict(coupons=coupons,principal=principal,coeff=coeff,accrued=float(accrued),known=known,future=future)

def exercise(input_,resources,p,terms):
    t=input_['bond']['terms']; holidays=resources['calendars'][0]['holidays']; ex=day(terms['style']['date'])
    notice=ex-timedelta(days=terms['notice_offset']['CalendarDays'])
    settle=shifted(ex+timedelta(days=terms['settlement_lag']['CalendarDays']),terms['settlement_adjustment'],holidays)
    branch=[]
    for c in p['coupons']:
        if day(c['end'])<=ex and day(c['payment_date'])>=notice:
            if not c['known'] and day(c['end'])>notice: raise ValueError('Undeclared path memory in independent exercise branch')
            if c['known']: branch.append((day(c['payment_date']),float(c['amount'])))
    branch += [(day(r['payment_date']),float(r['amount'])) for r in p['principal'] if day(r['effective_date'])<=ex and day(r['payment_date'])>=notice]
    outstanding=D(0)
    for r in t['profiles']['notional']:
        if day(r['start'])<=ex<day(r['end']): outstanding=D(r['value'])
    retirement=outstanding*D(terms['price_per_100'])/100
    if terms['accrued']=='pay_accrued':
        for a,b in zip(grid(t),grid(t)[1:]):
            if a<ex<b:
                if t['coupon']['kind']!='fixed':raise ValueError('Independent FRN aligned exercise required')
                retirement+=fixed_coupon(t,a,ex)
    branch.append((settle,float(retirement)))
    return notice,branch,dict(notice=str(notice),exercise=str(ex),settlement=str(settle),right=terms['right'])

def b(a,t):return t if a==0 else -math.expm1(-a*t)/a
def cdf(x):return .5*math.erfc(-x/math.sqrt(2))

def financial_price(input_,resources,discount=.04,projection=.05,oas=0.,a=.03,sigma=.02):
    root=day(input_['context']['settlement']); t=input_['bond']['terms']; face=float(t['face_value'])
    p=projected(input_,resources,discount,projection,oas)
    df=lambda on:math.exp(-(discount+oas)*(on-root).days/365)
    plain=sum(c*df(on) for on,c in p['coeff'])
    terms=t['exercise']; original=terms[0]
    # Declared European tests; a separate deterministic multiple-right case below.
    notice,branch,dates=exercise(input_,resources,p,original)
    T=(notice-root).days/365; sdev=sigma*math.sqrt(b(2*a,T))
    difference={}
    for on,c in p['coeff']:
        if on>=notice:difference[on]=difference.get(on,0)+c
    for on,c in branch:difference[on]=difference.get(on,0)-c
    coeff=[(on,c) for on,c in difference.items() if c]
    sign=1 if original['right']=='call' else -1
    moments=[(c*math.exp(-(discount+oas)*(on-notice).days/365),b(a,(on-notice).days/365)*sdev) for on,c in coeff]
    value=lambda z:sign*sum(c*math.exp(-beta*z-.5*beta*beta) for c,beta in moments)
    option=0.; boundaries=[-16.]; last=-16.; previous=value(last)
    if sdev==0:
        option=max(value(0),0)
    else:
        # Isolate every crossing on a declared Gaussian domain. Integrate each
        # positive interval by closed truncated moments, NOT by a native tree.
        for i in range(1,4097):
            x=-16+32*i/4096; current=value(x)
            if previous*current<0:
                lo=last;hi=x;left=previous
                for _ in range(100):
                    mid=(lo+hi)/2; f=value(mid)
                    if left*f>0:lo=mid;left=f
                    else:hi=mid
                boundaries.append((lo+hi)/2)
            previous=current;last=x
        boundaries.append(16.)
        if len(boundaries)>8:raise ValueError('Independent crossing bound')
        for lo,hi in zip(boundaries,boundaries[1:]):
            if value((lo+hi)/2)>0:
                option+=sign*sum(c*(cdf(hi+beta)-cdf(lo+beta)) for c,beta in moments)
    pv=plain-sign*math.exp(-(discount+oas)*T)*option
    return dict(pv=pv,dirty=pv/face*100,clean=(pv-p['accrued'])/face*100,accrued=p['accrued']/face*100,
        plain=plain,option=option,exercise=dates,coupons=p['coupons'],principal=p['principal'],known=p['known'],future=p['future'])


def factor(kind,instrument,discount=DISC,projection=PROJ,model=MODEL):
    rate=kind not in ['a','sigma'];quote={'discount':'zero_rate','projection':'zero_rate','joint':'joint_zero_rate','oas':'oas_spread','a':'hw1f_mean_reversion','sigma':'hw1f_volatility'}[kind]
    if kind in ['discount','projection']:
        curve=discount if kind=='discount' else projection;target=dict(kind='curve',curve_id=curve['id']);coordinate=dict(kind='zero',curve=clone(curve),node=None)
    elif kind=='joint':
        target=dict(kind='curve_pair',discount_curve_id=discount['id'],discount_curve_version=discount['version'],projection_curve_id=projection['id'],projection_curve_version=projection['version']);coordinate=dict(kind='raw',input_id='joint_zero',version=1)
    elif kind=='oas':target=dict(kind='instrument',instrument_id=instrument['id']);coordinate=dict(kind='raw',input_id=instrument['id'],version=instrument['version'])
    else:target=dict(kind='model',model_id=model['id'],model_version=model['version']);coordinate=dict(kind='raw',input_id='mean_reversion' if kind=='a' else 'volatility',version=1)
    return dict(key=dict(factor=dict(family='model' if not rate else 'credit_spread' if kind=='oas' else 'interest_rate',target=target,currency='USD',tenor='spot' if kind in ['oas','a','sigma'] else 'parallel',quote_type=quote,basis='model_parameter' if not rate else 'raw_quote' if kind=='oas' else 'zero_node',convention_version=1,model_version='optional-graph.original-native-semantics.1'),coordinate=coordinate,native_unit='DecimalRate',response='frozen_curves',unit='currency_per_bp' if rate else 'currency_per_model_parameter'),settings=dict(bump=.01 if kind=='sigma' else .02,stability=dict(absolute=.25 if rate else 5.,relative=.02),policy_version=1))

def curve(key,zero,role,root='2025-01-01'):
    return dict(key=clone(key),role=role,currency='USD',reference_date=root,time_basis='Act365Fixed',representation=dict(ZeroRates=dict(compounding='Continuous')),interpolation='Linear',extrapolation='Flat',dependencies=[],calibration='Direct',source=dict(Nodes=[dict(pillar=dict(Tenor=x),quote=dict(unit='DecimalRate',value=zero)) for x in [0.,50.]]))

def generate():
    originals={c['id']:c['request'] for c in json.loads((ROOT/SOURCES[0]).read_bytes())['cases']}
    base=clone(originals['optional.price']);optional=base['operation']['input'];base['inputs']['curves']=[curve(DISC,.04,dict(kind='Discount',collateral_currency='USD')),curve(PROJ,.05,dict(kind='Projection',index_id='USD.TERM6M'))]
    optional['discount']=clone(DISC);optional['model']['key']=clone(MODEL);optional['model']['reason']='Explicit synthetic full optional graph engineering control'
    optional['context']['settlement']='2025-10-01';optional['projection_context'].update(settlement='2025-10-01');optional['projection_context']['fixing'].update(valuation_date='2025-10-01',known_at='2025-10-01T12:00:00Z')
    graph=dict(rates=[],credit=[],cash=[],rate_context=None,controls=clone(originals['calibration.quote_risk']['operation']['input']['controls']))
    cases=[];answers=[]
    def make(id,input_,resources,kinds,discount=.04,sigma=.02):
        instrument=dict(id=id.upper(),version=3); factors=[factor(k,instrument,input_['discount'],input_['projection'] or PROJ,input_['model']['key']) for k in kinds]
        identity=dict(id=id,instrument=instrument,economics=dict(id=id+'.full-terms-state',version=1),calculation=dict(id='optional-graph.original-guards',version=1),entity='OPTIONAL.DESK')
        hedge=dict(identity=identity,reference_position=dict(quantity=input_['bond']['terms']['face_value'],reference_quantity=input_['bond']['terms']['face_value'],quantity_unit='notional',currency='USD'),snapshot_id=SNAPSHOT,factors=factors,max_factor_evaluations=5*len(factors))
        product=dict(kind='optional_bond',input=clone(input_),instrument=instrument,oas_decimal=.00123456,policies=clone(POLICIES))
        req=dict(schema_version=1,inputs=clone(resources),operation=dict(kind='graph_hedge_risk',graph=clone(graph),product=product,input=hedge))
        expected=[]
        for kind,f in zip(kinds,factors):
            h=f['settings']['bump'];vals=[]
            for move in [0,h,-h,h/2,-h/2]:
                vals.append(financial_price(input_,resources,discount=discount+(move if kind in ['discount','joint'] else 0),projection=.05+(move if kind in ['projection','joint'] else 0),oas=.00123456+(move if kind=='oas' else 0),a=.03+(move if kind=='a' else 0),sigma=sigma+(move if kind=='sigma' else 0)))
            pv=[v['pv'] for v in vals];unit=.0001 if kind not in ['a','sigma'] else 1
            expected.append(dict(kind=kind,valuations=vals,prices=pv,value=(pv[1]-pv[2])/(2*h)*unit,half_value=(pv[3]-pv[4])/h*unit))
        cases.append(dict(id=id,request=req,expected_error=None));answers.append(dict(id=id,expected=dict(factors=expected)))
        return req
    def negative(label,req,error,mutate):
        r=clone(req);mutate(r);cases.append(dict(id='optional_graph.invalid.'+label,request=r,expected_error=error))
    for side in ['call','put']:
        for discount,label in [(.04,'positive'),(-.005,'negative')]:
            inp=clone(optional);inp['bond']['terms']['exercise'][0]['right']=side
            res=clone(base['inputs']);res['curves'][0]['source']['Nodes']=[dict(pillar=dict(Tenor=x),quote=dict(unit='DecimalRate',value=discount)) for x in [0.,50.]]
            make('optional_graph.zero.'+label+'.'+side,inp,res,['discount','oas','a','sigma'],discount)
        inp=clone(optional);t=inp['bond']['terms'];t['schedule']['frequency']='SemiAnnual';start='2025-01-01';end='2027-01-01'
        t['profiles']['coupon']=span(start,'2026-01-01','0.035')+span('2026-01-01','2026-07-01','0.045')+span('2026-07-01',end,'0.05')
        t['profiles']['spread']=span(start,end,'0.002');t['profiles']['notional']=span(start,'2026-01-01','100')+span('2026-01-01','2026-07-01','80')+span('2026-07-01',end,'60')
        t['dates'].update(payment_adjustment='Following',payment_delay=dict(CalendarDays=2));e=t['exercise'][0];e.update(right=side,price_per_100='99',notice_offset=dict(CalendarDays=30),settlement_lag=dict(CalendarDays=3));e['style']['date']='2026-03-15'
        res=clone(base['inputs']);res['calendars'][0]['holidays']=['2026-01-01','2026-07-01']
        make('optional_graph.stepped_notice.'+side,inp,res,['discount','oas','a','sigma'])
    # Complete stepped, seasoned aligned term FRN with two real eligible revisions.
    inp=clone(optional);t=inp['bond']['terms'];t.update(issue_date='2025-07-01',day_count='Act360');t['schedule'].update(start_date='2025-07-01',end_date='2027-07-01',frequency='SemiAnnual')
    t['coupon']=dict(kind='term',index_id='USD.TERM6M',resets=clone(t['schedule']),fixing_offset=dict(CalendarDays=0),reset_adjustment='Unadjusted',projection_day_count='Act360')
    t['profiles'].update(coupon=span('2025-07-01','2027-07-01','0.0005'),gearing=span('2025-07-01','2026-01-01','1.2')+span('2026-01-01','2027-01-01','1')+span('2027-01-01','2027-07-01','0.8'),spread=span('2025-07-01','2026-07-01','0.001')+span('2026-07-01','2027-07-01','0.002'),notional=span('2025-07-01','2026-01-01','100')+span('2026-01-01','2027-01-01','80')+span('2027-01-01','2027-07-01','60'))
    inp['projection']=clone(PROJ);res=clone(base['inputs']);res['publications']=[dict(index_id='USD.TERM6M',observation_date=str(x),expected_publication=str(x)+'T08:00:00Z') for x in grid(t)[:-1]]
    res['fixings']=[dict(index_id='USD.TERM6M',observation_date='2025-07-01',rate=rate,published_at=when,known_at=when,revision=i,status='Final',source_id='SYNTH.ELIGIBLE') for i,rate,when in [(1,'0.0297','2025-07-01T08:00:00Z'),(2,'0.0302','2025-07-02T08:00:00Z')]]
    for side in ['call','put']:
        inp['bond']['terms']['exercise'][0].update(right=side,price_per_100='100.25')
        make('optional_graph.frn.'+side,inp,res,['discount','projection','joint','oas','a','sigma'])
        deterministic=clone(inp);deterministic['model']['parameters']['volatility']=0
        make('optional_graph.frn.deterministic.'+side,deterministic,res,['discount','projection','joint','oas'],sigma=0)
    frn=clone(cases[-4]['request']);zero=clone(cases[0]['request'])
    negative('rate_unresolved',zero,'no_convergence',lambda r:r['operation']['input']['factors'][0]['settings'].update(bump=.0001))
    negative('half_unresolved',zero,'no_convergence',lambda r:r['operation']['input']['factors'][0]['settings'].update(bump=.005))
    negative('model_domain',zero,'invalid_input',lambda r:r['operation']['product']['input']['model']['parameters'].update(volatility=0))
    negative('model_version',zero,'invalid_input',lambda r:r['operation']['input']['factors'][2]['key']['factor']['target'].update(model_version='other-original'))
    negative('oas_scope',zero,'invalid_input',lambda r:r['operation']['input']['factors'][1]['key']['coordinate'].update(version=9))
    negative('work',zero,'invalid_input',lambda r:r['operation']['input'].update(max_factor_evaluations=19))
    negative('face',zero,'invalid_input',lambda r:r['operation']['input']['reference_position'].update(quantity='99',reference_quantity='99'))
    negative('duplicate',zero,'invalid_input',lambda r:r['operation']['input']['factors'].__setitem__(1,clone(r['operation']['input']['factors'][0])))
    negative('calendar_version',zero,'missing_input',lambda r:r['operation']['product']['input']['bond']['calendars']['exercise'].update(version=9))
    negative('american',zero,'unsupported',lambda r:r['operation']['product']['input']['bond']['terms']['exercise'][0].update(style=dict(style='american',start='2026-01-01',end='2026-07-01')))
    negative('price_refinement',zero,'no_convergence',lambda r:r['operation']['product']['input']['settings']['grid'].update(absolute_price_tolerance=1e-16,relative_price_tolerance=0))
    negative('rate_refinement',zero,'no_convergence',lambda r:r['operation']['product']['policies']['rates'].update(absolute_duration_tolerance=1e-12,relative_duration_tolerance=0,absolute_convexity_tolerance=1e-12,relative_convexity_tolerance=0))
    def model_refinement(r):
        r['operation']['input']['factors']=r['operation']['input']['factors'][2:];r['operation']['product']['policies']['model'].update(absolute_first_tolerance=1e-12,relative_first_tolerance=0,absolute_second_tolerance=1e-12,relative_second_tolerance=0)
    negative('model_refinement',zero,'no_convergence',model_refinement)
    # Select the actual stochastic call FRN explicitly, never by captured answers.
    frn=clone(next(c for c in cases if c['id']=='optional_graph.frn.call')['request'])
    negative('missing_fixing',frn,'missing_input',lambda r:r['inputs'].update(fixings=[]))
    negative('wrong_projection_role',frn,'invalid_input',lambda r:r['inputs']['curves'][1].update(role=dict(kind='Projection',index_id='OTHER.INDEX')))
    negative('joint_revision',frn,'invalid_input',lambda r:r['operation']['input']['factors'][2]['key']['factor']['target'].update(projection_curve_version='other-original'))
    negative('off_cycle_frn',frn,'unsupported',lambda r:r['operation']['product']['input']['bond']['terms']['exercise'][0]['style'].update(date='2026-02-01'))
    negative('elapsed_notice',zero,'unsupported',lambda r:r['operation']['product']['input']['bond']['terms']['exercise'][0].update(notice_offset=dict(CalendarDays=100)))
    # A complete actual term-future owner under all SAME six optional factors.
    future=clone(next(c for c in json.loads((ROOT/SOURCES[1]).read_bytes())['cases'] if c['id']=='graph_hedge.positive.term_future.base')['request']['operation']['product']['input'])
    def stamp(id):return dict(id=id,version=1,source='optional-engineering',source_at='2025-01-01T00:00:00Z',available_at='2025-01-01T00:00:00Z',valid_until='2028-01-01T00:00:00Z',quality='synthetic',evidence='Explicit complete synthetic input; no live exchange or execution authority')
    c=future['contract'];c.update(instrument_id='SYNTH.OPTIONAL.TERM6M',contract_code='SYNTH6M',contract_month=dict(year=2025,month=12),listed_from='2025-01-01',last_trade='2025-12-01',expiry='2025-12-01',ticks=[dict(from_='2025-01-01',until=None,increment='0.005')]);c['ticks'][0]['from']=c['ticks'][0].pop('from_');c['stamp']=stamp(c['instrument_id']);c['daily_settlement']['calendar']=clone(PIN)
    c['settlement'].update(final_settlement='2025-12-02',convexity=dict(kind='zero_assumption',evidence='Explicit synthetic zero assumption'));c['settlement']['accrual'].update(index='USD.TERM6M',fixing='2025-12-01',start='2025-12-03',end='2026-06-03',calendar=clone(PIN))
    future.update(calendar_stamp=stamp('WEEKEND'),convexity_model=None,fixing_stamp=stamp('SYNTH.ELIGIBLE'),context=dict(as_of='2025-10-01T12:00:00Z',known_at='2025-10-01T12:00:00Z'),projection=dict(curve=clone(PROJ),stamp=stamp(PROJ['id'])))
    future['clearing']['contract']=dict(id=c['instrument_id'],version=1);future['clearing']['stamp']=stamp('SYNTH.OPTIONAL.CLEARING')
    req=clone(frn);op=req['operation'];op['product']=dict(kind='rate_future',input=future);op['input']['identity'].update(id='optional_graph.book.future',instrument=dict(id=c['instrument_id'],version=1));op['input']['reference_position'].update(quantity='1',reference_quantity='1',quantity_unit='contracts');req['inputs']['publications'].append(dict(index_id='USD.TERM6M',observation_date='2025-12-01',expected_publication='2025-12-01T08:00:00Z'))
    factors=[];accrual=c['settlement']['accrual'];tt=(day(accrual['end'])-day(accrual['start'])).days/365;yf=(day(accrual['end'])-day(accrual['start'])).days/360
    for kind,f in zip(['discount','projection','joint','oas','a','sigma'],op['input']['factors']):
        h=f['settings']['bump'];prices=[100*(1-math.expm1((.05+(m if kind in ['projection','joint'] else 0))*tt)/yf)*2500 for m in [0,h,-h,h/2,-h/2]]
        unit=.0001 if kind not in ['a','sigma'] else 1;factors.append(dict(kind=kind,prices=prices,value=(prices[1]-prices[2])/(2*h)*unit,half_value=(prices[3]-prices[4])/h*unit))
    cases.append(dict(id='optional_graph.book.future',request=req,expected_error=None));answers.append(dict(id='optional_graph.book.future',expected=dict(factors=factors)))
    # Native vectors are freshly calculated; templates contain only independent
    # holdings, limits, lots, costs, target, and expected aggregate/proposal cash.
    book=clone(frn);book['operation']['input']['identity']['id']='optional_graph.book.optional';cases.append(dict(id='optional_graph.book.optional',request=book,expected_error=None));answers.append(dict(id='optional_graph.book.optional',expected=clone(next(a['expected'] for a in answers if a['id']=='optional_graph.frn.call'))))
    ov=[f['value'] for f in answers[-1]['expected']['factors']];fv=[f['value'] for f in factors]
    problem=dict(id='optional_graph.book',snapshot_id=SNAPSHOT,entity='OPTIONAL.DESK',settlement='2025-10-01',reporting_currency='USD',expected_holdings=['optional_graph.book.optional','optional_graph.book.future'],factors=[dict(key=f['key'],risk_settings=f['settings'],credit=None,scale=1. if k in ['discount','projection','joint','oas'] else 100.,weight=1.,target=0.,lower=-10000.,upper=10000.) for k,f in zip(['discount','projection','joint','oas','a','sigma'],book['operation']['input']['factors'])],holdings=[dict(column='optional_graph.book.optional',current_quantity='300',pending_quantity='50'),dict(column='optional_graph.book.future',current_quantity='2',pending_quantity='1')],candidates=[dict(column='optional_graph.book.optional',trade_lower='-400',trade_upper='400',holding_lower='0',holding_upper='1000',buy_available='400',sell_available='400',lot='50',costs=dict(currency='USD',buy_per_reference='1.5',sell_per_reference='1.25'),eligibility=dict(eligible=True,reason='Explicit supplied synthetic proposal universe')),dict(column='optional_graph.book.future',trade_lower='-4',trade_upper='4',holding_lower='0',holding_upper='10',buy_available='4',sell_available='4',lot='1',costs=dict(currency='USD',buy_per_reference='2.5',sell_per_reference='2'),eligibility=dict(eligible=True,reason='Explicit synthetic reference; proposal only'))],max_cost=20.,settings=dict(rank_relative_tolerance=1e-10,max_condition=1e4,solver_tolerance=1e-10,factor_tolerance=1e-8,cost_tolerance=1e-8,cost_scale=10.,cost_weight=0.,max_work=100000,svd_iterations=1000))
    current=[3*x+2*y for x,y in zip(ov,fv)];pending=[.5*x+y for x,y in zip(ov,fv)];before=[x+y for x,y in zip(current,pending)]
    expected=dict(current=current,pending=pending,before=before,after=[0.]*6,quantities=['-350','-3'],holdings=['0','0'],cost=10.375)
    provenance=dict(certified=False,source_reads={p:hashlib.sha256((ROOT/p).read_bytes()).hexdigest() for p in SOURCES},reference='Independent Decimal60 dated cash/profiles and closed Gaussian portfolio moments at full notice, same common-state FRN economics, native-unit central h/h2 and exact close-out hedge arithmetic; no native captures or independent human certificate.',primary_sources=['https://raw.githubusercontent.com/lballabio/QuantLib/v1.40/ql/models/shortrate/onefactormodels/hullwhite.cpp','https://cs.uwaterloo.ca/~paforsyt/numcall.pdf'])
    return dict(schema_version=1,**provenance,cases=cases),dict(**provenance,tolerances=TOLERANCES,cases=answers),dict(**provenance,tolerances=TOLERANCES,problem=problem,expected=expected)

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--write',action='store_true');args=parser.parse_args()
    for name,value in zip(['optional-graph-hedge-api.json','optional-graph-hedge-reference.json','optional-graph-hedge-book.json'],generate()):
        raw=(json.dumps(value,indent=2,default=str)+'\n').encode();p=FOLDER/name
        if args.write:
            if p.exists():raise ValueError('Refusing to overwrite declared independent controls')
            p.write_bytes(raw)
        elif p.read_bytes()!=raw:raise ValueError('Independent controls differ: '+name)
        print(json.dumps(dict(status='passed',path=name,cases=len(value.get('cases',[])),certified=False)))
