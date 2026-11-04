#!/usr/bin/env python3
"""Independent exact-fraction controls for the date-free fixed-365 short bill yield."""
import argparse
from decimal import Decimal, localcontext
from fractions import Fraction
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
TARGET = ROOT / 'tests/fixtures/library/simple-bill-reference.json'


def build():
    cases = []
    with localcontext() as context:
        context.prec = 60
        for days in [1, 20, 90, 181, 182]:
            for face, price in [('100', '97'), ('100', '100'), ('100', '103'),
                                ('1000', '981.23456789'), ('0.001', '0.000975')]:
                rate = (Fraction(face) / Fraction(price) - 1) * Fraction(36500, days)
                expected = Decimal(rate.numerator) / Decimal(rate.denominator)
                cases.append(dict(face=face, price=price, days=days,
                                  expected_percent=str(expected)))
    return dict(schema_version=1, convention='Explicit fixed-365 simple yield in percent; remaining days 1..182. This is not the issue-date Treasury investment-rate convention.',
                equation='(face / price - 1) * 36500 / days',
                derivation='Exact rational arithmetic evaluated independently at Decimal60.',
                absolute_tolerance='1e-22', relative_tolerance='1e-24', cases=cases,
                rejected_days=[183, 184, 364, 365, 366, 4294967295],
                primary_sources=['https://www.treasurydirect.gov/files/laws-and-regulations/auction-regulations-uoc/31-cfr-part-356.pdf',
                                 'https://www.treasurydirect.gov/files/laws-and-regulations/uoc-noncomp-increase.pdf'])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=['write', 'check'])
    args = parser.parse_args()
    data = (json.dumps(build(), indent=2) + '\n').encode()
    if args.action == 'write':
        with TARGET.open('xb') as stream:
            stream.write(data)
    elif TARGET.read_bytes() != data:
        raise ValueError('Original independent short-bill declaration changed')
    print(json.dumps(dict(status='passed',independent_prices=25,rejected_long_terms=6)))


if __name__ == '__main__':
    main()
