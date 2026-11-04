#!/usr/bin/env python3
"""Independent Decimal(60) dated cash/quote references; never imports native code.

New synthetic contracts and model grids only. Earlier artifacts are read-only.
Reference bounds are fixed here before any candidate numerical execution.
"""
from pathlib import Path
from datetime import date, timedelta
from decimal import Decimal as D, getcontext
import argparse, copy, json, hashlib

getcontext().prec = 60
ROOT = Path(__file__).resolve().parents[2]
OUTPUT = ROOT / 'tests/fixtures/curves/mixed-rates.json'
REF = date(2026, 10, 7)
HOLIDAYS = {date(2026,10,12),date(2026,11,11),date(2026,11,26),date(2027,1,1)}
PILLARS = [date(2026,10,30),date(2026,12,16),date(2027,10,11)]
POLICY = {'unpublished':'Forecast','final_only':True}
KEYS = {s:{'id':s,'version':'1'} for s in ['D.OIS','P.SOFR','P.3M','P.6M']}
def pin(s): return {'id':s,'version':1}
def stamp(s):
    return dict(id=s,version=1,source='engineering-CVX057',source_at='2026-09-01T00:00:00Z',
                available_at='2026-09-01T00:00:00Z',valid_until='2028-01-01T00:00:00Z',
                quality='synthetic',evidence='Explicit synthetic CVX-057 input, not live market or actual convention certification')
def business(d): return d.weekday()<5 and d not in HOLIDAYS
def following(d):
    while not business(d): d += timedelta(days=1)
    return d
def offset(d,n):
    while n:
        d += timedelta(days=1 if n>0 else -1)
        if business(d): n += -1 if n>0 else 1
    return d
def tau(a,b): return D((b-a).days)/D(360)
def profile(a,b,v): return [dict(start=str(a),end=str(b),value=str(v))]
def step(a,m,b,v,w): return profile(a,m,v)+profile(m,b,w)
def at(p,d): return next(D(v['value']) for v in p if date.fromisoformat(v['start'])<=d<date.fromisoformat(v['end']))
def schedule(a,b,f,stub='None'):
    return dict(start_date=str(a),end_date=str(b),frequency=f,stub_type=stub,end_of_month=False,first_regular_date=None,penultimate_date=None)
def leg(a,b,f,n,stub='None'):
    return dict(schedule=schedule(a,b,f,stub),dates=dict(accrual_adjustment='Unadjusted',
        payment_adjustment='Following',payment_delay={'BusinessDays':2},fixing_offset=None,observation_offset=None,ex_coupon=None),
        calendars=dict(accrual=pin('ACCRUAL'),payment=pin('PAYMENT')),day_count='Act360',notionals=n,
        rounding='Unrounded',negative_coupons='Allow')
def term(a,b,f,key,index,n,spreads,gearings):
    months=3 if f=='Quarterly' else 6
    return dict(kind='term',leg=leg(a,b,f,n),resets=dict(index_id=index,tenor_months=months,
        schedule=schedule(a,b,f),reset_calendar=pin('RESET'),reset_adjustment='Unadjusted',
        fixing_calendar=pin('FIXING'),fixing_offset={'BusinessDays':-2},fixing_adjustment='Preceding',
        rate_period='IndexTenor',index_end_of_month=False,index_end_adjustment='Following',projection_day_count='Act360'),
        profiles=dict(spreads=spreads,gearings=gearings),projection_curve=KEYS[key],fixing_policy=POLICY)
def df(on,zeros):
    x=D((on-REF).days)
    times=[D(0)]+[D((p-REF).days) for p in PILLARS]; values=[zeros[0]]+list(zeros)
    if x<0 or x>times[-1]: raise ValueError('outside explicit reference grid')
    for i in range(1,len(times)):
        if x<=times[i]:
            z=values[i-1]+(values[i]-values[i-1])*(x-times[i-1])/(times[i]-times[i-1])
            return (-z*x/D(365)).exp()
    raise ValueError('missing grid interval')
