#!/usr/bin/env python3
"""Independent exact-rational admissibility at the 360 money-market rate pole."""
import argparse
from decimal import Decimal, localcontext
from fractions import Fraction
import json
from pathlib import Path
TARGET=Path(__file__).resolve().parents[2]/'tests/fixtures/library/bill-methods-boundary-reference.json'


def build():
    cases=[]
    with localcontext() as ctx:
        ctx.prec=60
        for days,pole in [(1,'-360'),(90,'-4'),(180,'-2')]:
            for offset in ['-0.00000000000000000000000001','0','0.00000000000000000000000001']:
                rate=Decimal(pole)+Decimal(offset)
                r=Fraction(rate)
                expected=r*Fraction(365,360)
                cases.append(dict(days=days,money_market_decimal=str(rate),
                                  admissible=1+r*Fraction(days,360)>0,
                                  simple_fixed365_decimal=str(Decimal(expected.numerator)/Decimal(expected.denominator))))
    return dict(schema_version=1,reference='Exact rational positivity of1+rate*days/360; Decimal60 rate answers declared before first native pole comparison.',cases=cases,absolute_tolerance='1e-22',relative_tolerance='1e-24')


def main():
    p=argparse.ArgumentParser();p.add_argument('action',choices=['write','check']);a=p.parse_args()
    raw=(json.dumps(build(),indent=2)+'\n').encode()
    if a.action=='write':
        with TARGET.open('xb') as stream:stream.write(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Independent pole-control bytes changed')
    print(json.dumps(dict(status='passed',independent_pole_cases=9)))


if __name__=='__main__':main()
