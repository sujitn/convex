"""Bind the original full contractual bond reference to the unchanged new inputs.

The first generic reference omitted the actual discount-spread/full schedule
owner. Retain both earlier declarations and their failed trials. Recompute only
the six new full books with the original complete stepped/amortizing coupon,
principal, actual payment calendar and spread owner, never candidate output.
Every full request, cash fact, numerical bound and prior mixed-book body remains.
"""
import argparse,hashlib,json
from decimal import localcontext
import dated_cash_pnl_reference as original
import cohort_pnl_reference as r
import cohort_pnl_contracts as contracts
import cohort_pnl_cash_dates as dates
SOURCE=original.TARGET.with_name('dated-cash-pnl-bound.json');TARGET=SOURCE.with_name('dated-cash-pnl-contracts.json')
def generate():
    data=json.loads(SOURCE.read_bytes());by_id={c['id']:c for c in data['cases']};old=r.native
    with localcontext() as ctx:
        ctx.prec=60;r.native=contracts.native
        try:
            for sign in [1,-1]:
                for mode in ['coupon','amortization','redemption']:
                    c=original.make(sign,mode);previous=by_id[c['id']]
                    if c['request']!=previous['request']:raise ValueError('Original full request changed')
                    c['expected']['delivery_comparisons']=dates.comparisons(c)
                    previous['expected']=c['expected']
        finally:r.native=old
    data.update(reference=__doc__,source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest());return data
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();data=generate();raw=(json.dumps(data,indent=2,default=r.c.de.serial)+'\n').encode()
    if a.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Complete original contract reference differs')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),certified=False)))