def forward(a,b,zeros): return (df(a,zeros)/df(b,zeros)-1)/tau(a,b)
def intervals(start,end):
    d=following(start)
    if d>start: d=offset(d,-1)
    while d<end:
        nxt=offset(d,1);a=max(start,d);b=min(end,nxt)
        yield d,nxt,a,b
        d=nxt
def weighted(profile_,a,b):
    cuts=sorted({a,b}|{date.fromisoformat(x[k]) for x in profile_ for k in ['start','end'] if a<date.fromisoformat(x[k])<b})
    return sum((at(profile_,s)*tau(s,e) for s,e in zip(cuts,cuts[1:])),D(0))
def cash_term(t,boundaries,zeros,discount,known):
    rows=[];pv=annuity=spread_pv=D(0)
    for a,b in zip(boundaries,boundaries[1:]):
        obs=offset(a,-2);rate=known[obs] if obs<=REF else forward(a,b,zeros)
        # Profile boundaries and reset intervals are independent, summed simply.
        cuts=sorted({a,b}|{date.fromisoformat(v[k]) for p in [t['leg']['notionals'],t['profiles']['spreads'],t['profiles']['gearings']] for v in p for k in ['start','end'] if a<date.fromisoformat(v[k])<b})
        amount=ann=spread=D(0)
        for s,e in zip(cuts,cuts[1:]):
            n=at(t['leg']['notionals'],s);g=at(t['profiles']['gearings'],s);sp=at(t['profiles']['spreads'],s)
            amount+=n*(g*rate+sp)*tau(s,e);ann+=n*tau(s,e);spread+=n*sp*tau(s,e)
        pay=following(offset(b,2));disc=df(pay,discount)
        pv+=amount*disc;annuity+=ann*disc;spread_pv+=spread*disc
        rows.append(dict(start=str(a),end=str(b),fixing=str(obs),rate=str(rate),payment=str(pay),amount=str(amount),pv=str(amount*disc)))
    return pv,annuity,spread_pv,rows
