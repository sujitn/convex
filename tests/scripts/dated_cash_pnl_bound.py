"""Retain complete dated financial bodies and bind original full CTD provenance.

The first declared control omitted the complete actual-vs-model comparison list.
Every price, legal cash fraction, recipient, receipt, tolerance and full request
is unchanged. Reuse the original independent comparison owner; native outputs
are never read. The initial declaration and failed run remain immutable.
"""
import argparse,hashlib,json
import dated_cash_pnl_reference as original
import cohort_pnl_cash_dates as dates
SOURCE=original.TARGET;TARGET=SOURCE.with_name('dated-cash-pnl-bound.json')
def generate():
    data=json.loads(SOURCE.read_bytes())
    for case in data['cases']:
        if case['expected_error'] is None:case['expected']['delivery_comparisons']=dates.comparisons(case)
    data.update(reference=__doc__,source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest());return data
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();data=generate();raw=(json.dumps(data,indent=2)+'\n').encode()
    if a.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Whole original dated financial bodies and comparison facts differ')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),certified=False)))
