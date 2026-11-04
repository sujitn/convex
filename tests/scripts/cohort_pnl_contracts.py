"""Complete original fixed-bond dates/daycounts/profiles/rounding in every cohort.

Prior generic flat cash preparations and failed original comparisons remain
immutable. Recompute all new books from full original stepped/amortizing coupon
and principal, payment delays THEN actual calendar adjustment, cash rounding and
discount spread. Original remaining credit/rates/optional/basket owners, complete
states/claims/CTD/P&L and bounds remain. No candidate output is read or fitted.
"""
import argparse,copy,hashlib,json
from decimal import Decimal as D,localcontext
import cohort_pnl_reference as r
import cohort_pnl_states as s
SOURCE=r.TARGET.with_name('cohort-pnl-extended.json');TARGET=SOURCE.with_name('cohort-pnl-contracts.json')
original=r.native
def native(info,h):
    p=h['product'].get('product',h['product'])
    if p['kind']!='bond':return original(info,h)
    t=p['bond']['terms'];on=info['point']['valuation_date'];pin=p['bond']['calendars']['payment'];holidays=next(x['holidays'] for x in info['inputs']['calendars'] if x['pin']==pin);flows=r.c.bond_flows(t,h['lifecycle']['history']);z=r.c.curve_zero(info['inputs'],p['method']['curve']['id'])+D(str(p['method']['decimal_spread']));cash=[]
    for flow in flows:
        due=str(r.c.op.cash_date(t,r.c.day(flow['effective']),holidays))
        if due>on:cash.append(dict(date=due,amount=flow['amount']))
    cash.sort(key=lambda x:x['date']);pv=sum((x['amount']*(-z*D((r.c.day(x['date'])-r.c.day(on)).days)/365).exp() for x in cash),D(0))
    return dict(kind='bond',pv=pv,full=dict(cashflows=cash))
def generate():
    data=json.loads(SOURCE.read_bytes())
    with localcontext() as context:
        context.prec=60;old_native=r.native;old_claims=r.inferred;r.native=native;r.inferred=s.inferred
        try:
            for case in data['cases']:
                if case['expected_error'] is not None:continue
                book=s.expand(case['request']);spec=case['request']['operation']['input'];opening=r.c.ClaimSet([r.rows(t['marks'][0]) for t in case['expected']['trades']]);opening._events=[len(h['lifecycle']['events']) for h in book['before']['holdings']];case['expected']=r.financial(book,spec['cohorts'],spec['physical_deliveries'],opening)
        finally:r.native=old_native;r.inferred=old_claims
    data.update(source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),reference=__doc__)
    return data
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');args=p.parse_args();data=generate();raw=(json.dumps(data,indent=2,default=r.c.de.serial)+'\n').encode()
    if args.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Original full contract financial controls differ')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),positive=sum(x['expected_error'] is None for x in data['cases']),certified=False)))
