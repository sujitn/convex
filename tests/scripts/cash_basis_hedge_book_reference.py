"""Predeclared complete credit-cash/CDS/IRS/futures mixed-book controls.

Uses only independent financial primitives and original full-term controls.
Every intended book factor is calculated by each product; recovery is tested
separately in cash columns and is explicitly outside this mixed book's factors.
Known four-column target is fixed before native execution, including integer
contracts, full current/pending, exact native side costs and USD/EUR conversion.
"""
import argparse
from copy import deepcopy as clone
from datetime import date
from decimal import Decimal as D
import hashlib,json
from pathlib import Path
import graph_hedge_reference as graph
from graph_hedge_cash_reference import price as cds_price
from cash_basis_hedge_reference import project, value, quote
from constrained_hedge_reference import SETTINGS
ROOT=Path(__file__).resolve().parents[2];FOLDER=ROOT/'tests/fixtures/library'
SOURCES=['tests/fixtures/library/cash-basis-hedge-api.json','tests/fixtures/library/graph-hedge-api.json','tests/scripts/graph_hedge_reference.py','tests/scripts/graph_hedge_cash_reference.py','tests/scripts/cash_basis_hedge_reference.py','tests/scripts/constrained_hedge_reference.py']

def generate():
    raw=json.loads((ROOT/SOURCES[0]).read_bytes())['cases'];old=json.loads((ROOT/SOURCES[1]).read_bytes())['cases'];req=clone(raw[0]['request']);op=req['operation'];h=op['security']['helper'];id='cash_basis_hedge.book.cash';op['input']['hedge']['identity']['id']=id
    # Same original flat product controls; every column uses this one curve set.
    for c in req['inputs']['curves']:
        c['reference_date']='2026-10-07'
        if c['key']['id']=='D.OIS':c['source']['Nodes']=[dict(pillar=dict(Tenor=t),quote=dict(value=float((-D('.032')*D(t)).exp()),unit='DiscountFactor')) for t in [0,50]]
        if c['key']['id']=='P.3M':
            for n in c['source']['Nodes']:n['quote']['value']=.041
    b=op['input']['credit']['basis'];b['reference_date']='2026-10-07';b['dated_spreads'][0][0]='2026-10-07'
    del op['input']['hedge']['factors'][3];op['input']['hedge']['max_factor_evaluations']=20;factors=clone(op['input']['hedge']['factors']);snapshot=op['input']['hedge']['snapshot_id']
    holidays=next(c['holidays'] for c in req['inputs']['calendars'] if c['pin']==op['security']['calendars']['payment']);p=project(h,holidays);v=value(h,p,D('.032'),D('.02'),D('.0025'),D('.4'));mq,mi=quote(h,p,D(v['dirty_per100']))
    a=dict(cash=p,valuation=v,observed=quote(h,p),model_quote=str(mq),model_icma=mi,factors=[])
    for kind in ['zero','hazard','basis','unrelated']:
        vals=[D(value(h,p,D('.032')+(m if kind=='zero' else 0),D('.02')+(m if kind=='hazard' else 0),D('.0025')+(m if kind=='basis' else 0),D('.4'))['present_value']) for m in [D(0),D('.0001'),D('-.0001'),D('.00005'),D('-.00005')]]
        a['factors'].append(dict(prices=list(map(str,vals)),value=str((vals[1]-vals[2])/(2*D('.0001'))*(1 if kind=='hazard' else D('.0001')))))
    cases=[dict(id=id,request=req,expected_error=None)];refs=[dict(id=id,expected=a)]
    for kind in ['cds','irs','term_future']:
        source=next(c for c in old if c['id']=='graph_hedge.positive.'+kind+'.base');r=clone(source['request']);r['inputs']['curves']=clone(req['inputs']['curves']);o=r['operation'];o['graph']=clone(op['graph']);o['input']['factors']=clone(factors);o['input']['snapshot_id']=snapshot;o['input']['max_factor_evaluations']=20;id='cash_basis_hedge.book.'+kind;o['input']['identity']['id']=id
        history={(x['index_id'],date.fromisoformat(x['observation_date'])):D(x['rate']) for x in r['inputs']['fixings']}
        def price(discount,hazard,projection):
            if kind=='cds':return cds_price(o['product']['terms'],discount,hazard)
            if kind=='irs':return graph.swap_price(o['product']['terms'],discount,projection,history)
            return graph.future_price(o['product']['input'],projection,history)
        answers=[]
        for coordinate in ['zero','hazard','basis','unrelated']:
            vals=[price(D('.032')+(m if coordinate=='zero' else 0),D('.02')+(m if coordinate=='hazard' else 0),D('.041')+(m if coordinate=='unrelated' else 0)) for m in [D(0),D('.0001'),D('-.0001'),D('.00005'),D('-.00005')]]
            pv=[D(x['pv']) for x in vals];unit=1 if coordinate=='hazard' else D('.0001')
            answers.append(dict(valuations=vals,value=str((pv[1]-pv[2])/(2*D('.0001'))*unit),half_value=str((pv[3]-pv[4])/D('.0001')*unit)))
        cases.append(dict(id=id,request=r,expected_error=None));refs.append(dict(id=id,expected=dict(factors=answers)))
    values=[[D(f['value']) for f in r['expected']['factors']] for r in refs];ids=[c['id'] for c in cases];current=[D(0)]*4;pending=current[:];qs=[('10000','-2000'),('200000','50000'),('100000','50000')];references=[D(5000),D(1000000),D(1000000),D(1)]
    for j,(q,pend) in enumerate(qs):
        for i,x in enumerate(values[j]):current[i]+=x*D(q)/references[j];pending[i]+=x*D(pend)/references[j]
    before=[x+y for x,y in zip(current,pending)];xs=list(map(D,['-.6','.08','-.2','2']));after=[x+sum((xs[j]*values[j][i] for j in range(4)),D(0)) for i,x in enumerate(before)]
    templates=[]
    for currency,fx in [('USD',D(1)),('EUR',D('.9'))]:
        actual=[]
        for i,f in enumerate(factors):actual.append(dict(key=clone(f['key']),risk_settings=clone(f['settings']),credit=clone(op['input']['credit']['cds']['reference']) if i in [1,2] else None,scale=1000000.0 if i==1 else 100.0,weight=1.0,target=float(after[i]*fx),lower=-1000000.0 if i==1 else -1000.0,upper=1000000.0 if i==1 else 1000.0))
        templates.append(dict(id='cash_basis_hedge.mixed.'+currency,snapshot_id=snapshot,entity='DESK.A',settlement='2026-10-07',reporting_currency=currency,factors=actual,holdings=[dict(column=id,current=q,pending=pend) for id,(q,pend) in zip(ids,qs)],candidates=[dict(column=id,lot='1' if j==3 else None,buy_per_reference='3',sell_per_reference='2',limit='5' if j==3 else '10000' if j==0 else '1000000') for j,id in enumerate(ids)],fx=None if currency=='USD' else dict(from_='USD',to='EUR',rate=str(fx),observation=dict(id='EXPLICIT.SAME.SNAPSHOT.FX',version=1),snapshot_id=snapshot),settings=clone(SETTINGS),max_cost=10.0,expected=dict(current=list(map(str,(x*fx for x in current))),pending=list(map(str,(x*fx for x in pending))),before=list(map(str,(x*fx for x in before))),quantities=['-3000','80000','-200000','2'],final_holdings=['5000','330000','-50000','2'],after=list(map(str,(x*fx for x in after))),total_cost=str(D('7.84')*fx))))
    provenance=dict(certified=False,source_reads={p:hashlib.sha256((ROOT/p).read_bytes()).hexdigest() for p in SOURCES},qualification='Full independent actual cash/CDS/IRS/futures columns and declared four-factor mixed book. Recovery remains separately tested and outside this book. Original solver policies remain; financial application limits/scales are explicitly supplied. No native captured answer or production/order authority.')
    return dict(certified=False,cases=cases),dict(**provenance,cases=refs),dict(**provenance,tolerances=dict(risk_absolute='1e-5',rate_risk_absolute='1e-6',hazard_risk_absolute='1e-5',quantity_absolute='1e-4',cost_absolute='1e-8'),cases=templates)
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');args=p.parse_args()
    for name,doc in zip(['cash-basis-hedge-book-columns.json','cash-basis-hedge-book-reference.json','cash-basis-hedge-book-problems.json'],generate()):
        file=FOLDER/name;b=(json.dumps(doc,indent=2,default=str)+'\n').encode()
        if args.write:
            if file.exists():raise ValueError('Refusing to overwrite controls')
            file.write_bytes(b)
        elif file.read_bytes()!=b:raise ValueError('Independent mixed-book controls differ')
        print(json.dumps(dict(status='passed',path=name,cases=len(doc['cases']),certified=False)))
