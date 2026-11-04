"""Predeclared Decimal60 complete issuer cash/five-PV/quote controls.

No native imports, executions, outputs or fitted tolerances. All new economic
inputs are explicitly synthetic. Reuse only preserved independent time/cash
primitives; contractual cash is rebuilt from every supplied profile/date.
"""
import argparse
from calendar import monthrange
from copy import deepcopy as clone
from datetime import date, timedelta
from decimal import Decimal as D, getcontext
import hashlib
import json
from pathlib import Path
import cash_calibration_reference as cash
from icma_reference import fraction
from icma_helpers_reference import quote_price, bisect, MULTIPLIER

getcontext().prec = 60
ROOT = Path(__file__).resolve().parents[2]
FOLDER = ROOT / 'tests/fixtures/library'
SOURCES = ['tests/fixtures/library/graph-hedge-api.json', 'tests/fixtures/library/icma-helpers-api.json',
           'tests/scripts/cash_calibration_reference.py', 'tests/scripts/icma_reference.py',
           'tests/scripts/icma_helpers_reference.py']
MODEL = 'credit_par_recovery_promised_basis_v1'
REFMODEL = 'reference_zero_spread_v1'
SNAPSHOT = 'cash-basis-hedge.synthetic.snapshot.1'
KEY = dict(id='B.ACME', version='1')

def day(s): return date.fromisoformat(s)
def prof(t, name, on):
    return D(next(r['value'] for r in t['profiles'][name] if r['start'] <= str(on) < r['end']))
