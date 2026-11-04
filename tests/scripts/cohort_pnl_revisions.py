"""Explicit original numeric curve/source revision binding for physical points.

All earlier complete financial/input trials remain. Whole physically supplied
curves already have independent values; give the market revision a distinct
numeric token and bind the original futures discount source stamp exactly. No
price, contract, observation, calendar, source value or financial bound changes.
"""
import argparse,hashlib,json
import cohort_pnl_reference as r
SOURCE=r.TARGET.with_name('cohort-pnl-currencies.json');TARGET=SOURCE.with_name('cohort-pnl-revisions.json')
def replace(v):
    if isinstance(v,dict):
        if set(v)=={'id','version'} and v['version']=='causal.market.2':v['version']='2001'
        for x in v.values():replace(x)
    elif isinstance(v,list):
        for x in v:replace(x)
def generate():
    data=json.loads(SOURCE.read_bytes())
    for case in data['cases']:
        spec=case['request']['operation']['input'];replace(spec)
        for h in spec['book']['holdings']:
            p=h['product'].get('product',h['product'])
            if p['kind']=='bond_future':p['discount']['stamp']['version']=int(p['discount']['curve']['version'])
    data.update(source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),reference=__doc__)
    return data
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');args=p.parse_args();data=generate();raw=(json.dumps(data,indent=2)+'\n').encode()
    if args.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Exact whole curve/source binding differs')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),positive=sum(x['expected_error'] is None for x in data['cases']),certified=False)))
