"""Bind the original unchanged wire-array exhaustion outcome.

The complete4098-allocation malformed request exceeds the original4096-element
JSON array guard before native custody admission. Its retained trial expected a
native invalid_input outcome. Declare the original resource_limit contract for
this identical malformed request; no input, price, cash, model or bound changes.
"""
import argparse,hashlib,json
import custody_lineage_pnl_units as original
SOURCE=original.TARGET;TARGET=SOURCE.with_name('custody-lineage-pnl-admitted.json')
def generate():
    data=json.loads(SOURCE.read_bytes())
    case=next(c for c in data['cases'] if c['id']=='custody.invalid.available.receipt_work_bound')
    if len(case['request']['operation']['input']['physical_disposals'][0]['receipts'])<=4096:raise ValueError('Original array guard not exceeded')
    case['expected_error']='resource_limit'
    data.update(reference=__doc__,source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest());return data
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();data=generate();raw=(json.dumps(data,indent=2)+'\n').encode()
    if a.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Original unchanged guard outcome differs')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),certified=False)))
