"""Exact original Money unit and original collateral-bound request serialization.

Retain earlier full input/financial trials. Add only the required original money
unit to actual cash/fill/termination receipts; monetary answers/bounds are exact.
No candidate monetary output, economic simplification or paid-state relabelling.
"""
import argparse,hashlib,json
import cohort_pnl_reference as r
SOURCE=r.TARGET.with_name('cohort-pnl-states.json');TARGET=SOURCE.with_name('cohort-pnl-wire.json')
def units(v):
    if isinstance(v,dict):
        if set(v)=={'value','currency'}:v['unit']='currency'
        for x in v.values():units(x)
    elif isinstance(v,list):
        for x in v:units(x)
def generate():
    data=json.loads(SOURCE.read_bytes())
    for case in data['cases']:units(case['request'])
    data.update(source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),reference=__doc__)
    return data
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');args=p.parse_args();data=generate();raw=(json.dumps(data,indent=2)+'\n').encode()
    if args.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Original exact unit wire declaration differs')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),positive=sum(x['expected_error'] is None for x in data['cases']),certified=False)))
