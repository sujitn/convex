#!/usr/bin/env python3
"""Independent rational controls for explicit 360/fixed-365 bill conversions."""
import argparse
from decimal import Decimal, localcontext
from fractions import Fraction
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
TARGET = ROOT / 'tests/fixtures/library/bill-methods-reference.json'


def build():
    def decimal(value):
        return str(Decimal(value.numerator) / Decimal(value.denominator))
    cash, rates = [], []
    with localcontext() as context:
        context.prec = 60
        for days in [1, 20, 90, 181, 182, 183, 364, 365]:
            for face, price in [('100', '97'), ('100', '100'), ('100', '103'),
                                ('1000', '981.23456789'), ('0.001', '0.000975')]:
                f, p = Fraction(face), Fraction(price)
                cash.append(dict(price=price, face=face, days=days,
                                 bank_discount_decimal=decimal((1-p/f)*Fraction(360,days)),
                                 money_market_decimal=decimal((f/p-1)*Fraction(360,days)),
                                 simple_fixed365_decimal=(decimal((f/p-1)*Fraction(365,days))
                                                          if days <= 182 else None)))
        for days in [1, 20, 90, 181, 182, 183, 365]:
            for rate in ['-3.9', '-0.5', '0', '0.02', '0.2', '100000000']:
                r = Fraction(rate)
                admissible = days <= 182 and 1 + r*Fraction(days,360) > 0
                rates.append(dict(money_market_decimal=rate, days=days,
                                  simple_fixed365_decimal=(decimal(r*Fraction(365,360))
                                                           if admissible else None)))
    return dict(schema_version=1,units='All rates are decimal fractions; analytics percent helpers multiply exactly once by100.',
                convention='Explicit 360 bank discount on face and money market on price; fixed365 simple rate admits1 through182remaining days. Treasury investment-rate quotes use the separate original-date owner.',
                provenance='Exact rational expressions evaluated independently at Decimal60 before candidate edits; no native import/output.',
                absolute_tolerance='1e-22',relative_tolerance='1e-24',cash_cases=cash,rate_cases=rates,
                invalid_cash=[dict(price=p,face=f,days=d) for p,f,d in [('0','100',90),('-1','100',90),('97','0',90),('97','-100',90),('97','100',0),('97','100',-1)]],
                invalid_bond_facts=['before_issue','at_maturity','after_maturity','zero_issue_interval','non_face_redemption','nonpositive_face','nonpositive_price'],
                overflow_inputs=[dict(price='0.0000000000000000000000000001',face='100',days=1)],
                primary_sources=['https://www.treasurydirect.gov/files/laws-and-regulations/auction-regulations-uoc/31-cfr-part-356.pdf',
                                 'https://www.treasurydirect.gov/files/laws-and-regulations/uoc-noncomp-increase.pdf'])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action',choices=['write','check'])
    args = parser.parse_args()
    raw = (json.dumps(build(),indent=2)+'\n').encode()
    if args.action=='write':
        with TARGET.open('xb') as stream:
            stream.write(raw)
    elif TARGET.read_bytes()!=raw:
        raise ValueError('Original independent bill-method declaration changed')
    print(json.dumps(dict(status='passed',independent_cash_cases=40,independent_rate_cases=42)))


if __name__=='__main__':
    main()
