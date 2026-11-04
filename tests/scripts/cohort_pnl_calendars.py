"""Original exact calendar revisions for every cohort claim and full valuation.

The separate delivery asset keeps the exact original WEEKEND version2 holidays,
including Nov9. Earlier generic claim preparation used an empty holiday set.
Recompute every complete financial book from its actual supplied calendar pin,
full contract/profile/rounding, original unpaid-accrued and original coupon-kind
decomposition. Keep every earlier financial declaration and failed comparison.
No native financial output or changed tolerance is used.
"""
import argparse,hashlib,json
from decimal import Decimal as D,localcontext
import cohort_pnl_reference as r
import cohort_pnl_states as s
import cohort_pnl_contracts as fixed
import cohort_pnl_cash_dates as accrued
import cohort_pnl_bindings as kind
SOURCE=r.TARGET.with_name('cohort-pnl-ownership.json')
TARGET=SOURCE.with_name('cohort-pnl-calendars.json')
def inferred(info,j,opening,owner):
    claims,status,remaining,notices=s.inferred(info,j,opening,owner)
    h=info['holdings'][j];p=h['product'].get('product',h['product'])
    if p['kind']=='bond':
        t=p['bond']['terms'];pin=p['bond']['calendars']['payment']
        holidays=next(c['holidays'] for c in info['inputs']['calendars'] if c['pin']==pin)
        for cash in claims:
            if cash['kind'] in ['bond_coupon','principal']:
                cash['due']=str(r.c.op.cash_date(t,r.c.day(cash['effective']),holidays))
    return claims,status,remaining,notices
def generate():
    data=json.loads(SOURCE.read_bytes())
    with localcontext() as context:
        context.prec=60;prior=(r.c.de.cash,r.native,r.inferred)
        r.c.de.cash=accrued.cash;r.native=fixed.native;r.inferred=inferred
        try:
            for case in data['cases']:
                if case['expected_error'] is not None:continue
                book=s.expand(case['request']);spec=case['request']['operation']['input']
                opening=r.c.ClaimSet([r.rows(t['marks'][0]) for t in case['expected']['trades']]);opening._events=[len(h['lifecycle']['events']) for h in book['before']['holdings']]
                case['expected']=r.financial(book,spec['cohorts'],spec['physical_deliveries'],opening)
                for trade in case['expected']['trades']:
                    for mark in trade['marks']:
                        removed=D(0)
                        for cash in mark['claims']:
                            if cash['key']['kind']=='termination':removed+=D(cash['removed']);cash['removed']=D(0)
                        for name in ['current','projected']:
                            mark['local'][name]+=removed;mark['reporting'][name]+=removed*mark['fx']
                kind.explain(case['expected'],spec,book)
                case['expected']['delivery_comparisons']=accrued.comparisons(case)
        finally:r.c.de.cash,r.native,r.inferred=prior
    data.update(source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),reference=__doc__)
    return data
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');args=p.parse_args();data=generate();raw=(json.dumps(data,indent=2,default=r.c.de.serial)+'\n').encode()
    if args.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Whole exact original calendar financial controls differ')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),positive=sum(x['expected_error'] is None for x in data['cases']),certified=False)))
