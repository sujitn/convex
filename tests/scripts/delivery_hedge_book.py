"""Full five-factor original delivery/cash book and independent close-out math."""
import argparse
import copy
from decimal import Decimal, localcontext
import hashlib
import json
from delivery_hedge_reference import ROOT,HERE,serial,pack,key,settings,graph,position,cash,answer,D,fraction,date,SETTLEMENT

TARGET=HERE/'delivery-hedge-book.json'
TEMPLATE='tests/fixtures/library/optional-graph-hedge-policy-book.json'


def cash_boundary(member,curve,move=D(0)):
    # Actual book boundary is 2026-10-07; basket purchase is the following day.
    # There is no payment on either boundary. Reuse every independent dated cash
    # amount, then normalize the SAME discounted cash to its actual earlier anchor.
    original=cash(member,curve,move);t=fraction(date(curve['reference_date']),SETTLEMENT)
    knots=curve['source']['Nodes'];left=D(str(knots[0]['quote']['value']))+move
    right=D(str(knots[1]['quote']['value']))+move;z=left+(right-left)*t/D(4)
    pv=original['pv']*(-z*t).exp()
    start=max(c['start'] for c in original['coupons'] if c['start'] < date('2026-10-07'))
    terms=member['bond']['terms'];face=D(terms['face_value']);accrued=D(0)
    for p in terms['profiles']['coupon']:
        a=max(start,date(p['start']));b=min(date('2026-10-07'),date(p['end']))
        if a<b: accrued+=face*D(p['value'])*fraction(a,b)
    return dict(pv=pv,dirty=pv/face*100,clean=(pv-accrued)/face*100,accrued=accrued/face*100,
        coupons=original['coupons'],cashflows=original['cashflows'])


