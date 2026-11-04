#!/usr/bin/env python3
"""Independent Decimal80 cash/DF controls, declared before native comparison."""
from decimal import Decimal as D, localcontext
import argparse
import json
from pathlib import Path


def reference():
    with localcontext() as ctx:
        ctx.prec = 80
        def price(rate):
            return -D(403)-D('600.06')*(-rate*D(3)/D(365)).exp()
        values = {name:price(rate) for name,rate in [('base_pv',D('.033')),('plus_pv',D('.0331')),
            ('minus_pv',D('.0329')),('half_plus_pv',D('.03305')),('half_minus_pv',D('.03295'))]}
        values['value']=(values['plus_pv']-values['minus_pv'])/2
        values['half_value']=values['half_plus_pv']-values['half_minus_pv']
        return dict(schema_version=1, provenance='Independent Decimal80: explicit synthetic ACT/360 fixed cash, no native imports',
            absolute_bound='0.0000001',relative_bound='0',units=dict(cash='USD',risk='signed USD per +1bp continuous collateral zero'),
            terms=dict(face='1000',held='-1000',first_period_days=30,last_period_days=1,rate='.036',
                principal_after_step='600',rounding='NearestEven2',payment_delay_calendar_days=1,payment_adjustment='Following',
                maturity='2026-10-09',maturity_valuation='2026-10-09',collateral_rate='.033',collateral_time='ACT/365F'),
            claims=[dict(kind='bond_coupon',effective='2026-10-08',ordinal=0,amount='-3',due='2026-10-09'),
                dict(kind='principal',effective='2026-10-08',ordinal=0,amount='-400',due='2026-10-09'),
                dict(kind='bond_coupon',effective='2026-10-09',ordinal=1,amount='-0.06',due='2026-10-12'),
                dict(kind='principal',effective='2026-10-09',ordinal=1,amount='-600',due='2026-10-12')],
            expected_maturity_current={key:str(value) for key,value in values.items()},
            expected_native_coupon_cash=dict(swap_fixed=str((-D(1000)*D('.036')*D(31)/D(360)).quantize(D('.01'))),swap_floating=str((D(1000)*D('.04')*D(31)/D(360)).quantize(D('.01'))),cds_final=str((-D(1000)*D('.01')*D(32)/D(360)).quantize(D('.01')))),
            expected_due_current='-1003.06',expected_received_before_actual_payment='0',expected_remaining_after_amortization='1',expected_outstanding_principal_after_amortization='600')


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('mode',choices=('write','check'));args=parser.parse_args()
    path=Path(__file__).resolve().parents[1]/'fixtures/library/date-rolls-reference.json'
    raw=(json.dumps(reference(),indent=2,sort_keys=True)+'\n').encode()
    if args.mode=='write':
        if path.exists():raise RuntimeError('Reference already exists; never refresh it silently')
        path.write_bytes(raw)
    elif path.read_bytes()!=raw:raise RuntimeError('Declared date-roll reference changed')
    print(json.dumps(dict(status='passed',mode=args.mode)))
