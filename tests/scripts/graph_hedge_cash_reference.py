"""Separate full actual CDS accrual chronology; raw failed answers stay pinned.

Intermediate accrual ends and subsequent starts are Following-adjusted. The raw
control assumed unadjusted twentieths. Direct Decimal60 integration and bounds
stay; no Convex execution/output is read. All sixteen non-CDS answers stay exact.
"""
from copy import deepcopy
from datetime import date, timedelta
from decimal import Decimal as D, getcontext
import argparse, hashlib, json
from pathlib import Path
import graph_hedge_reference as raw
import graph_hedge_problem_reference as problems
getcontext().prec=60
ROOT=Path(__file__).resolve().parents[2];FOLDER=ROOT/'tests/fixtures/library'
SOURCES=['tests/fixtures/library/graph-hedge-reference.json','tests/fixtures/library/graph-hedge-terms.json',
         'tests/scripts/graph_hedge_reference.py','tests/scripts/graph_hedge_problem_reference.py']

def price(t,discount,hazard):
    k=(discount+hazard)/365
    def primitive(a,b):
        ea=(-k*a).exp();eb=(-k*b).exp()
        return (ea-eb)/k,((a+1/k)*ea-(b+1/k)*eb)/k
    first=date.fromisoformat(t['premium_schedule']['start_date']);last=date.fromisoformat(t['premium_schedule']['end_date'])
    grid=[first]
    while grid[-1]<last:grid.append(date.fromisoformat(raw.cash.months(str(grid[-1]),3)))
    n=D(t['notional']);coupon=D(t['coupon_decimal']);sign=D(1 if t['side']=='ProtectionBuyer' else -1)
    rows=[];scheduled=default=D(0);start=first
    for end in grid[1:]:
        payment=raw.rates.following(end);actual_end=end+timedelta(days=1) if end==last else payment
        origin=D((start-raw.REF).days-1);stop=D((actual_end-raw.REF).days-1);yf=D((actual_end-start).days)/360
        s=yf*(-hazard*stop/365).exp()*raw.df(payment,discount)
        i0,i1=primitive(max(origin,D(0)),stop);ad=hazard/365*(i1+(-origin+D('0.5'))*i0)/360
        scheduled+=s;default+=ad
        rows.append(dict(accrual_start=str(start),accrual_end=str(actual_end),payment_date=str(payment),conditional_coupon_amount=str(-sign*n*coupon*yf),scheduled_pv=str(-sign*n*coupon*s),accrual_on_default_pv=str(-sign*n*coupon*ad)))
        start=actual_end
    i0,_=primitive(D(0),D((last-raw.REF).days))
    protection=sign*n*(1-D(str(t['reference']['recovery_fraction'])))*hazard/365*i0;premium=-sign*n*coupon*(scheduled+default)
    return dict(pv=str(protection+premium),protection=str(protection),premium=str(premium),premiums=rows)

def build():
    controls=deepcopy(json.loads((ROOT/SOURCES[0]).read_bytes()));requests={r['id']:r['request'] for r in json.loads((ROOT/SOURCES[1]).read_bytes())['cases']};amended=[]
    for row in controls['cases']:
        request=requests[row['id']];p=request['operation']['product']
        if p['kind']!='cds':continue
        zeros=[D(v) for v in (['0.032','0.041','0.037','0.02'] if '.positive.' in row['id'] else ['-0.005','-0.003','-0.002','0.02'])];factors=[]
        for i,f in enumerate(request['operation']['input']['factors']):
            h=D(str(f['settings']['bump']));values=[]
            for move in [D(0),h,-h,h/2,-h/2]:
                z=zeros[:];z[i]+=move;values.append(price(p['terms'],z[0],z[3]))
            pv=[D(v['pv']) for v in values];unit=D(1) if i==3 else D('0.0001')
            factors.append(dict(valuations=values,value=str((pv[1]-pv[2])/(2*h)*unit),half_value=str((pv[3]-pv[4])/h*unit)))
        row['expected']=dict(factors=factors);amended.append(row['id'])
    controls['actual_accrual_boundary_controls']=dict(raw_control_sha256=hashlib.sha256((ROOT/SOURCES[0]).read_bytes()).hexdigest(),separate_case_ids=amended,raw_failed_answers_preserved=True)
    controls['source_reads'].update({p:hashlib.sha256((ROOT/p).read_bytes()).hexdigest() for p in SOURCES})
    controls['qualification']+=' Separate four full actual Following-accrual CDS controls replace no raw file or bound. Sixteen non-CDS answers stay identical.'
    return controls

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--write',action='store_true');a=parser.parse_args()
    def save(name,value):
        dest=FOLDER/name;data=(json.dumps(value,indent=2)+'\n').encode()
        if a.write:
            if dest.exists():raise ValueError('Refusing to overwrite separate controls')
            dest.write_bytes(data)
        elif dest.read_bytes()!=data:raise ValueError('Independent actual-cash controls differ')
    save('graph-hedge-actual-cash-reference.json',build())
    problems.SOURCES=['tests/fixtures/library/graph-hedge-actual-cash-reference.json','tests/fixtures/library/graph-hedge-terms.json','tests/scripts/constrained_hedge_reference.py']
    value=problems.build();value['source_reads'].update({p:hashlib.sha256((ROOT/p).read_bytes()).hexdigest() for p in SOURCES})
    save('graph-hedge-actual-cash-problems.json',value)
    print(json.dumps(dict(status='passed',full_controls=20,original_non_cds_answers_preserved=16,separate_actual_accrual_cds_controls=4,mixed_books=2,certified=False)))
