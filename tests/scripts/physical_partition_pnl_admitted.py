"""Original physical invoice ownership guard, without weakening admission.

The existing native owner declares a linked delivery with no owned invoice as
InvalidInput, before general complete entitlement coverage. Bind that original
failure contract in a separate whole corpus; retain the earlier MissingInput
trial. Every financial row, input, bound and other failure code is unchanged.
"""
import argparse,hashlib,json
import physical_partition_pnl_bound as original
SOURCE=original.TARGET;TARGET=SOURCE.with_name('physical-partition-pnl-admitted.json')
def generate():
    data=json.loads(SOURCE.read_bytes())
    c=next(c for c in data['cases'] if c['id']=='physical.invalid.missing_cash_entitlement')
    if c['expected_error']!='missing_input':raise ValueError('Original full failure changed')
    c['expected_error']='invalid_input'
    data.update(reference=__doc__,source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest());return data
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();data=generate();raw=(json.dumps(data,indent=2)+'\n').encode()
    if a.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Original complete admitted controls differ')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),certified=False)))
