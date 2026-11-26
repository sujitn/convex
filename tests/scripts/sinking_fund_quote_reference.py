#!/usr/bin/env python3
"""Independent quotes of a mandatory sinking-fund bond, per current and original face.

One stated bond: 6% semiannual 30/360 US, dated and issued 15 March 2020, coupons 15 March
and 15 September, unadjusted, maturing 15 March 2032 at par, with five mandatory sinks per
100 of original face, one above par and one between coupon dates. The rest is redeemed at
maturity.

For each settlement, from the stated terms alone:
- the factor is the face outstanding after any sink on settlement over the original face;
- a clean price per 100 current face times the factor is the price per 100 original face;
- each coupon is the rate times the face outstanding on each part of its period, cut at
  any sink inside it, times that part's 30/360 days over 360; accrued is the same sum from
  the period's start to settlement;
- a holder settling on a date is owed what pays after it; a sink pays the face it retires
  times its price over 100;
- the street yield discounts each payment at (1 + y/2)^(-2t), t in 30/360 years from
  settlement, and is solved by bisection to the clean price per original face;
- the average life is the face each later sink or redemption retires times its calendar
  days from settlement, over 365 times the face outstanding.
Entitled cash between two settlement dates is what a holder settling on the first is owed
and one settling on the second is not, per 100 original face.

Evaluated with Python Decimal at 60 digits before the server was run against it; no
library import or output.
"""
import argparse
from datetime import date
from decimal import Decimal as D, getcontext
import json
from pathlib import Path

getcontext().prec = 60
ROOT = Path(__file__).resolve().parents[2]
TARGET = ROOT / 'crates/convex-engine/tests/fixtures/sinking-fund-quotes.json'

FACE = D('100')
RATE = D('0.06')
DATED = date(2020, 3, 15)
MATURITY = date(2032, 3, 15)
REDEMPTION = D('100')
# Date, face retired per 100 of original face, price per 100 of the face retired.
SINKS = [('2025-03-15', '10', '100'), ('2026-03-15', '10', '100'),
         ('2027-03-15', '10', '101'), ('2027-11-13', '10', '100'),
         ('2028-03-15', '20', '100')]
# Settlement, clean price per 100 current face. 2026-10-06 is the Live runtime's.
QUOTES = [('2024-06-10', '98.25'), ('2026-03-14', '99.5'), ('2026-03-15', '99.5'),
          ('2026-10-06', '99.5'), ('2027-08-20', '100.75'), ('2027-12-01', '100.1')]
# From, to.
ENTITLED = [('2026-03-01', '2026-03-20'), ('2027-08-20', '2027-12-01'),
            ('2024-06-10', '2024-06-10')]


def d(text):
    return date.fromisoformat(text)


SINKS_PARSED = [(d(day), D(amount), D(price)) for day, amount, price in SINKS]
LAST_SINK = SINKS_PARSED[-1][0]


def days_360(a, b):
    """30/360 US (bond basis) days; no date here falls on a month's end."""
    d1, d2 = min(a.day, 30), b.day
    if d2 == 31 and d1 == 30:
        d2 = 30
    return 360 * (b.year - a.year) + 30 * (b.month - a.month) + (d2 - d1)


def outstanding_after(day):
    """Face outstanding just after `day`, sinks on it retired."""
    retired = sum((amount for when, amount, _ in SINKS_PARSED if when <= day), D(0))
    return FACE * (100 - retired) / 100 if day < MATURITY else D(0)


def coupon_dates():
    dates, year, month = [DATED], DATED.year, DATED.month
    while dates[-1] < MATURITY:
        month += 6
        if month > 12:
            month, year = month - 12, year + 1
        dates.append(date(year, month, 15))
    return dates


def accrual(start, end):
    """Coupon accrued from `start` to `end` inside one period, cut at each sink inside."""
    cuts = sorted({start, end} | {when for when, _, _ in SINKS_PARSED if start < when < end})
    return sum((outstanding_after(a) * RATE * days_360(a, b) / 360
                for a, b in zip(cuts, cuts[1:])), D(0))


def payments():
    """Every coupon and principal payment: (date, amount)."""
    dates = coupon_dates()
    flows = [(b, accrual(a, b)) for a, b in zip(dates, dates[1:])]
    flows += [(when, FACE * amount / 100 * price / 100) for when, amount, price in SINKS_PARSED]
    flows.append((MATURITY, outstanding_after(LAST_SINK) * REDEMPTION / 100))
    return sorted(flows)


def owed(settlement):
    return [(day, amount) for day, amount in payments() if day > settlement]


def period_of(settlement):
    dates = coupon_dates()
    return next((a, b) for a, b in zip(dates, dates[1:]) if a <= settlement < b)


def years_360(settlement, day):
    start, _ = period_of(settlement)
    return D(days_360(start, day) - days_360(start, settlement)) / 360


def dirty(settlement, y):
    return sum((amount * 100 / FACE * (1 + y / 2) ** (-2 * years_360(settlement, day))
                for day, amount in owed(settlement)), D(0))


def street_yield(settlement, clean, accrued):
    low, high = D('-0.05'), D('0.5')
    for _ in range(200):
        mid = (low + high) / 2
        if dirty(settlement, mid) - accrued > clean:
            low = mid
        else:
            high = mid
    return (low + high) / 2


def average_life(settlement):
    changes = [(when, FACE * amount / 100) for when, amount, _ in SINKS_PARSED if when > settlement]
    changes.append((MATURITY, outstanding_after(LAST_SINK)))
    weighted = sum((face * (day - settlement).days for day, face in changes), D(0))
    return weighted / (365 * outstanding_after(settlement))


def text(value, places):
    return format(value.quantize(D(1).scaleb(-places)), 'f')


def build():
    cases = []
    for day, price in QUOTES:
        settlement = d(day)
        factor = outstanding_after(settlement) / FACE
        clean = D(price) * factor
        start, _ = period_of(settlement)
        accrued = accrual(start, settlement) * 100 / FACE
        cases.append(dict(
            settlement=day,
            clean_price_per_current_face=price,
            factor=text(factor, 20),
            clean_price_mid=text(clean, 20),
            accrued_interest=text(accrued, 20),
            ytm_mid=text(street_yield(settlement, clean, accrued), 20),
            average_life_years=text(average_life(settlement), 20)))
    entitled = [{'from': a, 'to': b,
                 'cash_per_100': text(sum((amount for day, amount in payments()
                                           if d(a) < day <= d(b)), D(0)) * 100 / FACE, 20)}
                for a, b in ENTITLED]
    return dict(
        schema_version=1,
        bond=dict(coupon_rate=str(RATE), frequency=2, day_count='30/360 US',
                  business_day_convention='Unadjusted', dated_date=DATED.isoformat(),
                  issue_date=DATED.isoformat(), maturity_date=MATURITY.isoformat(), face_value=str(FACE),
                  redemption_value=str(REDEMPTION),
                  sinking_schedule=[dict(date=day, amount=amount, price=price)
                                    for day, amount, price in SINKS]),
        provenance='Evaluated with Python Decimal at 60 digits from the stated terms, '
                   'before the server was run against them; no library import or output.',
        quotes=cases,
        entitled_cash=entitled)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true',
                        help='fail if the fixture differs from what this script builds')
    args = parser.parse_args()
    content = json.dumps(build(), indent=2) + '\n'
    if args.check:
        if TARGET.read_text() != content:
            raise SystemExit(f'{TARGET} differs from the reference')
        return
    TARGET.parent.mkdir(parents=True, exist_ok=True)
    TARGET.write_text(content)


if __name__ == '__main__':
    main()
