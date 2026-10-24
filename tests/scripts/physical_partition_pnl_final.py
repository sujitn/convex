"""Unique corpus identities for complete unchanged mixed-book regressions.

The original mixed cash/CTD requests and every financial reference remain exact.
Give their supplemental descriptor IDs an additive physical.regression prefix,
so the operation inventory can retain all original case IDs without duplication.
"""
import argparse,hashlib,json
import physical_partition_pnl_admitted as original
SOURCE=original.TARGET;TARGET=SOURCE.with_name('physical-partition-pnl-final.json')
def generate():
    data=json.loads(SOURCE.read_bytes())
    for c in data['cases']:
        if c['id'].startswith('claim.'):c['id']='physical.regression.'+c['id']
    data.update(reference=__doc__,source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest());return data
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();data=generate();raw=(json.dumps(data,indent=2)+'\n').encode()
    if a.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Complete unique original controls differ')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),certified=False)))
