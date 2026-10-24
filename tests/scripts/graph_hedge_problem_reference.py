"""Predeclared independent mixed-book/current/pending/cost/FX hedge controls.

Every column uses the separately pinned Decimal60 full financial reference.
Targets are constructed from specified original trades, making the identified
two-column zero-tracking solution independently known before native execution.
"""
from copy import deepcopy
from decimal import Decimal as D
import argparse
import hashlib
import json
from pathlib import Path
from constrained_hedge_reference import SETTINGS

ROOT=Path(__file__).resolve().parents[2]
DEST=ROOT/'tests/fixtures/library/graph-hedge-problems.json'
SOURCES=['tests/fixtures/library/graph-hedge-reference.json','tests/fixtures/library/graph-hedge.json','tests/scripts/constrained_hedge_reference.py']


def build():
    source=json.loads((ROOT/SOURCES[0]).read_bytes());corpus=json.loads((ROOT/SOURCES[1]).read_bytes())['cases']
    ids=['graph_hedge.positive.'+s+'.base' for s in ['bond','cds','irs','term_future']]
    controls={row['id']:row['expected'] for row in source['cases']};requests={row['id']:row['request'] for row in corpus}
    quantities=[('5000','-1000'),('200000','50000'),('100000','50000')]
    current=[D(0)]*4;pending=current[:]
    for id_,(q,p) in zip(ids,quantities):
        ref=D(requests[id_]['operation']['input']['reference_position']['reference_quantity'])
        for i,f in enumerate(controls[id_]['factors']):
            current[i]+=D(f['value'])*D(q)/ref;pending[i]+=D(f['value'])*D(p)/ref
    before=[a+b for a,b in zip(current,pending)]
    trades=[D('-0.2'),D('2')];target=before[:]
    for id_,x in zip(ids[2:],trades):
        for i,f in enumerate(controls[id_]['factors']):target[i]+=D(f['value'])*x
    factors=[]
    for i,f in enumerate(requests[ids[0]]['operation']['input']['factors']):
        factors.append(dict(key=deepcopy(f['key']),risk_settings=deepcopy(f['settings']),credit=deepcopy(requests[ids[1]]['operation']['product']['terms']['reference']) if i==3 else None,
            scale=100000.0 if i==3 else 1.0,weight=1.0,target=float(target[i]),lower=-1000000000.0,upper=1000000000.0))
    cases=[]
    for currency,fx in [('USD',D(1)),('EUR',D('0.9'))]:
        actual_factors=deepcopy(factors)
        for i,f in enumerate(actual_factors):f['target']=float(target[i]*fx)
        cases.append(dict(id='graph_hedge.mixed.'+currency,column_ids=ids,reporting_currency=currency,
            snapshot_id=requests[ids[0]]['operation']['input']['snapshot_id'],entity='DESK.A',settlement='2026-10-07',
            factors=actual_factors,holdings=[dict(column=id_,current=q,pending=p) for id_,(q,p) in zip(ids,quantities)],
            candidates=[dict(column=id_,lot=None if i==0 else '1',buy_per_reference='3',sell_per_reference='2',limit='1000000' if i==0 else '5') for i,id_ in enumerate(ids[2:])],
            fx=None if currency=='USD' else dict(from_='USD',to=currency,rate=str(fx),observation=dict(id='FULL.REPORTING.FX',version=1),snapshot_id=requests[ids[0]]['operation']['input']['snapshot_id']),
            settings=deepcopy(SETTINGS),max_cost=10.0,
            expected=dict(current=[str(x*fx) for x in current],pending=[str(x*fx) for x in pending],before=[str(x*fx) for x in before],
                quantities=['-200000','2'],final_holdings=['-50000','2'],after=[str(x*fx) for x in target],total_cost=str(D('6.4')*fx))))
    return dict(schema_version=1,certified=False,qualification='Complete actual product columns replace independently calculated reference columns only in execution. Known identified target uses explicit original trades; no native answer read. All original solver policies remain unchanged.',source_reads={p:hashlib.sha256((ROOT/p).read_bytes()).hexdigest() for p in SOURCES},tolerances=source['tolerances'],cases=cases)


if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--write',action='store_true');a=parser.parse_args();data=(json.dumps(build(),indent=2)+'\n').encode()
    if a.write:
        if DEST.exists():raise ValueError('Refusing to overwrite original mixed-book control')
        DEST.write_bytes(data)
    elif DEST.read_bytes()!=data:raise ValueError('Independent mixed-book controls differ')
    print(json.dumps(dict(status='passed',cases=2,certified=False)))
