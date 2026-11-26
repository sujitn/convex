#!/usr/bin/env python3
"""Independent reference for callable perpetual quotes.

Writes crates/convex-analytics/tests/fixtures/perpetual-quotes.json. Python Decimal at 60
digits; it imports nothing from the library. `--check` fails when the file on disk differs
from what this script writes.

Two 30/360 US semi-annual perpetuals, both callable at par on their first call and every
coupon date after it, with 30 calendar days' notice:
- `fixed`: 6.5% for ever;
- `reset`: 7% to its first reset on the first call, then every five years the 5-year swap
  fixing two business days before the reset plus a 4.12% margin.

Payment dates roll to the following weekday. Each quote is a dated yield under 30/360 US
compounded semi-annually: a payment `t` years after settlement is discounted by
(1 + y/2)^(-2t). For each settlement it gives
- the accrued per 100 face and the eligible call dates up to the horizon;
- the yield of each workout from a clean price (each call, and the perpetuity), and the
  worst of them;
- the price of each workout at a yield, and the worst (lowest) of them. A yield of zero or
  below has no perpetuity.

The perpetuity pays every coupon to its tail start on its payment date, then the coupon
in force for ever, each on its unadjusted date. The tail is summed term by term, each
term's time read by the day count, until a term falls below 1e-45 of the price; no
closed form is used.
"""

import argparse
import datetime as dt
import json
import pathlib
import sys
from collections import namedtuple
from decimal import Decimal as D, getcontext

getcontext().prec = 60

FIXTURE = (
    pathlib.Path(__file__).resolve().parents[2]
    / "crates/convex-analytics/tests/fixtures/perpetual-quotes.json"
)

FACE = D("1000000")
FREQUENCY = 2
ACCRUAL_START = dt.date(2025, 1, 15)
FIRST_CALL = dt.date(2030, 1, 15)
MARGIN = D("0.0412")
BENCHMARK_ID = "USD.SWAP.5Y"
TAIL_PRECISION = D("1e-45")
YIELD_PRECISION = D("1e-30")

# One coupon period: its accrual dates, payment date, cash, rate, and whether that rate
# holds the benchmark's current level.
Period = namedtuple("Period", "start end paid amount rate is_held")


