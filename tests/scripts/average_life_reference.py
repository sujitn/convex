#!/usr/bin/env python3
"""Independent average lives of bullets and mandatory sinkers, in exact rationals.

Each case states a bond's face, maturity and sinks (date, percent of original face,
price), and a settlement date. The average life is the face each principal change after
settlement retires, times its calendar days from settlement, summed and divided by 365
times the face outstanding. A change is timed at its contractual date, not a business-day
adjusted payment date, and weighted by face, not by the cash its price pays. The exact
ratio is rounded once, nearest/even, to the finest scale of at most 28 places whose
coefficient fits 96 bits: the Decimal the library returns.
"""
import argparse
from datetime import date
from fractions import Fraction
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
TARGET = ROOT / 'crates/convex-bonds/tests/fixtures/average-life.json'
MAX_COEFFICIENT = (1 << 96) - 1

# 6% semiannual, 15 June and 15 December, issued 15 June 2019, maturing 15 June 2029.
ANNUAL_SINKS = [('2025-06-15', '20'), ('2026-06-15', '20'), ('2027-06-15', '20'),
                ('2028-06-15', '20')]


def decimal_string(value):
    """The Decimal the library rounds `value` to, as a string."""
    magnitude = abs(value)
    for scale in range(28, -1, -1):
        scaled = magnitude * 10**scale
        coefficient = scaled.numerator // scaled.denominator
        twice_remainder = 2 * (scaled - coefficient)
        if twice_remainder > 1 or (twice_remainder == 1 and coefficient % 2 == 1):
            coefficient += 1
        if coefficient <= MAX_COEFFICIENT:
            digits = str(coefficient).rjust(scale + 1, '0')
            text = digits if scale == 0 else digits[:-scale] + '.' + digits[-scale:]
            return ('-' if value < 0 else '') + text
    raise ValueError('beyond a Decimal')


def average_life(face, maturity, sinks, settlement):
    face = Fraction(face)
    maturity = date.fromisoformat(maturity)
    settlement = date.fromisoformat(settlement)
    changes = []
    remaining = face
    for when, percent, _price in sinks:
        retired = face * Fraction(percent) / 100
        changes.append((date.fromisoformat(when), retired))
        remaining -= retired
    if remaining:
        changes.append((maturity, remaining))
    later = [(when, retired) for when, retired in changes if when > settlement]
    outstanding = sum(retired for _, retired in later)
    if not later or outstanding == 0:
        return None
    weighted = sum((when - settlement).days * retired for when, retired in later)
    return decimal_string(weighted / (365 * outstanding))


def case(name, settlement, sinks=(), face='100', maturity='2029-06-15', note=''):
    sinks = [(when, percent, price) for when, percent, *rest in sinks
             for price in [rest[0] if rest else '100']]
    return dict(name=name, note=note, face=face, maturity=maturity, settlement=settlement,
                sinks=[dict(date=when, percent=percent, price=price)
                       for when, percent, price in sinks],
                years=average_life(face, maturity, sinks, settlement))


def build():
    cases = [
        case('bullet', '2026-03-10',
             note='No sinks: the time to maturity, days / 365.'),
        case('bullet_large_face', '2026-03-10', face='1000000',
             note='The face does not change the life.'),
        case('annual_sinker_before_the_first_sink', '2024-01-01', ANNUAL_SINKS),
        case('annual_sinker_on_a_sink_date', '2026-06-15', ANNUAL_SINKS,
             note="The sink effective on settlement is behind it; 60 of 100 remains."),
        case('annual_sinker_after_its_last_sink', '2028-07-01', ANNUAL_SINKS,
             note='Only the residual 20 at maturity remains: the time to maturity.'),
        case('annual_sinker_above_par', '2024-01-01',
             [(when, percent, '102') for when, percent in ANNUAL_SINKS],
             note='Sinks at 102 retire the same face: the same life as at par.'),
        case('final_sink_on_maturity_above_par', '2024-01-01',
             ANNUAL_SINKS + [('2029-06-15', '10', '101')],
             note='A priced sink on maturity is split from the residual; each part counts once.'),
        case('weekend_sink', '2024-01-01',
             [('2025-11-15', '25'), ('2027-06-15', '25')],
             note='15 November 2025 is a Saturday: the change is timed at its date, not the Monday it pays.'),
        case('uneven_sinks', '2024-02-29',
             [('2024-12-15', '12.5'), ('2026-06-15', '7.25'), ('2028-12-15', '30')],
             face='250000',
             note='Uneven amounts mid-coupon and on coupon dates, settling on a leap day.'),
    ]
    refused = [
        case('at_maturity', '2029-06-15', ANNUAL_SINKS,
             note='Nothing is outstanding after the final redemption.'),
        case('after_maturity', '2029-07-01'),
        case('before_issue', '2019-06-01',
             note='Nothing is outstanding before issue.'),
    ]
    for refusal in refused:
        refusal['years'] = None
    assert all(c['years'] for c in cases)
    return dict(
        schema_version=1,
        convention='Average life in years of 365 days from settlement: the face each '
                   'principal change after settlement retires, times its calendar days '
                   'from settlement, over 365 times the face outstanding. Changes are '
                   'timed at their contractual dates and weighted by face, not cash.',
        bond='6% semiannual 30/360 US corporate, issued 2019-06-15, coupons 15 June and '
             '15 December, maturing as stated; sinks per 100 of original face.',
        rounding='Exact rational, rounded once nearest/even to the finest scale <= 28 '
                 'whose coefficient fits 96 bits.',
        provenance='Evaluated with Python fractions from the stated dates and amounts, '
                   'before the library was run against them; no library import or output.',
        cases=cases,
        refused=refused)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true',
                        help='fail if the fixture differs from what this script builds')
    args = parser.parse_args()
    text = json.dumps(build(), indent=2) + '\n'
    if args.check:
        if TARGET.read_text() != text:
            raise SystemExit(f'{TARGET} differs from the reference')
        return
    TARGET.write_text(text)


if __name__ == '__main__':
    main()
