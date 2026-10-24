"""Independent full embedded-cash removal with distinct model/cash discount roles.

The prior delayed-coupon trial retained original cohort cash in native quantity
units but scaled removal a second time by reference face. Preserve that entire
trial. Original full contract cash already includes original cohort face: remove
only its held-plus-receivable share using the actual pricing DF/spread, while
independently owned claims use the collateral cash DF. Excluded claims retain
the original unit reporting ratio. Requests/financial bounds/pricers unchanged.
"""
import argparse, hashlib, json
from decimal import Decimal as D, localcontext
import pending_cash_pnl_admitted as original
import cohort_pnl_reference as r
import cohort_pnl_states as states
import cohort_pnl_bindings as binding
SOURCE=original.TARGET
TARGET=SOURCE.with_name('pending-cash-pnl-discounted.json')
def generate():
    data=json.loads(SOURCE.read_bytes())
    with localcontext() as ctx:
        ctx.prec=60
        for case in data['cases']:
            if case['expected_error'] is not None or 'regression' in case['id']:continue
            spec=case['request']['operation']['input'];book=states.expand(case['request'])
            for i,info in enumerate(r.sets(book)):
                for j,trade in enumerate(case['expected']['trades']):
                    if not j:continue
                    m=trade['marks'][i];pos=info['point']['positions'][j];p=info['holdings'][j]['product']['product']
                    physical=m['physical'];face=D(pos['current']['quantity'])+(D(physical['outstanding_face']) if physical else D(0))
                    share=face/D(spec['cohorts'][j]['quantity'])*D(m['remaining']);delta=D(0)
                    for cash in m['claims']:
                        days=D((r.c.day(cash['due'])-r.c.day(info['point']['valuation_date'])).days)
                        model_zero=r.c.curve_zero(info['inputs'],p['method']['curve']['id'])+D(str(p['method']['decimal_spread']))
                        removed=D(cash['amount'])*share*(-model_zero*days/365).exp() if days>0 and m['status']=='Active' else D(0)
                        delta+=D(cash['removed'])-removed;cash['removed']=removed
                        if not cash['entitled']:cash['ratio']=D(1)
                    for k in ['current','projected']:m['local'][k]=D(m['local'][k])+delta;m['reporting'][k]=D(m['reporting'][k])+delta*D(m['fx'])
            binding.explain(case['expected'],spec,book)
    data.update(reference=__doc__,source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest());return data
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();data=generate();raw=(json.dumps(data,indent=2,default=r.c.de.serial)+'\n').encode()
    if a.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Full original model/cash discount decomposition differs')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),certified=False)))
