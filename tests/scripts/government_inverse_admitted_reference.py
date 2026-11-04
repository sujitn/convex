"""Preserve raw controls and separately admit original native benchmark domains.

Three raw payment-boundary cases included an already matured benchmark. Those
raw cases/answers remain immutable and now serve as rejection controls. New
identified cases supply a future low security, with independently computed
original-price quotes and new Decimal60 answers before their native comparison.
Float spellings are canonicalized without changing any numerical value.
"""
import argparse
import copy
from decimal import Decimal as D
import json
from pathlib import Path
from government_inverse_reference import cash, day, inverse, price

ROOT = Path(__file__).resolve().parents[2]


def expected(case):
    o=case['request']['operation'];t=o['target']['target'];g=o['settings']['government']
    bs=o['benchmarks'];conv=g['convention'];maturity=t['terms']['schedule']['end_date']
    yields=[inverse(b['quote']['helper'],conv) for b in bs]
    order=sorted(range(len(bs)),key=lambda i:bs[i]['quote']['helper']['terms']['schedule']['end_date'])
    exact=next((i for i in order if bs[i]['quote']['helper']['terms']['schedule']['end_date']==maturity),None)
    if exact is not None:
        lo=exact;hi=None;weight=D(0)
    else:
        lo,hi=next((a,b) for a,b in zip(order,order[1:]) if bs[a]['quote']['helper']['terms']['schedule']['end_date']<maturity<bs[b]['quote']['helper']['terms']['schedule']['end_date'])
        a=day(bs[lo]['quote']['helper']['terms']['schedule']['end_date']);b=day(bs[hi]['quote']['helper']['terms']['schedule']['end_date'])
        weight=D((day(maturity)-a).days)/D((b-a).days)
    benchmark=yields[lo]+(weight*(yields[hi]-yields[lo]) if hi is not None else 0)
    quote=o['quote']['spread'];spread=D(str(quote['value']))/(10000 if quote['unit']=='BasisPoints' else 1)
    y=benchmark+spread;v=price(t,y,conv)
    v.update(benchmark_decimal_yield=str(benchmark),target_decimal_yield=str(y),decimal_spread=str(spread),basis_points=str(spread*10000),
        consideration=str(D(v['dirty_per100'])*D(o['position']['quantity'])/100),benchmark_yields=[str(y) for y in yields],
        lower_index=lo,upper_index=hi,lower_weight=str(1-weight),upper_weight=str(weight),
        target_remaining_days=(day(maturity)-day(t['settlement'])).days)
    return dict(id=case['id'],expected=v,price_absolute=1e-8,accrued_absolute=1e-10,rate_absolute=1e-10,basis_points_absolute=1e-6,money_absolute=1e-6,cash_absolute=1e-20)


def build():
    folder=ROOT/'tests/fixtures/library'
    raw=json.loads((folder/'government-inverse.json').read_bytes())
    reference=json.loads((folder/'government-inverse-reference.json').read_bytes())
    admitted=copy.deepcopy(raw);refs=copy.deepcopy(reference);bad=set();added=[]
    for case in admitted['cases']:
        o=case['request']['operation'];o['quote']['spread']['value']=float(o['quote']['spread']['value'])
        if case['expected_error'] is not None:continue
        t=o['target']['target'];settle=t['settlement']
        expired=[i for i,b in enumerate(o['benchmarks']) if b['quote']['helper']['terms']['schedule']['end_date']<=settle]
        if not expired:continue
        bad.add(case['id']);case['expected_error']='invalid_input'
        new=copy.deepcopy(case);new['id']+='.admitted';new['expected_error']=None
        for i in expired:
            helper=new['request']['operation']['benchmarks'][i]['quote']['helper'];terms=helper['terms'];end='2026-01-01'
            terms['schedule']['end_date']=end;helper['pillar']=end
            for profile in terms['profiles'].values():profile[-1]['end']=end
            dirty=price(helper,D('0.02'),o['settings']['government']['convention'])['dirty_per100']
            helper['quote'].update(value=float(dirty))
        added.append(new)
    refs['cases']=[r for r in refs['cases'] if r['id'] not in bad]
    for endpoint,value in [('lower',-1200.0),('upper',4800.0)]:
        new=copy.deepcopy(next(c for c in admitted['cases'] if c['id']=='gov_inverse.exact'))
        new['id']='gov_inverse.exact.'+endpoint;new['request']['operation']['quote']['spread']['value']=value
        added.append(new)
    new=copy.deepcopy(admitted['cases'][0]);new['id']='gov_inverse.linear.Act360'
    new['request']['operation']['settings']['government']['convention']['day_count']='Act360';added.append(new)
    for case in added:refs['cases'].append(expected(case))
    admitted['cases']+=added
    new=copy.deepcopy(admitted['cases'][0]);new['id']='gov_inverse.failure.exact_maturity';new['expected_error']='invalid_input'
    new['request']['operation']['settings']['government']['selection']='ExactMaturity'
    new['request']['operation']['benchmarks']=new['request']['operation']['benchmarks'][:1];admitted['cases'].append(new)
    new=copy.deepcopy(admitted['cases'][0]);new['id']='gov_inverse.failure.duplicate_maturity';new['expected_error']='invalid_input'
    duplicate=copy.deepcopy(new['request']['operation']['benchmarks'][0]);duplicate['quote']['helper']['id']='synthetic.duplicate.date'
    duplicate['quote']['security']['id']='synthetic.duplicate.date';new['request']['operation']['benchmarks'].append(duplicate)
    admitted['cases'].append(new)
    admitted['qualification']='Raw immutable controls retained separately; explicit old native future-benchmark admission, exact value-preserving float spellings and separately identified Decimal60 boundary controls.'
    refs['qualification']='24 unchanged raw independent answers plus six separately declared Decimal60 admission/boundary answers. Three expired original benchmark inputs are rejected. Original accuracy bounds unchanged; no candidate output.'
    return admitted,refs,bad


def main():
    parser=argparse.ArgumentParser();parser.add_argument('--write',action='store_true');args=parser.parse_args()
    corpus,refs,bad=build()
    for name,value in [('government-inverse-admitted.json',corpus),('government-inverse-admitted-reference.json',refs)]:
        path=ROOT/'tests/fixtures/library'/name;raw=(json.dumps(value,sort_keys=True,separators=(',',':'))+'\n').encode()
        if args.write:
            if path.exists():raise ValueError('Refusing overwrite: '+name)
            path.write_bytes(raw)
        elif path.read_bytes()!=raw:raise ValueError('Admitted control changed: '+name)
    print(json.dumps(dict(status='passed',cases=len(corpus['cases']),independent_cases=len(refs['cases']),retained_raw_rejections=sorted(bad),certified=False)))


if __name__=='__main__':main()
