"""Independent Decimal60 complete-cash/common-factor controls; no Convex execution.

Frozen full supplied terms and independent calendar/profile routines are read.
New controls use flat continuous discount/projection and exponential survival;
CDS default/accrual integrals are evaluated directly in days, without a grid.
Native captured results are never read by this writer.
"""
from copy import deepcopy
from datetime import date, timedelta
from decimal import Decimal as D, getcontext
import argparse
import hashlib
import json
from pathlib import Path
import cash_calibration_reference as cash
import rate_calibration_reference as rates

getcontext().prec = 60
ROOT = Path(__file__).resolve().parents[2]
FOLDER = ROOT / 'tests/fixtures/library'
SOURCES = ['tests/fixtures/library/cases.json', 'tests/fixtures/curves/mixed-rates.json',
           'tests/scripts/cash_calibration_reference.py', 'tests/scripts/rate_calibration_reference.py']
REF = date(2026, 10, 7)
SNAPSHOT = 'graph-hedge.synthetic.snapshot.1'
KEYS = ['D.OIS', 'P.3M', 'P.SOFR', 'H.ACME']
pin = rates.pin


def factor(name):
    hazard = name == 'H.ACME'
    return dict(key=dict(factor=dict(family='hazard' if hazard else 'interest_rate',
        target=dict(kind='curve', curve_id=name), currency='USD', tenor='parallel',
        quote_type='hazard_rate' if hazard else 'zero_rate', basis='zero_node',
        convention_version=1, model_version='graph-hedge.synthetic.full-model.1'),
        coordinate=dict(kind='hazard' if hazard else 'zero', curve=dict(id=name, version='1'), node=None),
        native_unit='DecimalRate', response='frozen_curves',
        unit='currency_per_annual_rate' if hazard else 'currency_per_bp'),
        settings=dict(bump=0.0001, stability=dict(absolute=1.0 if hazard else 0.05,
        relative=0.0001), policy_version=1))


FACTORS = [factor(name) for name in KEYS]


def curve(name, value, reference):
    credit = name == 'H.ACME'
    role = (dict(kind='Credit', entity_id=reference['entity_id'], seniority=reference['seniority'], recovery=reference['recovery_fraction'])
            if credit else dict(kind='Discount', collateral_currency='USD') if name == 'D.OIS'
            else dict(kind='Projection', index_id='USD.TERM3M' if name == 'P.3M' else 'USD.SOFR'))
    # CDS's exact original model requires log-linear DF and survival inputs.
    prob = credit or name == 'D.OIS'
    return dict(key=dict(id=name, version='1'), role=role, currency='USD',
        reference_date=str(REF), time_basis='Act365Fixed',
        representation='SurvivalProbabilities' if credit else 'DiscountFactors' if prob else dict(ZeroRates=dict(compounding='Continuous')),
        interpolation='LogLinear' if prob else 'Linear', extrapolation='None', dependencies=[], calibration='Direct',
        source=dict(Nodes=[dict(pillar=dict(Tenor=t), quote=dict(value=float((-D(value)*D(t)).exp()) if prob else float(value),
        unit='SurvivalProbability' if credit else 'DiscountFactor' if prob else 'DecimalRate')) for t in [0, 50]]))


def df(on, zero):
    return (-zero*D((on-REF).days)/365).exp()


def forward(a, b, zero):
    return ((zero*D((b-a).days)/365).exp()-1)/rates.tau(a, b)


