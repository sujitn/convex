"""Retain the original facade's early complete-work budget diagnostic.

One product evaluation cannot cover the complete supplied book. The original
facade rejects that resources contract as invalid_input before native valuation.
Keep the previous whole request/trial and every financial answer/bound unchanged.
"""
import argparse, hashlib, json
import pending_cash_pnl_final as original
SOURCE=original.TARGET
TARGET=SOURCE.with_name('pending-cash-pnl-bound.json')
def generate():
    data=json.loads(SOURCE.read_bytes())
    c=next(c for c in data['cases'] if c['id']=='pending.invalid.work_bound')
    assert c['request']['operation']['input']['book']['max_product_evaluations']==1
    c['expected_error']='invalid_input'
    data.update(reference=__doc__,source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest());return data
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();data=generate();raw=(json.dumps(data,indent=2)+'\n').encode()
    if a.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Original complete-work diagnostic differs')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),certified=False)))
