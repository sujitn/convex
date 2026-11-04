#!/usr/bin/env python3
"""Independent Decimal80 cash and collateral-risk declarations, no native calls."""
from decimal import Decimal as D, localcontext
import argparse
import json
from pathlib import Path


def reference():
    with localcontext() as context:
        context.prec = 80
        expected = {}
        for trade, recovery, earned in (('bond', D('-1200') * D('.35'), D('-21.35')),
                                        ('second_bond', D('2400') * D('.35'), D('42.70')),
                                        ('cds', D('-450000') * (1-D('.40')), D('218.75'))):
            def price(rate):
                df = (-rate * D(14) / D(365)).exp()
                return (recovery + earned)*df if trade == 'cds' else recovery*df + earned
            values = dict(base_pv=price(D('.03')), plus_pv=price(D('.0301')), minus_pv=price(D('.0299')),
                          half_plus_pv=price(D('.03005')), half_minus_pv=price(D('.02995')))
            values['value'] = (values['plus_pv']-values['minus_pv'])/2
            values['half_value'] = values['half_plus_pv']-values['half_minus_pv']
            expected[trade] = dict(recovery_cash=str(recovery), earned_cash=str(earned),
                                   prices={k: str(v) for k, v in values.items()})
        return dict(schema_version=1, provenance='Independent Decimal80; explicit synthetic principal-only bond and CDS actual cash; retained native contracts',
            absolute_bound='0.0000001', relative_bound='0', units=dict(cash='USD', risk='signed USD per +1bp continuous collateral zero'),
            assumptions=dict(issuer='ACME', default_event='core.issuer.default.1', default_date='2026-10-07', cash_date='2026-10-21',
                collateral_rate='.03', day_count='ACT/365F', bond_earned_cash='due on default date; nominal',
                bond_recovery='.35', cds_recovery='.40',
                cds_accrual='450000*.01*17.5/360 rounded NearestEven2', parent_contagion=False), expected=expected)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('mode', choices=('write', 'check'))
    args = parser.parse_args()
    target = Path(__file__).resolve().parents[1] / 'fixtures/library/issuer-events-contract.json'
    raw = (json.dumps(reference(), indent=2, sort_keys=True) + '\n').encode()
    if args.mode == 'write':
        if target.exists():
            raise RuntimeError('Reference already exists; never silently refresh it')
        target.write_bytes(raw)
    elif target.read_bytes() != raw:
        raise RuntimeError('Declared issuer reference bytes changed')
    print(json.dumps(dict(status='passed', mode=args.mode)))