def swap_price(t, discount, projection, history):
    fixed = t['fixed']; floating = t['floating']; start = date.fromisoformat(t['effective_date']); end = date.fromisoformat(t['maturity_date'])
    fixed_grid = [start, REF, end] if start < REF else [start, end]
    fixed_rows = []; float_rows = []
    sign = D(-1 if t['side'] == 'PayFixed' else 1)
    for a, b in zip(fixed_grid, fixed_grid[1:]):
        cuts = sorted({a, b} | {date.fromisoformat(p[k]) for ps in [fixed['leg']['notionals'], fixed['rates']] for p in ps for k in ['start', 'end'] if a < date.fromisoformat(p[k]) < b})
        amount = sum((rates.at(fixed['leg']['notionals'], x)*rates.at(fixed['rates'], x)*rates.tau(x, y) for x,y in zip(cuts,cuts[1:])), D(0))*sign
        payment = rates.following(rates.offset(b, 2))
        fixed_rows.append(dict(payment=str(payment), amount=str(amount), pv=str(amount*df(payment, discount))))
    if floating['kind'] == 'term':
        grid = [REF, date(2027,1,7), date(2027,4,7), date(2027,7,7), end]
    else:
        grid = [start, REF, date(2027,1,7), date(2027,4,7), date(2027,7,7), end]
    for a, b in zip(grid, grid[1:]):
        if floating['kind'] == 'term':
            observation = rates.offset(a, -2)
            index = floating['resets']['index_id']
            value = history[(index, observation)] if observation <= REF else forward(a, b, projection)
            cuts = sorted({a,b} | {date.fromisoformat(v[k]) for p in [floating['leg']['notionals'],floating['profiles']['spreads'],floating['profiles']['gearings']] for v in p for k in ['start','end'] if a<date.fromisoformat(v[k])<b})
            amount = sum((rates.at(floating['leg']['notionals'],x)*(rates.at(floating['profiles']['gearings'],x)*value+rates.at(floating['profiles']['spreads'],x))*rates.tau(x,y) for x,y in zip(cuts,cuts[1:])),D(0))
        else:
            compound = D(1)
            for observation, rate_end, x, y in rates.intervals(a, b):
                value = history[('USD.SOFR',observation)] if ('USD.SOFR',observation) in history else forward(observation,rate_end,projection)
                compound *= 1+value*rates.tau(x,y)
            amount = rates.at(floating['leg']['notionals'],a)*rates.at(floating['profiles']['gearings'],a)*(compound-1)+rates.weighted(floating['leg']['notionals'],a,b)*rates.at(floating['profiles']['spreads'],a)
        amount *= -sign; payment = rates.following(rates.offset(b, 2))
        float_rows.append(dict(payment=str(payment),amount=str(amount),pv=str(amount*df(payment,discount))))
    return dict(pv=str(sum((D(r['pv']) for r in fixed_rows+float_rows),D(0))), fixed=fixed_rows, floating=float_rows)


def cds_price(t, discount, hazard):
    # Direct primitive antiderivatives in days; no native integration knots read.
    k=(discount+hazard)/365
    def integrals(a,b):
        ea=(-k*a).exp(); eb=(-k*b).exp()
        return (ea-eb)/k, ((a+1/k)*ea-(b+1/k)*eb)/k
    boundaries=[date(2026,9,20)]; last=date.fromisoformat(t['premium_schedule']['end_date'])
    while boundaries[-1]<last:
        boundaries.append(date.fromisoformat(cash.months(str(boundaries[-1]),3)))
    n=D(t['notional']); coupon=D(t['coupon_decimal']); sign=D(1 if t['side']=='ProtectionBuyer' else -1)
    premiums=[]; scheduled=default=D(0)
    for a,b in zip(boundaries,boundaries[1:]):
        accrual_end=b+timedelta(days=1) if b==last else b
        origin=D((a-REF).days-1); end=D((accrual_end-REF).days-1)
        payment=rates.following(b); yf=D((accrual_end-a).days)/360
        s=yf*(-hazard*end/365).exp()*df(payment,discount)
        i0,i1=integrals(max(origin,D(0)),end)
        ad=hazard/365*(i1+(-origin+D('0.5'))*i0)/360
        scheduled+=s; default+=ad
        premiums.append(dict(accrual_start=str(a),accrual_end=str(accrual_end),payment_date=str(payment),
            conditional_coupon_amount=str(-sign*n*coupon*yf),scheduled_pv=str(-sign*n*coupon*s),accrual_on_default_pv=str(-sign*n*coupon*ad)))
    i0,_=integrals(D(0),D((last-REF).days))
    protection=sign*n*(1-D(str(t['reference']['recovery_fraction'])))*hazard/365*i0
    premium=-sign*n*coupon*(scheduled+default)
    return dict(pv=str(protection+premium),protection=str(protection),premium=str(premium),premiums=premiums)


