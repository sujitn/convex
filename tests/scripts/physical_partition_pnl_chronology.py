"""Separate full actual/model diagnostics for every newly inserted legal point.

The original wire trial retained only the old delivery comparison point list.
Recompute the complete independently declared comparison over every full new
information set, with original all-member winners and actual receipt chronology.
No financial value, original model, expected failure or numerical bound changes.
"""
import argparse,hashlib,json
import physical_partition_pnl_wire as original
import cohort_pnl_cash_dates as dates
SOURCE=original.TARGET;TARGET=SOURCE.with_name('physical-partition-pnl-chronology.json')
def generate():
    data=json.loads(SOURCE.read_bytes())
    for c in data['cases']:
        if c['expected_error'] is None:c['expected']['delivery_comparisons']=dates.comparisons(c)
    data.update(reference=__doc__,source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest())
    return data
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();data=generate();raw=(json.dumps(data,indent=2)+'\n').encode()
    if a.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Complete original chronology differs')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),certified=False)))