def generate():
    template=json.loads((ROOT/TEMPLATE).read_bytes())['cases'][0]['problem'];cases=[]
    with localcontext() as c:
        c.prec=60
        resources,product=pack(control='switch');future=product['input']
        factor_names=['discount','repo_a','repo_b','basis_a','basis_b']
        factors=[dict(key=key(name,future),settings=settings('0.000005' if name=='discount' else '.01' if name.startswith('basis') else '.0002')) for name in factor_names]
        identity=dict(id='delivery.book.future',instrument=dict(id=future['contract']['instrument_id'],version=4),
            economics=dict(id='delivery.book.future.full-original-terms-sources-state',version=1),
            calculation=dict(id='delivery.book.future.full-fixed-date-model-v1',version=1),entity='DELIVERY.DESK')
        hedge_input=dict(identity=identity,reference_position=position(),snapshot_id='S.DELIVERY',factors=factors,max_factor_evaluations=25)
        future_request=dict(schema_version=1,inputs=resources,operation=dict(kind='graph_hedge_risk',graph=graph(),product=product,input=hedge_input))
        member=future['members'][0];terms=member['bond']['terms'];face=terms['face_value']
        cash_product=dict(kind='bond',bond=member['bond'],context=dict(settlement='2026-10-07',on_payment='Exclude',
            fixing=dict(valuation_date='2026-10-07',known_at='2026-10-07T12:00:00Z',unpublished='Forecast',final_only=True),overnight_accrual_cutoffs=[]),
            projection=None,method=dict(kind='discount_spread',curve=product['discount']['curve'],role=dict(kind='collateral'),decimal_spread=0.0))
        cash_input=copy.deepcopy(hedge_input);cash_input['identity']=dict(id='delivery.book.cash',instrument=member['instrument'],
            economics=dict(id='delivery.book.cash.full-original-terms-state',version=1),calculation=dict(id='delivery.book.cash.discount-model-v1',version=1),entity='DELIVERY.DESK')
        cash_input['reference_position']=dict(quantity=face,reference_quantity=face,quantity_unit='notional',currency='USD')
        cash_request=dict(schema_version=1,inputs=resources,operation=dict(kind='graph_hedge_risk',graph=graph(),product=cash_product,input=cash_input))
        future_answers=[answer(product,resources,f['key'],f['settings']) for f in factors]
        cash_answers=[]
        for name,f in zip(factor_names,factors):
            h=D(str(f['settings']['bump']));values=[cash_boundary(member,resources['curves'][0],d if name=='discount' else D(0)) for d in [D(0),h,-h,h/2,-h/2]]
            money=[v['pv'] for v in values];size=D('.0001') if f['key']['unit']=='currency_per_bp' else D(1)
            cash_answers.append(dict(base_pv=money[0],plus_pv=money[1],minus_pv=money[2],half_plus_pv=money[3],half_minus_pv=money[4],
                value=(money[1]-money[2])/(2*h)*size,half_value=(money[3]-money[4])/h*size,evaluations=values))
        for currency,fx in [('USD',D(1)),('EUR',D('.9'))]:
            problem=copy.deepcopy(template);problem.update(snapshot_id='S.DELIVERY',entity='DELIVERY.DESK',settlement='2026-10-07',reporting_currency=currency,
                expected_holdings=['delivery.book.cash','delivery.book.future'],max_cost='10')
            problem['factors']=[dict(key=f['key'],risk_settings=f['settings'],credit=None,scale=float(scale),weight=1.0,target=0.0,lower=0.0,upper=0.0)
                for f,scale in zip(factors,[D(1),D('.01'),D('.01'),D(1000),D(1000)])]
            problem['holdings']=[dict(column='delivery.book.cash',current_quantity='300',pending_quantity='50'),
                dict(column='delivery.book.future',current_quantity='2',pending_quantity='1')]
            for i,candidate in enumerate(problem['candidates']):
                candidate['column']=problem['expected_holdings'][i]
                candidate['costs']['observation']=dict(id='SYNTH.DELIVERY.SIDE.COSTS',version=2)
                candidate['eligibility'].update(reason='Explicit synthetic supplied candidate universe; proposal only',decision=dict(id='SYNTH.DELIVERY.UNIVERSE',version=3))
            problem['position_state']=dict(id='SYNTH.DELIVERY.CURRENT.PENDING',version=4)
            current=[(a['value']*D(300)/D(face)+b['value']*2)*fx for a,b in zip(cash_answers,future_answers)]
            pending=[(a['value']*D(50)/D(face)+b['value'])*fx for a,b in zip(cash_answers,future_answers)]
            expected=dict(current=current,pending=pending,before=[a+b for a,b in zip(current,pending)],after=[D(0)]*5,
                quantities=['-350','-3'],holdings=['0','0'],cost=(D('1.25')+D('2')*3)*fx,rank=2)
            cases.append(dict(id='delivery.book.'+currency,columns=[dict(id='delivery.book.cash',request=cash_request,expected=cash_answers),
                dict(id='delivery.book.future',request=future_request,expected=future_answers)],problem=problem,expected=expected,
                fx=None if currency=='USD' else {'from':'USD','to':'EUR','rate':'.9','snapshot_id':'S.DELIVERY','source':dict(id='SYNTH.DELIVERY.FX',version=5)}))
    return dict(certified=False,qualification='Full independently controlled original current/pending/lot/side-cost/eligibility/limit/FX five-factor cash and all-member delivery books. Unique exact close-out follows nonzero repo/basis future factors then cash discount; rank2. No native answer is read.',
        source_reads={TEMPLATE:hashlib.sha256((ROOT/TEMPLATE).read_bytes()).hexdigest()},cases=cases)

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();value=generate();raw=(json.dumps(value,indent=2,default=serial)+'\n').encode()
    if a.write:
        if TARGET.exists(): raise ValueError('Refusing to overwrite complete independent book')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw: raise ValueError('Original book controls differ')
    print(json.dumps(dict(status='passed',books=len(value['cases']),certified=False)))