def future_price(input_, projection, history):
    s=input_['contract']['settlement']; a=s['accrual']; rows=[]
    if a['kind']=='term':
        observation=date.fromisoformat(a['fixing']); start=date.fromisoformat(a['start']); end=date.fromisoformat(a['end'])
        value=history[(a['index'],observation)] if observation<=REF else forward(start,end,projection)
        rate=value
        rows.append(dict(observation=str(observation),rate=str(value)))
    else:
        start=date.fromisoformat(a['start']);end=date.fromisoformat(a['end']);total=D(0);compound=D(1)
        for observation,rate_end,x,y in rates.intervals(start,end):
            value=history[(a['index'],observation)] if (a['index'],observation) in history else forward(observation,rate_end,projection)
            total+=value*rates.tau(x,y);compound*=1+value*rates.tau(x,y)
            rows.append(dict(observation=str(observation),rate=str(value)))
        rate=((compound-1) if a['averaging']=='compounded' else total)/rates.tau(start,end)
    adjustment=D(s['convexity'].get('decimal_rate','0'));price=100*(1-(rate-adjustment))
    return dict(pv=str(price*D(input_['contract']['currency_per_price_point'])),price=str(price),observations=rows)


def build():
    original=json.loads((ROOT/SOURCES[0]).read_bytes())['cases']; mixed=json.loads((ROOT/SOURCES[1]).read_bytes())['cases'][0]
    lookup={r['id']:r['request'] for r in original}; full=deepcopy(lookup['calibration.quote_risk']['inputs']); full['curves']=[]
    graph=dict(rates=[],credit=[],cash=[],rate_context=None,controls=deepcopy(lookup['calibration.quote_risk']['operation']['input']['controls']))
    bond_op=deepcopy(lookup['bond.construct']['operation']); bond_op['context']['settlement']=str(REF);bond_op['context']['fixing']['valuation_date']=str(REF);bond_op['context']['fixing']['known_at']='2026-10-07T12:00:00Z'
    # Add the actual original bond calendars to the same explicit information set.
    full['calendars']+=deepcopy(lookup['bond.construct']['inputs']['calendars'])
    bond=dict(kind='bond',bond=bond_op['bond'],context=bond_op['context'],projection=None,
        method=dict(kind='discount_spread',curve=dict(id='D.OIS',version='1'),role=dict(kind='collateral'),decimal_spread=0.0012))
    swaps=[dict(kind='swap',terms=deepcopy(lookup[id_]['operation']['terms']),context=deepcopy(lookup[id_]['operation']['context'])) for id_ in ['swap.1.2.price','swap.0.2.price']]
    credit_op=deepcopy(lookup['cds.price']['operation']);ct=credit_op['terms'];ct.update(trade_id='graph-hedge.CDS',trade_date=str(REF),step_in_date='2026-10-08',protection_start_date='2026-10-08',cash_settlement_date='2026-10-13',notional='1000000',discount_curve=dict(id='D.OIS',version='1'),survival_curve=dict(id='H.ACME',version='1'),premium_calendar=pin('PAYMENT'),settlement_calendar=pin('PAYMENT'))
    ct['premium_schedule'].update(start_date=str(REF),end_date='2028-12-20')
    ct['reference']['entity_id']='ACME'
    cds=dict(kind='cds',terms=ct,context=dict(valuation_date=str(REF),step_in_date='2026-10-08',cash_settlement_date='2026-10-13',initial_payment_state='NotContracted'))
    fi=deepcopy(lookup['rate_future.price']['operation']['input']);fi['projection']['curve']=dict(id='P.SOFR',version='1');fi['projection']['stamp']['id']='P.SOFR'
    fi['fixing_stamp']=rates.stamp('graph-hedge.fixings');fi['calendar_stamp']=rates.stamp('CME-calendar-reference')
    term=deepcopy(fi); ft=mixed['term_future_terms'];term.update(contract=ft['contract'],clearing=ft['clearing'],convexity_model=ft['convexity_model'],projection=dict(curve=ft['projection_curve'],stamp=ft['projection_stamp']))
    products={'bond':bond,'irs':swaps[0],'ois':swaps[1],'cds':cds,'overnight_future':dict(kind='rate_future',input=fi),'term_future':dict(kind='rate_future',input=term)}
    history={(r['index_id'],date.fromisoformat(r['observation_date'])):D(r['rate']) for r in full['fixings']}
    cases=[];references=[]
    def add(id_,request,error=None,expected=None):
        cases.append(dict(id=id_,request=request,expected_error=error))
        if expected is not None:references.append(dict(id=id_,expected=expected))
    for regime in ['positive','negative_rates']:
        zeros=[D(v) for v in (['0.032','0.041','0.037','0.02'] if regime=='positive' else ['-0.005','-0.003','-0.002','0.02'])]
        for name,product_ in products.items():
            product=deepcopy(product_)
            variants=['base','reverse_side'] if name in ['irs','ois','cds'] else ['base','compounded'] if name=='overnight_future' else ['base']
            for variant in variants:
                p=deepcopy(product)
                if variant=='reverse_side':p['terms']['side']='ReceiveFixed' if name!='cds' else 'ProtectionSeller'
                if variant=='compounded':p['input']['contract']['settlement']['accrual']['averaging']='compounded'
                kind=p['kind'];ref='5000' if kind=='bond' else '1000000' if kind in ['swap','cds'] else '1'
                instrument=pin('graph-hedge.supplied.bond') if kind=='bond' else dict(id=p['terms']['trade_id'],version=p['terms']['version']) if kind in ['swap','cds'] else dict(id=p['input']['contract']['instrument_id'],version=p['input']['contract']['stamp']['version'])
                identity=dict(id='GH.'+name+'.'+variant,instrument=instrument,economics=pin('FULL.TERMS.'+name+'.'+variant),calculation=pin('FULL.CALCULATION.'+name+'.'+variant),entity='DESK.A')
                input_=dict(identity=identity,reference_position=dict(quantity=ref,reference_quantity=ref,quantity_unit='contracts' if kind=='rate_future' else 'notional',currency='USD'),snapshot_id=SNAPSHOT,factors=deepcopy(FACTORS),max_factor_evaluations=20)
                resources=deepcopy(full);resources['curves']=[curve(n,z,ct['reference']) for n,z in zip(KEYS,zeros)]
                request=dict(schema_version=1,inputs=resources,operation=dict(kind='graph_hedge_risk',graph=deepcopy(graph),product=p,input=input_))
                def price(z):
                    if kind=='bond':
                        flows=cash.cash(p['bond']['terms'],str(REF),set(lookup['bond.construct']['inputs']['calendars'][0]['holidays']))
                        pv=sum((amount*df(date.fromisoformat(on),z[0]+D(str(p['method']['decimal_spread']))) for on,amount in flows['flows']),D(0))
                        return dict(pv=str(pv),cashflows=[dict(date=on,amount=str(amount)) for on,amount in flows['flows']])
                    if kind=='swap':return swap_price(p['terms'],z[0],z[1] if name=='irs' else z[2],history)
                    if kind=='cds':return cds_price(p['terms'],z[0],z[3])
                    return future_price(p['input'],z[1] if name=='term_future' else z[2],history)
                answers=[]
                for i,f in enumerate(FACTORS):
                    h=D(str(f['settings']['bump']));values=[]
                    for delta in [D(0),h,-h,h/2,-h/2]:
                        changed=zeros[:];changed[i]+=delta;values.append(price(changed))
                    pv=[D(v['pv']) for v in values];unit=D(1) if i==3 else D('0.0001')
                    answers.append(dict(valuations=values,value=str((pv[1]-pv[2])/(2*h)*unit),half_value=str((pv[3]-pv[4])/h*unit)))
                add('graph_hedge.'+regime+'.'+name+'.'+variant,request,expected=dict(factors=answers))
    base=deepcopy(cases[0]['request'])
    for name,error,mutate in [
        ('empty_factors','invalid_input',lambda x:x['operation']['input'].update(factors=[])),
        ('too_many_factors','invalid_input',lambda x:x['operation']['input'].update(factors=deepcopy(FACTORS)*5)),
        ('duplicate_factor','invalid_input',lambda x:x['operation']['input']['factors'].append(deepcopy(FACTORS[0]))),
        ('work','invalid_input',lambda x:x['operation']['input'].update(max_factor_evaluations=19)),
        ('unbounded_work','invalid_input',lambda x:x['operation']['input'].update(max_factor_evaluations=81)),
        ('signed_reference','invalid_input',lambda x:x['operation']['input']['reference_position'].update(quantity='-5000')),
        ('wrong_currency','invalid_input',lambda x:x['operation']['input']['reference_position'].update(currency='EUR')),
        ('wrong_reference','invalid_input',lambda x:x['operation']['input']['reference_position'].update(quantity='1000',reference_quantity='1000')),
        ('stability','invalid_input',lambda x:x['operation']['input']['factors'][0]['settings']['stability'].update(absolute=0.0,relative=0.0)),
        ('missing_calculation','invalid_input',lambda x:x['operation']['input']['identity'].pop('calculation')),
        ('unknown_input','invalid_input',lambda x:x['operation']['input'].update(cached_risk=0)),
        ('yield_alias','not_applicable',lambda x:x['operation']['product'].update(method=dict(kind='yield',decimal_rate=0.04,convention=dict(day_count='Act365Fixed',compounding='SemiAnnual',lower_yield=-0.9,upper_yield=1.,max_iterations=256,price_tolerance=1e-11)))),
    ]:
        request=deepcopy(base);mutate(request);add('graph_hedge.invalid.'+name,request,error)
    for name in ['irs','cds','term_future']:
        source=next(r['request'] for r in cases if r['id']=='graph_hedge.positive.'+name+'.base')
        for suffix,mutate in [('instrument',lambda x:x['operation']['input']['identity']['instrument'].update(version=2)),
                              ('quantity_unit',lambda x:x['operation']['input']['reference_position'].update(quantity_unit='trade_copies'))]:
            request=deepcopy(source);mutate(request);add('graph_hedge.invalid.'+name+'.'+suffix,request,'invalid_input')
    # Rebuilt quote route uses the unchanged existing full graph owner. Its
    # five-price agreement is an owner/transport control, not an independent oracle.
    source=deepcopy(lookup['calibration.quote_risk']);action=source['operation']['action'];p=action['product']
    source['operation']=dict(kind='graph_hedge_risk',graph=source['operation']['input'],product=p,input=dict(
        identity=dict(id='GH.rebuilt.IRS',instrument=dict(id=p['terms']['trade_id'],version=p['terms']['version']),economics=pin('FULL.rebuilt.IRS'),calculation=pin('CALCULATION.rebuilt.IRS'),entity='DESK.A'),
        reference_position=dict(quantity='1000000',reference_quantity='1000000',quantity_unit='notional',currency='USD'),snapshot_id=action['snapshot_id'],
        factors=[dict(key=action['key'],settings=action['settings'])],max_factor_evaluations=5))
    add('graph_hedge.rebuilt.irs',source)
    controls=dict(schema_version=1,certified=False,qualification='Independent synthetic engineering controls, not human approval or market/model/production certification. Rebuilt route has separate unchanged-owner agreement only.',
        source_reads={p:hashlib.sha256((ROOT/p).read_bytes()).hexdigest() for p in SOURCES},
        tolerances=dict(pv_absolute=1e-7,rate_risk_absolute=1e-7,hazard_risk_absolute=0.001,quantity_absolute=0.02,cost_absolute=1e-6),cases=references)
    corpus=dict(schema_version=1,certified=False,qualification=controls['qualification'],cases=cases)
    return {'graph-hedge.json':corpus,'graph-hedge-reference.json':controls}


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--write',action='store_true');args=parser.parse_args()
    result=build()
    for name,value in result.items():
        data=(json.dumps(value,indent=2)+'\n').encode();path=FOLDER/name
        if args.write:
            if path.exists():raise ValueError('Refusing to replace declared control: '+name)
            path.write_bytes(data)
        elif path.read_bytes()!=data:raise ValueError('Independent control differs: '+name)
    print(json.dumps(dict(status='passed',requests=len(result['graph-hedge.json']['cases']),independent_full_controls=len(result['graph-hedge-reference.json']['cases']),certified=False)))


if __name__=='__main__':main()