def build_case(name,discount,p3,p6,negative=False):
    old=ROOT/'tests/fixtures/futures/valuation.json'
    previous=json.loads(old.read_text())['rates']['cases']
    known_case=next(c for c in previous if c['coverage']=='partial' and c['averaging']=='compounded')
    known={date.fromisoformat(r['observation']):D(r['decimal_rate']) if not negative else -D(r['decimal_rate'])/D(10) for r in known_case['observations'] if r['known']}
    history=[]
    for d,r in known.items():
        pub=str(offset(d,1))+'T08:00:00Z'
        history.append(dict(index_id='USD.SOFR',observation_date=str(d),rate=str(r),published_at=pub,known_at=pub,revision=1,status='Final',source_id='engineering-CVX057'))
    terms_known={'USD.TERM3M':D('0.0371') if not negative else D('-0.00371'),'USD.TERM6M':D('0.0391') if not negative else D('-0.00391')}
    for idx,r in terms_known.items():
        history.append(dict(index_id=idx,observation_date='2026-10-05',rate=str(r),published_at='2026-10-06T08:00:00Z',known_at='2026-10-06T08:00:00Z',revision=1,status='Final',source_id='engineering-CVX057'))
    contract=copy.deepcopy(json.loads((ROOT/'tests/fixtures/futures/cme-2026-09-24.json').read_text())['rate_contract'])
    contract['stamp']=stamp(contract['instrument_id'])
    adjustment=D('0.00015')
    contract['settlement']['convexity']=dict(kind='supplied',decimal_rate=str(adjustment),model=pin('convexity-control'),evidence='Explicit supplied whole-quote additive forward-minus-future rate adjustment')
    factor=D(1);average=D(0);obsrows=[]
    for d,nxt,a,b in intervals(date(2026,9,16),date(2026,12,16)):
        r=known[d] if d in known else forward(d,nxt,discount)
        factor*=1+r*tau(a,b);average+=r*tau(a,b)
        obsrows.append(dict(observation=str(d),rate_end=str(nxt),weight_start=str(a),weight_end=str(b),known=d in known,rate=str(r)))
    fq=(factor-1)/tau(date(2026,9,16),date(2026,12,16))-adjustment
    average_price=D(100)*(1-(average/tau(date(2026,9,16),date(2026,12,16))-adjustment))
    a=date(2026,9,16);b=date(2027,10,7);am=date(2027,4,7)
    n=step(a,am,b,'1000000','600000')
    fixed=leg(a,b,'Annual',n,'ShortFirst')
    overnight=dict(kind='overnight',leg=leg(a,b,'Quarterly',n,'ShortFirst'),index_id='USD.SOFR',
        observation_calendar=pin('CME-calendar-reference'),convention=dict(lookback_business_days=0,observation_shift=False,lockout_business_days=0),
        profiles=dict(spreads=profile(a,b,'0.0002'),gearings=profile(a,b,'1')),projection_curve=KEYS['P.SOFR'],fixing_policy=POLICY)
    boundaries=[a,REF,date(2027,1,7),am,date(2027,7,7),b]
    opv=D(0);orows=[]
    for s,e in zip(boundaries,boundaries[1:]):
        f=D(1)
        for d,nxt,w1,w2 in intervals(s,e):
            rate=known[d] if d in known else forward(d,nxt,discount)
            f*=1+rate*tau(w1,w2)
        amount=at(n,s)*((f-1)+D('0.0002')*tau(s,e))
        pay=following(offset(e,2));pv=amount*df(pay,discount);opv+=pv
        orows.append(dict(start=str(s),end=str(e),payment=str(pay),amount=str(amount),pv=str(pv)))
    fa=sum((weighted(n,s,e)*df(following(offset(e,2)),discount) for s,e in [(a,REF),(REF,b)]),D(0))
    oq=opv/fa
    collateral=dict(agreement=pin('CSA.USD'),currency='USD',discount_curve=KEYS['D.OIS'])
    ois=dict(schema_version=1,trade_id='seasoned-OIS',version=1,currency='USD',side='PayFixed',effective_date=str(a),maturity_date=str(b),
        fixed=dict(leg=fixed,rates=profile(a,b,f'{oq:.24f}')),floating=overnight,collateral=collateral)
    a=REF; n3=step(a,date(2027,4,7),b,'1000000','700000')
    sp3=step(a,date(2027,4,7),b,'0.001','0.0015');g3=step(a,date(2027,7,7),b,'1','1.1')
    irsfloat=term(a,b,'Quarterly','P.3M','USD.TERM3M',n3,sp3,g3)
    quarter=[a,date(2027,1,7),date(2027,4,7),date(2027,7,7),b]
    ipv,ia,isp,irrows=cash_term(irsfloat,quarter,p3,discount,{date(2026,10,5):terms_known['USD.TERM3M']})
    ifa=weighted(n3,a,b)*df(following(offset(b,2)),discount);iq=ipv/ifa
    irs=dict(schema_version=1,trade_id='stepped-amortizing-IRS',version=1,currency='USD',side='PayFixed',effective_date=str(a),maturity_date=str(b),
        fixed=dict(leg=leg(a,b,'Annual',n3),rates=profile(a,b,f'{iq:.24f}')),floating=irsfloat,collateral=collateral)
    bn=profile(a,b,'1000000')
    pay=term(a,b,'Quarterly','P.3M','USD.TERM3M',bn,step(a,date(2027,1,7),b,'0.001','0.0007'),profile(a,b,'1'))
    receive=term(a,b,'SemiAnnual','P.6M','USD.TERM6M',step(a,date(2027,4,7),b,'1000000','600000'),profile(a,b,'0'),profile(a,b,'0.9'))
    ppv,pa,ps,prows=cash_term(pay,quarter,p3,discount,{date(2026,10,5):terms_known['USD.TERM3M']})
    semi=[a,date(2027,4,7),b]
    rpv,ra,rs,rrows=cash_term(receive,semi,p6,discount,{date(2026,10,5):terms_known['USD.TERM6M']})
    bq=(ppv-rpv)/ra
    receive['profiles']['spreads']=profile(a,b,f'{bq:.24f}')
    basis=dict(schema_version=1,trade_id='3M-6M-basis',version=1,currency='USD',effective_date=str(a),maturity_date=str(b),pay=pay,receive=receive,collateral=collateral)
    clearing=dict(contract=dict(id=contract['instrument_id'],version=1),stamp=stamp('clearing-control'),
        money_method='rounded_contract_values',final_rate_increment='0.000001',final_rate_rounding='nearest_up',fixing_selection='first_published')
    fterms=dict(kind='rate_future',contract=contract,clearing=clearing,projection_curve=KEYS['P.SOFR'],
        projection_stamp=stamp('P.SOFR'),fixing_stamp=stamp('fixings-control'),convexity_model=stamp('convexity-control'))
    # A separate explicit synthetic single-term contract. Fixing, underlying
    # value interval and final settlement are distinct actual supplied dates.
    term_future=copy.deepcopy(fterms)
    tc=term_future['contract'];tc['instrument_id']='SYNTH.TERM3M.2026'
    tc['stamp']=stamp(tc['instrument_id']);tc['last_trade']='2026-10-28';tc['expiry']='2026-10-28'
    tc['settlement']['final_settlement']='2026-10-29'
    tc['settlement']['accrual']=dict(kind='term',index='USD.TERM3M',fixing='2026-10-28',
        start=str(PILLARS[0]),end=str(PILLARS[1]),calendar=pin('CME-calendar-reference'),day_count='Act360')
    term_future['clearing']['contract']=pin(tc['instrument_id'])
    term_future['projection_curve']=KEYS['P.3M'];term_future['projection_stamp']=stamp('P.3M')
    term_price=D(100)*(1-(forward(PILLARS[0],PILLARS[1],p3)-adjustment))
    def helper(id_,pillar,terms,quote,unit='DecimalRate'):
        tolerance={'DecimalRate':1e-12,'PercentRate':1e-10,'BasisPoints':1e-8,'FuturePrice100MinusRate':1e-10}[unit]
        return dict(id=id_,pillar=str(pillar),terms=terms,quote=dict(value=float(quote),unit=unit),tolerance=tolerance)
    def mm(curve,start,end,fra=False):
        terms=dict(kind='Deposit',start_date=str(start),maturity_date=str(end),day_count='Act360')
        if fra:
            terms=dict(kind='Fra',index_id='USD.TERM3M' if curve=='P.3M' else 'USD.TERM6M',fixing_date=str(offset(start,-2)),
                start_date=str(start),end_date=str(end),settlement_date=str(start),settlement='AtStart',day_count='Act360')
        return dict(kind='money_market',terms=terms,curve=KEYS[curve],calendar=pin('FIXING'))
    dhelpers=[helper('discount-deposit',PILLARS[0],mm('D.OIS',REF,PILLARS[0]),forward(REF,PILLARS[0],discount)),
        helper('partly-fixed-SR3',PILLARS[1],fterms,D(100)*(1-fq),'FuturePrice100MinusRate'),
        helper('seasoned-OIS',PILLARS[2],dict(kind='swap',**ois),oq)]
    phelpers=[helper('3M-deposit',PILLARS[0],mm('P.3M',REF,PILLARS[0]),forward(REF,PILLARS[0],p3)*100,'PercentRate'),
        helper('3M-FRA',PILLARS[1],mm('P.3M',PILLARS[0],PILLARS[1],True),forward(PILLARS[0],PILLARS[1],p3)*10000,'BasisPoints'),
        helper('amortizing-IRS',PILLARS[2],dict(kind='swap',**irs),iq)]
    bhelpers=[helper('6M-deposit',PILLARS[0],mm('P.6M',REF,PILLARS[0]),forward(REF,PILLARS[0],p6)),
        helper('6M-FRA',PILLARS[1],mm('P.6M',PILLARS[0],PILLARS[1],True),forward(PILLARS[0],PILLARS[1],p6)),
        helper('tenor-basis',PILLARS[2],dict(kind='basis_swap',terms=basis,quoted_leg='Receive'),bq*10000,'BasisPoints')]
    def recipe(key,role,helpers,deps,view=None):
        return dict(curve=dict(key=KEYS[key],role=role,currency='USD',reference_date=str(REF),time_basis='Act365Fixed',
            representation=dict(ZeroRates=dict(compounding='Continuous')),interpolation='Linear',extrapolation='None',
            dependencies=[KEYS[d] for d in deps],calibration='Piecewise',source=dict(Helpers=[h['id'] for h in helpers])),
            pillars=[str(p) for p in PILLARS],helpers=helpers,overnight_projection=view,
            bootstrap=dict(lower_zero=-0.08,upper_zero=0.15,max_iterations=100),overlap_policy='Reject')
    recipes=[recipe('D.OIS',dict(kind='Discount',collateral_currency='USD'),dhelpers,[],dict(key=KEYS['P.SOFR'],index_id='USD.SOFR')),
        recipe('P.3M',dict(kind='Projection',index_id='USD.TERM3M'),phelpers,['D.OIS']),
        recipe('P.6M',dict(kind='Projection',index_id='USD.TERM6M'),bhelpers,['D.OIS','P.3M'])]
    return dict(id=name,recipes=recipes,history=history,term_future_terms=term_future,
        expected_zeros={'D.OIS':list(map(str,discount)),'P.3M':list(map(str,p3)),'P.6M':list(map(str,p6))},
        expected=dict(future_observations=obsrows,future_price=str(D(100)*(1-fq)),arithmetic_future_price=str(average_price),term_future_price=str(term_price),
            ois_par=str(oq),ois_floating_pv=str(opv),ois_fixed_annuity=str(fa),ois_coupons=orows,
            irs_par=str(iq),irs_floating_pv=str(ipv),irs_coupons=irrows,
            basis_receive_spread=str(bq),basis_pay_pv=str(-ppv),basis_annuity_receive=str(ra),
            basis_pay_coupons=prows,basis_receive_index_coupons=rrows))