def project(h, holidays):
    t = h['terms']; s = t['schedule']; start = day(s['start_date']); end = day(s['end_date']); settle = day(h['settlement'])
    mesh = h['quote_kind'].get('input', {}).get('reference_context', {}).get('periods')
    def tau(a, b):
        return fraction(a, b, mesh, 2) if t['day_count'] == 'ActActIcma' else D((b-a).days)/360
    def cashdate(on):
        if t['dates']['payment_adjustment'] == 'Unadjusted': return on
        return day(cash.shifted(str(on), holidays))
    def coupon(a, b):
        cuts = sorted({a,b} | {day(r[k]) for rows in t['profiles'].values() for r in rows for k in ['start','end'] if a < day(r[k]) < b})
        return sum((prof(t,'notional',x)*(prof(t,'coupon',x)+prof(t,'spread',x))*tau(x,y) for x,y in zip(cuts,cuts[1:])),D(0))
    rows=[]; a=start; i=1; accrued=D(0)
    while a<end:
        k=start.year*12+start.month-1+6*i
        last=monthrange(k//12,k%12+1)[1]
        b=min(end,date(k//12,k%12+1,last if s['end_of_month'] else min(start.day,last)))
        pay=cashdate(b)
        if pay>settle or (pay==settle and h['on_payment']=='Include'):
            amount=coupon(a,b); ai=coupon(a,min(settle,b)) if settle>a else D(0)
            rows.append(dict(start=str(a),end=str(b),payment_date=str(pay),amount=str(amount),accrued=str(ai)))
            accrued+=ai
        a=b;i+=1
        if i>4096: raise ValueError('Independent bounded coupon work exhausted')
    principal=[]; notionals=t['profiles']['notional']
    for i,n in enumerate(notionals):
        after=D(notionals[i+1]['value']) if i+1<len(notionals) else D(0)
        amount=(D(n['value'])-after)*(D(t['redemption_multiplier']) if i+1==len(notionals) else 1)
        pay=cashdate(day(n['end']))
        if amount and (pay>settle or (pay==settle and h['on_payment']=='Include')):
            principal.append(dict(effective_date=n['end'],payment_date=str(pay),amount=str(amount)))
    flows=[dict(date=c['payment_date'],kind='coupon',amount=c['amount']) for c in rows]
    flows += [dict(date=c['payment_date'],kind='principal',amount=c['amount']) for c in principal]
    flows.sort(key=lambda x:(x['date'],x['kind']))
    return dict(coupons=rows,principal=principal,cashflows=flows,accrued=accrued,accrued_per100=accrued/D(t['face_value'])*100)

def value(h, projected, discount, hazard, spread, recovery, reference_only=False):
    settle=day(h['settlement']); t=h['terms']; end=day(t['schedule']['end_date'])
    hazard=D(0) if reference_only else hazard
    promised=sum((D(c['amount'])*(-(discount+hazard+spread)*D((day(c['date'])-settle).days)/365).exp() for c in projected['cashflows']),D(0))
    recovered=D(0)
    if not reference_only:
        edges=sorted({settle,end} | {day(n['start']) for n in t['profiles']['notional'] if settle<day(n['start'])<end})
        k=discount+hazard
        for a,b in zip(edges,edges[1:]):
            lo=D((a-settle).days)/365;hi=D((b-settle).days)/365
            moment=(((-k*lo).exp()-(-k*hi).exp())/k) if k else hi-lo
            recovered+=recovery*prof(t,'notional',a)*hazard*moment
    pv=promised+recovered;dirty=pv/D(t['face_value'])*100
    return dict(present_value=str(pv),promised_pv=str(promised),recovery_pv=str(recovered),dirty_per100=str(dirty),clean_per100=str(dirty-projected['accrued_per100']))

def quote(h, projected, dirty=None):
    kind=h['quote_kind']['kind']; native=h['quote']['unit'];ai=projected['accrued_per100'];face=D(h['terms']['face_value']);settle=day(h['settlement'])
    if kind=='ContextualIcmaYield':
        price=lambda y:D(quote_price(h,projected['cashflows'],ai,y)['dirty_per100'])
    else:
        price=lambda y:sum((D(c['amount'])*(-(1+y/2).ln()*2*D((day(c['date'])-settle).days)/365).exp() for c in projected['cashflows']),D(0))/face*100
    if dirty is not None:
        if kind=='CleanPrice': return dirty-ai,None
        if kind=='DirtyPrice': return dirty,None
        y=bisect(price,dirty,D('-.15'),D('.15'))
        return y*MULTIPLIER[native],quote_price(h,projected['cashflows'],ai,y) if kind=='ContextualIcmaYield' else None
    original=D(str(h['quote']['value']))
    if kind=='CleanPrice': dirty=original+ai
    elif kind=='DirtyPrice': dirty=original
    else:dirty=price(original/MULTIPLIER[native])
    return dict(dirty_per100=str(dirty),clean_per100=str(dirty-ai),accrued_per100=str(ai),
        present_value=str(dirty*face/100),icma=quote_price(h,projected['cashflows'],ai,original/MULTIPLIER[native]) if kind=='ContextualIcmaYield' else None)

def factor(kind):
    curve={'zero':'D.OIS','hazard':'H.ACME','basis':'B.ACME','recovery':'H.ACME','unrelated':'P.3M'}[kind]
    quote_type={'zero':'zero_rate','hazard':'hazard_rate','basis':'residual_basis','recovery':'recovery_rate','unrelated':'zero_rate'}[kind]
    family={'zero':'interest_rate','hazard':'hazard','basis':'credit_spread','recovery':'recovery','unrelated':'interest_rate'}[kind]
    coordinate=dict(kind='zero' if kind=='unrelated' else kind,curve=dict(id=curve,version='1'))
    if kind!='recovery':coordinate['node']=None
    return dict(key=dict(factor=dict(family=family,target=dict(kind='entity',entity_id='ACME') if kind=='recovery' else dict(kind='curve',curve_id=curve),currency='USD',tenor='parallel',quote_type=quote_type,basis='model_parameter' if kind in ['hazard','recovery'] else 'zero_node',convention_version=1,model_version='cash-basis-hedge.synthetic.original-owner.1'),coordinate=coordinate,native_unit='DecimalRate',response='frozen_curves',unit='currency_per_annual_rate' if kind=='hazard' else 'currency_per_fraction' if kind=='recovery' else 'currency_per_bp'),settings=dict(bump=0.0001,stability=dict(absolute=1.0 if kind in ['hazard','recovery'] else 0.05,relative=0.0001),policy_version=1))

def curve(name, zero, origin, reference, model):
    credit=name=='H.ACME';basis=name=='B.ACME';prob=credit or name=='D.OIS'
    role=dict(kind='Credit',entity_id='ACME',seniority='senior_unsecured',recovery=reference['recovery_fraction']) if credit else dict(kind='IssuerSpread',entity_id='ACME',model_id=model) if basis else dict(kind='Discount',collateral_currency='USD') if name=='D.OIS' else dict(kind='Projection',index_id='USD.TERM3M')
    return dict(key=dict(id=name,version='1'),role=role,currency='USD',reference_date=origin,time_basis='Act365Fixed',representation='SurvivalProbabilities' if credit else 'DiscountFactors' if prob else 'IssuerSpreads' if basis else dict(ZeroRates=dict(compounding='Continuous')),interpolation='LogLinear' if prob else 'Linear',extrapolation='None',dependencies=[],calibration='Direct',source=dict(Nodes=[dict(pillar=dict(Tenor=time),quote=dict(value=float((-zero*D(time)).exp()) if prob else float(zero),unit='SurvivalProbability' if credit else 'DiscountFactor' if prob else 'DecimalRate')) for time in [0,50]]))

def generate():
    original=json.loads((ROOT/SOURCES[0]).read_bytes())['cases']; base=original[0]['request']; cds=clone(next(c for c in original if c['id']=='graph_hedge.positive.cds.base')['request']['operation']['product']['terms'])
    contextual=json.loads((ROOT/SOURCES[1]).read_bytes())['cases'][0]['request']
    cases=[];answers=[]
    def add(id,req,answer=None,error=None):
        cases.append(dict(id=id,request=clone(req),expected_error=error))
        if answer is not None: answers.append(dict(id=id,expected=answer))
    for rlabel,discount in [('positive',D('.03')),('negative',D('-.01'))]:
        for reference_only in [False,True]:
            for label in ['clean','dirty','percent_yield','icma']:
                reference=clone(cds['reference']);r=D('.4');hazard=D('.02');spread=D('.0025') if rlabel=='positive' else D('-.0015')
                if label=='icma':
                    src=contextual['operation']['input'];h=clone(src['helper']);pins=clone(src['calendars']);inputs=clone(contextual['inputs']);origin=h['settlement'];h['terms']['dates'].update(payment_delay=dict(CalendarDays=0),payment_adjustment='Unadjusted');h['pillar']=h['terms']['schedule']['end_date']
                else:
                    product=base['operation']['product'];h=dict(id='CASH.ACME',quote_version=2,pillar=product['bond']['terms']['schedule']['end_date'],terms=clone(product['bond']['terms']),trade_date='2026-10-05',settlement='2026-10-07',on_payment='Exclude',credit_identity=None,quote_kind=dict(kind='CleanPrice' if label=='clean' else 'DirtyPrice' if label=='dirty' else 'CashflowYield'),quote=dict(value=99.5 if label!='percent_yield' else 2.5,unit='PricePer100' if label!='percent_yield' else 'PercentRate'),tolerance=1e-8);pins=clone(product['bond']['calendars']);inputs=clone(base['inputs']);origin='2026-10-05'
                    if label=='percent_yield':h['quote_kind']['convention']=dict(day_count='Act365Fixed',compounding='SemiAnnual',lower_yield=-.15,upper_yield=.15,max_iterations=128,price_tolerance=1e-8)
                id='cash_basis_hedge.'+rlabel+'.'+('reference' if reference_only else 'credit')+'.'+label
                legal=dict(instrument_id=h['id'],instrument_version=17 if label=='icma' else 2,reference=dict(id='ACTUAL.SUPPLIED.LEGAL',version=3),entity_id='ACME',seniority='SeniorUnsecured',currency='USD',maturity=h['terms']['schedule']['end_date']);h['credit_identity']=clone(legal)
                modelid=REFMODEL if reference_only else MODEL
                basis=dict(pin=dict(id='B.ACME',version=1),currency='USD',entity_id='ACME',seniority='SeniorUnsecured',recovery_fraction=float(r),model_version=modelid,reference_date=origin,dated_spreads=[[origin,dict(value=float(spread),unit='DecimalRate')],['2030-12-20',dict(value=float(spread),unit='DecimalRate')]],liquidity_assumption=dict(id='EXPLICIT.LIQUIDITY',version=4))
                term=clone(cds)
                if label=='icma':term.update(trade_date=origin,step_in_date='2024-01-11',protection_start_date='2024-01-11',cash_settlement_date='2024-01-15');term['premium_schedule'].update(start_date='2023-12-20',end_date='2025-12-20')
                for cal in [term['premium_calendar'],term['settlement_calendar']]:
                    if not any(c['pin']==cal for c in inputs['calendars']):inputs['calendars'].append(dict(pin=cal,weekend_days=[5,6],holidays=[]))
                mapped=dict(mapping=dict(pin=dict(id='EXPLICIT.BOND.CDS.MAPPING',version=5),bond=clone(legal),cds_reference=reference,credit_curve=clone(term['survival_curve']),basis=clone(basis['pin']),liquidity_assumption=clone(basis['liquidity_assumption'])),bond=clone(legal),cds=term,credit=clone(term['survival_curve']),basis=basis)
                inputs['curves']=[curve(name,z,origin,reference,modelid) for name,z in [('D.OIS',discount),('P.3M',D('.035')),('H.ACME',hazard),('B.ACME',spread)]];inputs['calibration']=None;inputs['fixings']=[];inputs['publications']=[]
                fullmodel=dict(kind='ReferenceZeroSpread',reference=dict(id='D.OIS',version='1'),role=dict(kind='Discount')) if reference_only else dict(kind='CreditParRecovery',discount=dict(id='D.OIS',version='1'),survival=dict(id='H.ACME',version='1'),seniority='senior_unsecured',recovery=float(r))
                manifest=dict(identity=dict(id=id,instrument=dict(id=h['id'],version=legal['instrument_version']),economics=dict(id='FULL.TERMS.'+id,version=1),calculation=dict(id='FULL.CALC.'+id,version=1),entity='DESK.A'),reference_position=dict(quantity=h['terms']['face_value'],reference_quantity=h['terms']['face_value'],quantity_unit='notional',currency='USD'),snapshot_id=SNAPSHOT,factors=[factor(k) for k in ['zero','hazard','basis','recovery','unrelated']],max_factor_evaluations=25)
                req=dict(schema_version=1,inputs=inputs,operation=dict(kind='cash_basis_hedge_risk',graph=dict(rates=[],credit=[],cash=[],rate_context=None,controls=clone(base['operation']['graph']['controls'])),security=dict(helper=h,calendars=pins),input=dict(basis=clone(KEY),model=fullmodel,credit=mapped,basis_tolerance=1e-10,hedge=manifest)))
                holidays=next(c['holidays'] for c in inputs['calendars'] if c['pin']==pins['payment']);p=project(h,holidays);v=value(h,p,discount,hazard,spread,r,reference_only);modelquote,modelicma=quote(h,p,D(v['dirty_per100']));observed=quote(h,p)
                factors=[]
                for kind,f in zip(['zero','hazard','basis','recovery','unrelated'],manifest['factors']):
                    amounts=[]
                    for move in [D(0),D('.0001'),D('-.0001'),D('.00005'),D('-.00005')]:
                        d=discount+move if kind=='zero' else discount;q=hazard+move if kind=='hazard' else hazard;b=spread+move if kind=='basis' else spread;rec=r+move if kind=='recovery' else r
                        amounts.append(D(value(h,p,d,q,b,rec,reference_only)['present_value']))
                    unit=D(1) if kind in ['hazard','recovery'] else D('.0001')
                    factors.append(dict(prices=list(map(str,amounts)),value=str((amounts[1]-amounts[2])/(2*D('.0001'))*unit)))
                answer=dict(cash=p,valuation=v,observed=observed,model_quote=str(modelquote),model_icma=modelicma,factors=factors,parameters=dict(discount=str(discount),hazard=str(hazard),basis=str(spread),recovery=str(r)))
                add(id,req,answer)
    seed=cases[0]['request'];mutations=[
        ('no_mapping',lambda r:r['operation']['input'].update(credit=None),'invalid_input'),
        ('missing_mapping',lambda r:r['operation']['input']['credit'].update(mapping=None),'invalid_input'),
        ('wrong_security',lambda r:r['operation']['input']['hedge']['identity']['instrument'].update(version=99),'invalid_input'),
        ('wrong_bond_reference',lambda r:r['operation']['input']['credit']['bond']['reference'].update(version=99),'invalid_input'),
        ('wrong_document',lambda r:r['operation']['input']['credit']['cds']['reference']['document'].update(version=99),'invalid_input'),
        ('wrong_recovery',lambda r:r['operation']['input']['model'].update(recovery=.5),'invalid_input'),
        ('wrong_basis_pin',lambda r:r['operation']['input']['credit']['basis']['pin'].update(version=99),'invalid_input'),
        ('wrong_basis_value',lambda r:r['operation']['input']['credit']['basis']['dated_spreads'][1][1].update(value=.02),'invalid_input'),
        ('basis_tolerance',lambda r:r['operation']['input'].update(basis_tolerance=1e-8),'invalid_input'),
        ('no_basis_curve',lambda r:r['inputs']['curves'].pop(),'missing_input'),
        ('no_survival_curve',lambda r:r['inputs']['curves'].pop(2),'missing_input'),
        ('source_calendar',lambda r:r['operation']['security']['calendars']['payment'].update(version=99),'missing_input'),
        ('settlement',lambda r:r['operation']['security']['helper'].update(settlement='2026-10-08'),'invalid_input'),
        ('delayed_principal',lambda r:r['operation']['security']['helper']['terms']['dates'].update(payment_delay=dict(CalendarDays=1)),'invalid_input'),
        ('wrong_face',lambda r:r['operation']['input']['hedge']['reference_position'].update(quantity='3000',reference_quantity='3000'),'invalid_input'),
        ('signed_reference',lambda r:r['operation']['input']['hedge']['reference_position'].update(quantity='-5000'),'invalid_input'),
        ('wrong_currency',lambda r:r['operation']['input']['hedge']['reference_position'].update(currency='EUR'),'invalid_input'),
        ('factor_work',lambda r:r['operation']['input']['hedge'].update(max_factor_evaluations=24),'invalid_input'),
        ('missing_factors',lambda r:r['operation']['input']['hedge'].update(factors=[]),'invalid_input'),
        ('duplicate_factor',lambda r:r['operation']['input']['hedge']['factors'].append(clone(r['operation']['input']['hedge']['factors'][0])),'invalid_input'),
        ('hazard_domain',lambda r:r['operation']['input']['hedge']['factors'][1]['settings'].update(bump=.1),'invalid_input'),
        ('recovery_domain',lambda r:r['operation']['input']['hedge']['factors'][3]['settings'].update(bump=.5),'invalid_input'),
        ('calibrated_node_rebuild',lambda r:r['operation']['input']['hedge']['factors'][0]['key']['coordinate']['curve'].update(version='99'),'missing_input'),
        ('unknown_field',lambda r:r['operation']['input'].update(legacy_yield=0.05),'invalid_input'),
    ]
    for label,mutate,error in mutations:
        req=clone(seed);mutate(req);add('cash_basis_hedge.invalid.'+label,req,error=error)
    icma=clone(next(c['request'] for c in cases if c['id']=='cash_basis_hedge.positive.credit.icma'))
    for label,mutate,error in [
        ('icma_security',lambda r:r['operation']['security']['helper']['quote_kind']['input']['sources']['security'].update(version=99),'invalid_input'),
        ('icma_context',lambda r:r['operation']['security']['helper']['quote_kind']['input']['reference_context']['periods'].pop(5),'invalid_input'),
        ('icma_work',lambda r:r['operation']['security']['helper']['quote_kind']['input'].update(max_cash_evaluations=100),'resource_limit'),
    ]:
        req=clone(icma);mutate(req);add('cash_basis_hedge.invalid.'+label,req,error=error)
    controls=dict(schema_version=1,certified=False,precision=60,source_reads={p:hashlib.sha256((ROOT/p).read_bytes()).hexdigest() for p in SOURCES},tolerances=dict(cash='1e-8',price='1e-8',pv='1e-8',native_quote='1e-8',yield_rate='1e-9',risk='1e-6',time='1e-12',relative='0'),qualification='Predeclared independent complete synthetic economics; no native captured answer, external bond comparator or independent human/model/production certificate.',cases=answers)
    return dict(schema_version=1,certified=False,cases=cases),controls

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--write',action='store_true');a=parser.parse_args()
    for path,data in zip(['cash-basis-hedge-api.json','cash-basis-hedge-reference.json'],generate()):
        target=FOLDER/path;payload=(json.dumps(data,indent=2,default=str)+'\n').encode()
        if a.write:
            if target.exists():
                if target.read_bytes()!=payload: raise ValueError('Refusing to overwrite any existing control')
            else: target.write_bytes(payload)
        elif target.read_bytes()!=payload: raise ValueError('Independent controls differ')
        print(json.dumps(dict(status='passed',path=path,cases=len(data['cases']),certified=False)))