def add_months(date, months):
    month = date.month - 1 + months
    return dt.date(date.year + month // 12, month % 12 + 1, date.day)


def is_business_day(date):
    return date.weekday() < 5


def following(date):
    while not is_business_day(date):
        date += dt.timedelta(days=1)
    return date


def business_days_before(date, count):
    while count > 0:
        date -= dt.timedelta(days=1)
        if is_business_day(date):
            count -= 1
    return date


def thirty_360_us(start, end):
    """30/360 US on (year, month, day) tuples, any year. Every date here is mid-month."""
    (y1, m1, d1), (y2, m2, d2) = start, end
    assert d1 <= 30 and d2 <= 30, (start, end)
    return D(360 * (y2 - y1) + 30 * (m2 - m1) + (d2 - d1)) / D(360)


def ymd(date):
    return (date.year, date.month, date.day)


def coupon_dates(through):
    dates = []
    date = ACCRUAL_START
    while date <= through:
        dates.append(date)
        date = add_months(date, 12 // FREQUENCY)
    return dates


class Bond:
    def __init__(self, name, fixed_rate, reset):
        self.name = name
        self.fixed_rate = fixed_rate
        self.reset = reset

    def reset_dates(self, through):
        if not self.reset:
            return []
        dates, date = [], self.reset["first_reset"]
        while date <= through:
            dates.append(date)
            date = add_months(date, self.reset["interval_months"])
        return dates

    def rate_from(self, start, fixings, held):
        """Coupon rate for the period starting on `start`, and whether it is held."""
        resets = [r for r in self.reset_dates(start) if r <= start]
        if not resets:
            return self.fixed_rate, False
        observation = business_days_before(resets[-1], 2)
        if observation in fixings:
            return fixings[observation] + MARGIN, False
        return held + MARGIN, True

    def terms(self):
        redemption = {
            "kind": "perpetual",
            "call": {
                "interval_months": 6,
                "price_per_100": "100",
                "notice_offset": {"CalendarDays": -30},
                "settlement_lag": {"CalendarDays": 0},
                "settlement_adjustment": "Following",
                "accrued": "pay_accrued",
            },
        }
        if self.reset:
            redemption["reset"] = {
                "benchmark_id": BENCHMARK_ID,
                "margin": str(MARGIN),
                "first_reset": self.reset["first_reset"].isoformat(),
                "interval_months": self.reset["interval_months"],
                "fixing_offset": {"BusinessDays": -2},
            }
        start, end = ACCRUAL_START.isoformat(), FIRST_CALL.isoformat()
        return {
            "schema_version": 2,
            "currency": "USD",
            "issue_date": start,
            "face_value": str(FACE),
            "schedule": {
                "start_date": start,
                "end_date": end,
                "frequency": "SemiAnnual",
                "stub_type": "None",
                "end_of_month": False,
                "first_regular_date": None,
                "penultimate_date": None,
            },
            "dates": {
                "accrual_adjustment": "Unadjusted",
                "payment_adjustment": "Following",
                "payment_delay": {"CalendarDays": 0},
                "fixing_offset": None,
                "observation_offset": None,
                "ex_coupon": None,
            },
            "calendars": {
                "payment": "WEEKEND",
                "reset": "WEEKEND",
                "observation": "WEEKEND",
                "exercise": "WEEKEND",
            },
            "settlement_lag": {"BusinessDays": 2},
            "settlement_adjustment": "Following",
            "day_count": "Thirty360US",
            "profiles": {
                "coupon": [{"start": start, "end": end, "value": str(self.fixed_rate)}],
                "spread": [{"start": start, "end": end, "value": "0"}],
                "gearing": [{"start": start, "end": end, "value": "0"}],
                "notional": [{"start": start, "end": end, "value": str(FACE)}],
            },
            "coupon": {"kind": "fixed"},
            "coupon_principal": "AccrualProfile",
            "redemption_multiplier": "1",
            "principal_prices": [],
            "rounding": "Unrounded",
            "negative_coupons": "Allow",
            "ex_coupon_accrual": "SubtractCoupon",
            "exercise": [],
            "redemption": redemption,
        }


def discount(y, t):
    """(1 + y/2)^(-2t) for any real t."""
    return ((D(1) + y / FREQUENCY).ln() * (-FREQUENCY * t)).exp()


class Case:
    def __init__(self, bond, trade, settlement, horizon, fixings, held):
        self.bond = bond
        self.trade = trade
        self.settlement = settlement
        self.horizon = horizon
        self.fixings = fixings
        self.held = held
        # Coupon periods far enough to reach every call and the tail start.
        far = max(horizon, FIRST_CALL) + dt.timedelta(days=366 * 11)
        dates = coupon_dates(far)
        self.periods = []
        for start, end in zip(dates, dates[1:]):
            rate, held_rate = bond.rate_from(start, fixings, held)
            amount = FACE * rate * thirty_360_us(ymd(start), ymd(end))
            self.periods.append(Period(start, end, following(end), amount, rate, held_rate))
        current = next(p for p in self.periods if p.start <= settlement < p.end)
        self.accrued = FACE * current.rate * thirty_360_us(ymd(current.start), ymd(settlement))
        # The tail starts at the first coupon date on or after the first call, at or after
        # the end of settlement's period, from which every coupon is the held one.
        self.tail_start = next(
            p.start
            for p in self.periods
            if p.start >= FIRST_CALL
            and p.start >= current.end
            and (bond.reset is None or p.is_held)
        )
        if bond.reset is None:
            self.tail_rate = bond.fixed_rate
        else:
            self.tail_rate = held + MARGIN
        self.calls = []
        call = FIRST_CALL
        while call <= horizon:
            if call > settlement and call - dt.timedelta(days=30) >= trade:
                self.calls.append(call)
            call = add_months(call, 6)

    def future_coupons(self, through):
        return [
            (p.paid, p.amount)
            for p in self.periods
            if self.settlement < p.end <= through
        ]

    def time(self, date):
        return self.time_tuple(ymd(date))

    def time_tuple(self, end):
        return thirty_360_us(ymd(self.settlement), end)

    def dirty(self, workout, y):
        if workout == "perpetuity":
            cash = self.future_coupons(self.tail_start)
            pv = sum(amount * discount(y, self.time(d)) for d, amount in cash)
            coupon = FACE * self.tail_rate / FREQUENCY
            t_start = self.time(self.tail_start)
            step = D(1) / (D(1) + y / FREQUENCY)
            factor = discount(y, t_start)
            y0, m0, d0 = ymd(self.tail_start)
            k = 0
            while True:
                k += 1
                months = m0 - 1 + 6 * k
                end = (y0 + months // 12, months % 12 + 1, d0)
                # Each term's time is read by the day count: k regular periods past the
                # tail start, so its discount is the previous one's times one period's.
                assert (self.time_tuple(end) - t_start) * FREQUENCY == k
                factor *= step
                term = coupon * factor
                pv += term
                if term < TAIL_PRECISION * pv:
                    break
            return pv / FACE * 100
        cash = self.future_coupons(workout)
        cash.append((following(workout), FACE))
        pv = sum(amount * discount(y, self.time(d)) for d, amount in cash)
        return pv / FACE * 100

    def accrued_per_100(self):
        return self.accrued / FACE * 100

    def solve(self, workout, clean):
        target = clean + self.accrued_per_100()
        low, high = (D("0.01"), D("0.5")) if workout == "perpetuity" else (D("-0.5"), D(2))
        assert self.dirty(workout, low) > target > self.dirty(workout, high)
        while high - low > YIELD_PRECISION:
            mid = (low + high) / 2
            if self.dirty(workout, mid) > target:
                low = mid
            else:
                high = mid
        return (low + high) / 2

    def workouts(self):
        return [*self.calls, "perpetuity"]


def label(workout):
    return workout if workout == "perpetuity" else workout.isoformat()


def number(value):
    """24 decimal places."""
    return str(value.quantize(D("1e-24")))


def case_record(case, clean_prices, yields):
    record = {
        "trade_date": case.trade.isoformat(),
        "settlement": case.settlement.isoformat(),
        "horizon": case.horizon.isoformat(),
        "held_benchmark": None if case.held is None else str(case.held),
        "fixings": [
            {"observation_date": d.isoformat(), "rate": str(r)}
            for d, r in sorted(case.fixings.items())
        ],
        "accrued_per100": number(case.accrued_per_100()),
        "calls": [c.isoformat() for c in case.calls],
        "tail_start": case.tail_start.isoformat(),
        "tail_rate": str(case.tail_rate),
        "yield_to_worst": [],
        "price_to_worst": [],
    }
    for clean in clean_prices:
        yields_by = [(w, case.solve(w, clean)) for w in case.workouts()]
        worst = min(yields_by, key=lambda p: p[1])[0]
        record["yield_to_worst"].append(
            {
                "clean_price": str(clean),
                "yields": [{"workout": label(w), "yield": number(y)} for w, y in yields_by],
                "worst": label(worst),
            }
        )
    for y in yields:
        workouts = [w for w in case.workouts() if w != "perpetuity" or y > 0]
        prices = [(w, case.dirty(w, y)) for w in workouts]
        worst = min(prices, key=lambda p: p[1])[0]
        record["price_to_worst"].append(
            {
                "yield": str(y),
                "prices": [
                    {
                        "workout": label(w),
                        "dirty": number(p),
                        "clean": number(p - case.accrued_per_100()),
                    }
                    for w, p in prices
                ],
                "worst": label(worst),
            }
        )
    return record


def build():
    fixed = Bond("fixed", D("0.065"), None)
    reset = Bond(
        "reset",
        D("0.07"),
        {"first_reset": FIRST_CALL, "interval_months": 60},
    )
    held = D("0.0385")
    first_fixing = {business_days_before(FIRST_CALL, 2): D("0.0362")}
    bonds = []
    for bond, cases in [
        (
            fixed,
            [
                # Before the first call: a premium's worst is the first call, a
                # discount's the perpetuity.
                (
                    Case(fixed, dt.date(2026, 3, 6), dt.date(2026, 3, 10),
                         dt.date(2031, 1, 15), {}, None),
                    [D("104.25"), D("92.5")],
                    [D("0.055"), D("0.08"), D("-0.002")],
                ),
                # Past the first call and between calls.
                (
                    Case(fixed, dt.date(2031, 2, 18), dt.date(2031, 2, 20),
                         dt.date(2033, 1, 15), {}, None),
                    [D("101.125"), D("97")],
                    [D("0.06"), D("0.01")],
                ),
            ],
        ),
        (
            reset,
            [
                # Before the first reset: every reset is held at the current level.
                (
                    Case(reset, dt.date(2026, 3, 6), dt.date(2026, 3, 10),
                         dt.date(2031, 1, 15), {}, held),
                    [D("103.5"), D("95")],
                    [D("0.07"), D("0.01")],
                ),
                # After the first reset: its fixing is set; the next one is held.
                (
                    Case(reset, dt.date(2031, 2, 18), dt.date(2031, 2, 20),
                         dt.date(2033, 1, 15), first_fixing, held),
                    [D("100.75"), D("94")],
                    [D("0.075"), D("0.01")],
                ),
            ],
        ),
    ]:
        bonds.append(
            {
                "name": bond.name,
                "terms": bond.terms(),
                "cases": [case_record(c, p, y) for c, p, y in cases],
            }
        )
    return {
        "generator": "tests/scripts/perpetual_quote_reference.py",
        "convention": {"day_count": "Thirty360US", "compounding": "SemiAnnual"},
        "bonds": bonds,
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    text = json.dumps(build(), indent=2) + "\n"
    if args.check:
        if FIXTURE.read_text() != text:
            print(f"{FIXTURE} differs from the reference", file=sys.stderr)
            return 1
        return 0
    FIXTURE.write_text(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