def fixture():
    return dict(schema_version=1,certified=False,qualification='Independent Decimal(60) manual synthetic dated engineering controls; no external engine/market or quantitative certificate.',
        reference_method='Explicit piecewise-linear ACT365F continuous zeros; simple ACT360 deposit/FRA; full known/forecast weighted overnight product; independent term reset and stepped profile sums; spread outside compounding; actual synthetic calendar and delayed cash.',
        tolerances=dict(zero_absolute=1e-9,zero_relative=1e-10,currency_absolute=1e-4,currency_relative=1e-11,rate_absolute=1e-12,price_points_absolute=1e-10),
        reference_date=str(REF),as_of='2026-10-07T12:00:00Z',holidays=sorted(map(str,HOLIDAYS)),calendar_stamp=stamp('CME-calendar-reference'),
        source_reads={str(p.relative_to(ROOT)):hashlib.sha256(p.read_bytes()).hexdigest() for p in [ROOT/'tests/fixtures/futures/cme-2026-09-24.json',ROOT/'tests/fixtures/futures/valuation.json']},
        cases=[build_case('nonflat-positive',list(map(D,['.030','.032','.035'])),list(map(D,['.037','.039','.043'])),list(map(D,['.041','.044','.048']))),
            build_case('nonflat-negative',list(map(D,['-.006','-.004','-.003'])),list(map(D,['-.003','-.002','-.001'])),list(map(D,['-.002','-.001','.0005'])),True)])
def main():
    p=argparse.ArgumentParser();p.add_argument('mode',choices=['generate','check']);args=p.parse_args()
    data=(json.dumps(fixture(),indent=2)+'\n').encode()
    if args.mode=='generate':
        OUTPUT.parent.mkdir(parents=True,exist_ok=True);OUTPUT.write_bytes(data)
    elif OUTPUT.read_bytes()!=data: raise ValueError('independent fixture bytes differ')
    print(json.dumps(dict(status='passed',mode=args.mode,sha256=hashlib.sha256(data).hexdigest(),cases=2,curves=6,original_helpers=18,certified=False)))
if __name__=='__main__': main()
