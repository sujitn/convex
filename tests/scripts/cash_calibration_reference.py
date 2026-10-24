"""Independent Decimal(60) cash, par, credit-basis/recovery and repo controls.

No Convex imports, native execution, candidate measurements or tolerance fitting.
All new economic/calendar/curve/basis/carry inputs are Synthetic. Existing actual
ISDA discount/hazard capture bytes supply only the separate credit dependency.
"""
from pathlib import Path
from datetime import date, timedelta
from decimal import Decimal as D, getcontext
import argparse, copy, hashlib, json
getcontext().prec = 60
ROOT = Path(__file__).resolve().parents[2]
DEST = ROOT/'tests/fixtures/curves/cash-curves.json'

def dt(s): return date.fromisoformat(s)
def span(a,b,den=365): return D((dt(b)-dt(a)).days)/D(den)
def shifted(s,holidays):
    d=dt(s)
    for _ in range(366):
        if d.weekday()<5 and d.isoformat() not in holidays: return d.isoformat()
        d+=timedelta(days=1)
    raise ValueError('bounded calendar exhausted')
def months(s,n):
    d=dt(s); k=d.year*12+d.month-1+n
    return date(k//12,k%12+1,d.day).isoformat()
def prof(start,end,value): return [dict(start=start,end=end,value=str(value))]
def key(s,v='synthetic-1'): return dict(id=s,version=v)
def terms(start,end,coupon='0.04',frequency='SemiAnnual',issue=None):
    return dict(schema_version=1,currency='USD',issue_date=issue or start,face_value='5000',
      schedule=dict(start_date=start,end_date=end,frequency=frequency,stub_type='ShortLast',end_of_month=False,first_regular_date=None,penultimate_date=None),
      dates=dict(accrual_adjustment='Unadjusted',payment_adjustment='Following',payment_delay=dict(CalendarDays=0),fixing_offset=None,observation_offset=None,ex_coupon=None),
      calendars={k:'CASH.SYNTHETIC.v1' for k in ('payment','reset','observation','exercise')},
      settlement_lag=dict(BusinessDays=2),settlement_adjustment='Following',day_count='Act360',
      profiles=dict(coupon=prof(start,end,coupon),spread=prof(start,end,'0'),gearing=prof(start,end,'0'),notional=prof(start,end,'5000')),
      coupon=dict(kind='fixed'),coupon_principal='AccrualProfile',redemption_multiplier='1',principal_prices=[],rounding='Unrounded',negative_coupons='Allow',ex_coupon_accrual='SubtractCoupon',exercise=[])
def integral(t,a,b):
    total=D(0)
    edges=sorted({a,b}|{v[k] for p in t['profiles'].values() for v in p for k in ('start','end') if a<v[k]<b})
    for l,r in zip(edges,edges[1:]):
        vals={name:D(next(v['value'] for v in rows if v['start']<=l<v['end'])) for name,rows in t['profiles'].items()}
        total+=vals['notional']*(vals['coupon']+vals['spread'])*span(l,r,360)
    return total
def cash(t,settlement,holidays):
    s=t['schedule']; a=s['start_date']; end=s['end_date']; step={'Annual':12,'SemiAnnual':6,'Quarterly':3}[s['frequency']]
    coupons=[]; ai=D(0)
    for _ in range(4096):
        b=min(months(a,step),end); pay=shifted(b,holidays)
        if pay>settlement:
            amount=integral(t,a,b); accrued=integral(t,a,min(b,settlement)) if settlement>a else D(0)
            coupons.append(dict(start=a,end=b,payment_date=pay,amount=amount,accrued=accrued))
            ai+=accrued
        if b==end: break
        a=b
    else: raise ValueError('bounded periods exhausted')
    principal=[]; rows=t['profiles']['notional']
    for i,v in enumerate(rows):
        n=D(v['value']); after=D(rows[i+1]['value']) if i+1<len(rows) else D(0)
        effective=v['end']; pay=shifted(effective,holidays)
        if pay>settlement and n!=after:
            amount=(n-after)*(D(t['redemption_multiplier']) if i+1==len(rows) else D(1))
            principal.append(dict(effective_date=effective,payment_date=pay,amount=amount))
    flows=[(c['payment_date'],c['amount']) for c in coupons]+[(p['payment_date'],p['amount']) for p in principal]
    return dict(coupons=coupons,principal=principal,flows=sorted(flows),accrued=ai,face=D(t['face_value']))
def linear(reference,dates,values,on):
    xs=[D(0)]+[span(reference,d) for d in dates]; ys=[values[0]]+values; x=span(reference,on)
    if not xs[0]<=x<=xs[-1]: raise ValueError('outside explicit reference grid')
    for a,b,va,vb in zip(xs,xs[1:],ys,ys[1:]):
        if a<=x<=b: return va+(vb-va)*(x-a)/(b-a)
    raise ValueError('missing linear interval')
def zero_df(reference,dates,values,on): return (-linear(reference,dates,values,on)*span(reference,on)).exp()
def dirty(c,settle,df): return sum((amt*df(d)/df(settle) for d,amt in c['flows']),D(0))/c['face']*100
def yield_price(c,settle,y,periods=2):
    return sum((amt*((1+y/D(periods)).ln()*(-D(periods)*span(settle,d))).exp() for d,amt in c['flows']),D(0))/c['face']*100
def yield_inverse(c,settle,price):
    lo=D('-.9');hi=D('1')
    for _ in range(240):
        mid=(lo+hi)/2
        if yield_price(c,settle,mid)>price:lo=mid
        else:hi=mid
    return (lo+hi)/2
def rate(q):return D(str(q['value']))/{'DecimalRate':D(1),'PercentRate':D(100),'BasisPoints':D(10000)}[q['unit']]
def helper(id,t,reference,settle,kind,quote,credit=None):
    return dict(id=id,quote_version=1,pillar=t['schedule']['end_date'],terms=t,trade_date=reference,settlement=settle,on_payment='Exclude',credit_identity=credit,quote_kind=kind,quote=quote,tolerance=1e-8 if quote['unit'] in ('PricePer100','BasisPoints') else 1e-10)
def quote(value,unit): return dict(value=float(value),unit=unit)
def clean_kind(): return dict(kind='CleanPrice')
def yield_kind():return dict(kind='CashflowYield',convention=dict(day_count='Act365Fixed',compounding='SemiAnnual',lower_yield=-.9,upper_yield=1.,max_iterations=256,price_tolerance=1e-11))
def recipe(id,role,reference,pillars,helpers,model=None):
    deps=[]
    if model:deps=[model['reference']] if model['kind']=='ReferenceZeroSpread' else [model['discount'],model['survival']]
    return dict(curve=dict(key=key(id),role=role,currency='USD',reference_date=reference,time_basis='Act365Fixed',representation='IssuerSpreads' if model else dict(ZeroRates=dict(compounding='Continuous')),interpolation='Linear',extrapolation='None',dependencies=deps,calibration='Piecewise',source=dict(Helpers=[h['input']['id'] for h in helpers])),pillars=pillars,helpers=helpers,bootstrap=dict(lower_parameter=-.05,upper_parameter=.1,max_iterations=256),basis_model=model,overlap='Reject')
def expected(c,settle,price):
    ai=c['accrued']/c['face']*100
    return dict(coupons=c['coupons'],principal=c['principal'],accrued_per100=ai,dirty_per100=price,clean_per100=price-ai,cashflow_yield_decimal=yield_inverse(c,settle,price))
def legal(t,entity,reference='synthetic.legal'):
    return dict(instrument_id='CASH.'+t['schedule']['end_date'],instrument_version=1,reference=dict(id=reference,version=1),entity_id=entity,seniority='SeniorUnsecured',currency='USD',maturity=t['schedule']['end_date'])
def new_cases():
    reference='2026-10-07';settle='2026-10-13';holidays=['2026-10-09','2026-10-12','2026-11-11','2027-01-01'];dates=['2027-04-07','2027-10-07','2028-10-09']
    cases=[]
    for name,zeros,repos in [('positive',['.03','.036','.042'],['.025','.027','.03']),('negative',['-.008','-.006','-.002'],['-.003','-.005','-.008'])]:
        zeros=list(map(D,zeros));repos=list(map(D,repos));df=lambda d:zero_df(reference,dates,zeros,d)
        bill=terms(reference,dates[0],'0','Annual')
        stepped=terms('2026-04-07',dates[1]);stepped['profiles']['coupon']=prof('2026-04-07','2027-01-07','.035')+prof('2027-01-07',dates[1],'.045');stepped['profiles']['notional']=prof('2026-04-07',dates[0],'5000')+prof(dates[0],dates[1],'3000')
        par=terms(reference,'2028-10-07','0');c0=cash(par,settle,holidays);unit=copy.deepcopy(par);unit['profiles']['coupon'][0]['value']='1';c1=cash(unit,settle,holidays)
        p0=dirty(c0,settle,df); p1=dirty(c1,settle,df)-c1['accrued']/c1['face']*100
        coupon=((100-p0)/(p1-p0)).quantize(D('1e-28'));par['profiles']['coupon'][0]['value']=str(coupon)
        helpers=[];expect=[]
        for i,t in enumerate([bill,stepped,par]):
            c=cash(t,settle,holidays);p=dirty(c,settle,df);ai=c['accrued']/c['face']*100
            q=quote((1-p/100)/span(settle,dates[0],360)*100,'PercentRate') if i==0 else quote(p-ai,'PricePer100') if i==1 else quote(coupon*10000,'BasisPoints')
            kind=dict(kind='BankDiscountYield',day_count='Act360') if i==0 else clean_kind() if i==1 else dict(kind='ParCouponRate')
            h=helper(name+'.gov.'+str(i),t,reference,settle,kind,q);h['pillar']=dates[i]
            helpers.append(dict(kind='Bond',input=h));expect.append(expected(c,settle,p))
        gov=recipe(name+'.GOV',dict(kind='Government',benchmark_id='USD.SYNTHETIC.GOV'),reference,dates,helpers)
        bdates=dates[:2];basis=list(map(D,['-.0015','.0025']));bm=dict(kind='ReferenceZeroSpread',reference=gov['curve']['key'],role=dict(kind='Government',benchmark_id='USD.SYNTHETIC.GOV'));bhelpers=[];be=[]
        for i,t in enumerate([bill,stepped]):
            t=copy.deepcopy(t);c=cash(t,settle,holidays);weight=lambda d:df(d)*(-linear(reference,bdates,basis,d)*span(reference,d)).exp();p=dirty(c,settle,weight);e=expected(c,settle,p)
            q=quote(p,'PricePer100') if i==0 else quote(e['cashflow_yield_decimal']*100,'PercentRate');kind=dict(kind='DirtyPrice') if i==0 else yield_kind()
            h=helper(name+'.issuer.'+str(i),t,reference,settle,kind,q,legal(t,'Synthetic.Issuer'));h['pillar']=bdates[i];bhelpers.append(dict(kind='Bond',input=h));be.append(e)
        issuer=recipe(name+'.BASIS',dict(kind='IssuerSpread',entity_id='Synthetic.Issuer',model_id='reference_zero_spread_v1'),reference,bdates,bhelpers,bm)
        rpillars=['2026-10-30','2026-12-16','2027-04-07'];pin=dict(id='CASH.SYNTHETIC',version=1);rh=[]
        for i,d in enumerate(rpillars):
            f=zero_df(reference,rpillars,repos,d)/zero_df(reference,rpillars,repos,settle);r=(1/f-1)/span(settle,d,360)
            rh.append(dict(kind='Repo',input=dict(id=name+'.repo.'+str(i),quote_version=1,collateral_id='USD.SYNTHETIC.BOND',calendar=pin,start_date=settle,end_date=d,day_count='Act360',quote=quote(r*[D(1),D(100),D(10000)][i],['DecimalRate','PercentRate','BasisPoints'][i]),tolerance=[1e-12,1e-10,1e-8][i])))
        repo=recipe(name+'.REPO',dict(kind='Repo',collateral_id='USD.SYNTHETIC.BOND'),reference,rpillars,rh)
        carry_bond=terms('2026-01-15','2028-01-15',frequency='Quarterly');carry_bond['profiles']['coupon']=prof('2026-01-15','2026-08-15','.04')+prof('2026-08-15','2028-01-15','.06')
        c=cash(carry_bond,settle,holidays);spot=helper(name+'.carry',carry_bond,reference,settle,clean_kind(),quote('99.25','PricePer100'));spot['pillar']=shifted(carry_bond['schedule']['end_date'],holidays);delivery='2026-12-16';dirtyspot=D('99.25')+c['accrued']/c['face']*100
        own=list(map(D,['.04','.045','.05']));reinvest=list(map(D,['.022','.021','.02']));direct=[]
        for id,coll,vals in [(name+'.OWN','USD.SYNTHETIC.OWN',own),(name+'.COUPON','USD.SYNTHETIC.COUPON',reinvest)]:
            cr=copy.deepcopy(repo['curve']);cr.update(key=key(id),role=dict(kind='Repo',collateral_id=coll),calibration='Direct',source=dict(Nodes=[dict(pillar=dict(Date=d),quote=quote(v,'DecimalRate')) for d,v in zip([reference]+rpillars,[vals[0]]+vals)]));direct.append(cr)
        accrued_delivery=cash(carry_bond,delivery,holidays)['accrued']/c['face']*100
        repo_growth=zero_df(reference,rpillars,repos,settle)/zero_df(reference,rpillars,repos,delivery)
        own_growth=zero_df(reference,rpillars,own,settle)/zero_df(reference,rpillars,own,delivery)
        carry=[]
        for haircut,policy in [('0','HeldCash'),('0.15','Reinvested')]:
            coupons=[dict(payment_date=d,cash_per100=amount/c['face']*100,growth=(D(1) if policy=='HeldCash' else zero_df(reference,rpillars,reinvest,d)/zero_df(reference,rpillars,reinvest,delivery))) for d,amount in c['flows'] if settle<d<=delivery and amount!=0]
            rc=dirtyspot*(1-D(haircut))*(repo_growth-1);oc=dirtyspot*D(haircut)*(own_growth-1);cv=sum((v['cash_per100']*v['growth'] for v in coupons),D(0));fdirty=dirtyspot+rc+oc-cv;fclean=fdirty-accrued_delivery
            ft=dict(spot=spot,delivery=delivery,delivery_calendar=pin,repo=dict(curve=repo['curve']['key'],collateral_id='USD.SYNTHETIC.BOND'),haircut_fraction=haircut,own_cash=(dict(curve=direct[0]['key'],collateral_id='USD.SYNTHETIC.OWN') if haircut!='0' else None),coupons=(dict(kind='HeldCash',evidence='Synthetic declared uninvested coupon cash') if policy=='HeldCash' else dict(kind='Reinvested',funding=dict(curve=direct[1]['key'],collateral_id='USD.SYNTHETIC.COUPON'))))
            carry.append(dict(terms=ft,conversion_factor='0.9123',expected=dict(spot_dirty_per100=dirtyspot,spot_accrued_per100=c['accrued']/c['face']*100,delivery_accrued_per100=accrued_delivery,repo_growth=repo_growth,own_growth=own_growth if haircut!='0' else None,repo_cost_per100=rc,own_cost_per100=oc,coupons=coupons,dirty_forward_per100=fdirty,clean_forward_per100=fclean,futures_price_points=fclean/D('.9123'))))
        cases.append(dict(id=name,reference_date=reference,holidays=holidays,calendar_pin=pin,recipes=[issuer,repo,gov],direct_funding=direct,expected_parameters={gov['curve']['key']['id']:zeros,issuer['curve']['key']['id']:basis,repo['curve']['key']['id']:repos},government_expected=expect,basis_expected=be,carry=carry))
    return cases

def credit_case():
    directory=ROOT/'tests/fixtures/comparisons/credit-isda-1.8.3';source=json.loads((directory/'inputs.json').read_text());actual=json.loads((directory/'isda-reference.json').read_text());reference=source['discount_recipe']['reference_date'];settle='2022-06-24'
    dgrid=[(span(reference,n['pillar']['Date']),D(str(n['quote']['value']))) for n in source['discount_recipe']['source']['Nodes']]
    hazards=[D(str(h)) for h in actual['experiments'][0]['hazards_per_act365f_year']];qgrid=[(D(0),D(1))];previous=reference;cumulative=D(0)
    for end,h in zip(source['pillars'],hazards):
        cumulative+=h*span(previous,end);qgrid.append((span(reference,end),(-cumulative).exp()));previous=end
    def logvalue(grid,t):
        for (a,va),(b,vb) in zip(grid,grid[1:]):
            if a<=t<=b:return (va.ln()+(vb.ln()-va.ln())*(t-a)/(b-a)).exp()
        raise ValueError('credit reference outside original grid')
    dates=source['pillars'][:2];basis=list(map(D,['-.002','.003']));helpers=[];expect=[]
    for i,end in enumerate(dates):
        t=terms('2022-06-20',end,'0.04');t['settlement_lag']=dict(BusinessDays=3);t['calendars']={k:'CDS.NYM.official.2022.v1' for k in t['calendars']}
        if i==1:
            t['profiles']['coupon']=prof('2022-06-20','2023-01-20','.04')+prof('2023-01-20',end,'.05');t['profiles']['notional']=prof('2022-06-20',dates[0],'5000')+prof(dates[0],end,'3000')
        c=cash(t,settle,source['calendar_holidays']);start=span(reference,settle);horizon=span(reference,end);d0=logvalue(dgrid,start);q0=logvalue(qgrid,start)
        w=lambda day:logvalue(dgrid,span(reference,day))*logvalue(qgrid,span(reference,day))*(-linear(reference,dates,basis,day)*span(reference,day)).exp()
        promised=dirty(c,settle,w)*c['face']/100
        pts=sorted({start,horizon}|{x for x,_ in dgrid+qgrid if start<x<horizon}|{span(reference,n['start']) for n in t['profiles']['notional'] if settle<n['start']<end});recovery=D(0)
        for a,b in zip(pts,pts[1:]):
            n=D(next(v['value'] for v in reversed(t['profiles']['notional']) if span(reference,v['start'])<=a));da=logvalue(dgrid,a)/d0;db=logvalue(dgrid,b)/d0;qa=logvalue(qgrid,a)/q0;qb=logvalue(qgrid,b)/q0;mass=qa.ln()-qb.ln();x=mass+da.ln()-db.ln();moment=(1-(-x).exp())/x if x else D(1);recovery+=D('.4')*n*mass*qa*da*moment
        p=(promised+recovery)/c['face']*100;e=expected(c,settle,p);e.update(promised_pv=promised,recovery_pv=recovery)
        q=quote(p-e['accrued_per100'],'PricePer100') if i==0 else quote(e['cashflow_yield_decimal']*10000,'BasisPoints');h=helper('credit-bond.'+str(i),t,reference,settle,clean_kind() if i==0 else yield_kind(),q,legal(t,source['template']['reference']['entity_id'],'synthetic.legal.cds'));helpers.append(dict(kind='Bond',input=h));expect.append(e)
    model=dict(kind='CreditParRecovery',discount=source['template']['discount_curve'],survival=source['template']['survival_curve'],seniority='senior_unsecured',recovery=.4)
    r=recipe('CDS.BOND.BASIS',dict(kind='IssuerSpread',entity_id=source['template']['reference']['entity_id'],model_id='credit_par_recovery_promised_basis_v1'),reference,dates,helpers,model)
    return dict(id='credit-par-recovery',recipe=r,expected_parameters=basis,expected=expect,source_inputs=str((directory/'inputs.json').relative_to(ROOT)),source_capture=str((directory/'isda-reference.json').relative_to(ROOT)),qualification='New bond/basis cash is Synthetic; source discount/hazards are the retained actual original ISDA183 engineering capture. This bond recovery model is separately declared, not an ISDA bond engine.')

def build():
    reads=['tests/fixtures/comparisons/credit-isda-1.8.3/inputs.json','tests/fixtures/comparisons/credit-isda-1.8.3/isda-reference.json','tests/fixtures/futures/valuation.json']
    result=dict(schema_version=1,certified=False,qualification='Independent synthetic engineering cash curves, basis/recovery and fixed-date carry; no market/model/host/library/production/latency certificate.',reference_method='Standard-library Decimal60 manual actual-date cash/profile/accrual, cashflow yield inverse, full par coupon, nonflat zero/repo/basis curves, conditional surviving cash and exact log-linear par recovery.',
        tolerances=dict(parameter_absolute=1e-9,parameter_relative=1e-10,per100_absolute=1e-8,currency_absolute=1e-6,relative=1e-11,rate_absolute=1e-10),source_reads={p:hashlib.sha256((ROOT/p).read_bytes()).hexdigest() for p in reads},cases=new_cases(),credit_case=credit_case())
    def serialize(v):return str(v) if isinstance(v,D) else (_ for _ in ()).throw(TypeError())
    return (json.dumps(result,indent=2,default=serialize)+'\n').encode()
def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('mode',choices=['generate','check']);a=p.parse_args();data=build()
    if a.mode=='generate':DEST.parent.mkdir(parents=True,exist_ok=True);DEST.write_bytes(data)
    elif DEST.read_bytes()!=data:raise SystemExit('Independent reference bytes differ')
    print(json.dumps(dict(status='passed',mode=a.mode,sha256=hashlib.sha256(data).hexdigest(),certified=False)))
if __name__=='__main__':main()
