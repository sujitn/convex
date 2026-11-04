#!/usr/bin/env python3
"""Independent Decimal(60) actual-cash signed-risk references; never runs Convex.

Full source terms/calendar/date/face are retained. Existing reference functions
are pure manual mathematics, read unchanged; no native candidate value is read.
"""
from pathlib import Path
from decimal import Decimal as D, getcontext
import argparse, copy, hashlib, json
import cash_calibration_reference as cash

getcontext().prec=60
ROOT=Path(__file__).resolve().parents[2]
DEST=ROOT/'tests/fixtures/risk/quote-risk.json'
SOURCES=['tests/fixtures/curves/cash-curves.json','tests/scripts/cash_calibration_reference.py']
def build():
    data=json.loads((ROOT/SOURCES[0]).read_text());row=data['cases'][0]
    gov=next(r for r in row['recipes'] if r['curve']['role']['kind']=='Government')
    helper=next(h['input'] for h in gov['helpers'] if h['kind']=='Bond' and h['input']['quote_kind']['kind']=='CleanPrice')
    helper=copy.deepcopy(helper);helper['quote_kind']=cash.yield_kind();helper['quote']=dict(value=3.1,unit='PercentRate')
    c=cash.cash(helper['terms'],helper['settlement'],set(row['holidays']))
    outputs=[]
    for h in map(D,['0.00005','0.0001','0.0002']):
        y=D('0.031');face=c['face']
        pv=lambda d:cash.yield_price(c,helper['settlement'],y+d)*face/100
        base,plus,minus,hp,hm=[pv(d) for d in [D(0),h,-h,h/2,-h/2]]
        value=(plus-minus)/(2*h)*D('0.0001');half=(hp-hm)/h*D('0.0001')
        outputs.append(dict(bump=str(h),base=str(base),plus=str(plus),minus=str(minus),half_plus=str(hp),half_minus=str(hm),value=str(value),half_value=str(half)))
    result=dict(schema_version=1,certified=False,qualification='Independent synthetic engineering reference. No human quantitative approval or market/model/host/library/production/latency certificate.',
        method='Decimal60 full entitled stepped/amortizing cash, actual settlement-to-payment ACT365F semiannual cashflow yield, unrounded absolute PV, signed central and independent half-bump normalization.',
        tolerances=dict(pv_absolute='0.00000001',risk_absolute='0.00000001',integration_stability_absolute='0.05',integration_stability_relative='0.0001',bucket_parallel_absolute='0.10',bucket_parallel_relative='0.0001'),
        source_reads={p:hashlib.sha256((ROOT/p).read_bytes()).hexdigest() for p in SOURCES},helper=helper,holidays=row['holidays'],cases=outputs)
    return (json.dumps(result,indent=2)+'\n').encode()
def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('mode',choices=['generate','check']);a=p.parse_args();data=build()
    if a.mode=='generate':DEST.parent.mkdir(parents=True,exist_ok=True);DEST.write_bytes(data)
    elif DEST.read_bytes()!=data:raise ValueError('independent quote risk reference bytes differ')
    print(json.dumps(dict(status='passed',mode=a.mode,cases=3,sha256=hashlib.sha256(data).hexdigest(),certified=False)))
if __name__=='__main__':main()
