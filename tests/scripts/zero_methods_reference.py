#!/usr/bin/env python3
"""Independent Decimal80 zero-coupon cash/rate controls; no Convex imports."""
import datetime
import json
from decimal import Decimal, localcontext
from fractions import Fraction
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[2]
TARGET = ROOT / "tests/fixtures/library/zero-methods-reference.json"
PERIODS = {"Annual": 1, "SemiAnnual": 2, "Quarterly": 4, "Monthly": 12, "Continuous": None}


def text(value):
    return format(value, "f")


def year_fraction(start, end, convention):
    start = datetime.date.fromisoformat(start)
    end = datetime.date.fromisoformat(end)
    if convention in ("Act360", "Act365Fixed"):
        return Fraction((end - start).days, 360 if convention == "Act360" else 365)
    if convention == "ActActIsda":
        total = Fraction(0)
        cursor = start
        while cursor < end:
            boundary = min(end, datetime.date(cursor.year + 1, 1, 1))
            basis = (datetime.date(cursor.year + 1, 1, 1) - datetime.date(cursor.year, 1, 1)).days
            total += Fraction((boundary - cursor).days, basis)
            cursor = boundary
        return total
    first = min(start.day, 30)
    last = min(end.day, 30) if convention == "Thirty360E" or first == 30 else end.day
    return Fraction(360 * (end.year - start.year) + 30 * (end.month - start.month) + last - first, 360)


def continuous(rate, convention):
    periods = PERIODS[convention]
    if periods is None:
        return rate
    periods = Decimal(periods)
    return periods * (1 + rate / periods).ln()


def quoted(rate, convention):
    periods = PERIODS[convention]
    if periods is None:
        return rate
    periods = Decimal(periods)
    return periods * ((rate / periods).exp() - 1)


def document():
    dates = [
        ("2026-01-01", "2026-01-02", "Act360"),
        ("2026-01-01", "2027-01-01", "Act365Fixed"),
        ("2024-07-01", "2026-07-01", "ActActIsda"),
        ("2025-01-15", "2026-08-21", "Thirty360US"),
        ("2025-01-31", "2026-02-28", "Thirty360E"),
    ]
    cash = [("100", "103.25"), ("1000", "987.5"), ("0.001", "0.0009")]
    prices, inverses, conversions, poles = [], [], [], []
    with localcontext() as ctx:
        ctx.prec = 80
        for settlement, maturity, day_count in dates:
            fraction = year_fraction(settlement, maturity, day_count)
            years = Decimal(fraction.numerator) / Decimal(fraction.denominator)
            for compounding in PERIODS:
                for face, redemption in cash:
                    terms = dict(issue="2020-01-01", settlement=settlement, maturity=maturity,
                                 day_count=day_count, compounding=compounding, face=face,
                                 redemption=redemption, year_fraction=text(years))
                    for rate in ("-0.25", "0", "0.05", "1e-19"):
                        value = Decimal(rate)
                        price = Decimal(redemption) * (-continuous(value, compounding) * years).exp()
                        prices.append(dict(**terms, rate=text(value), expected=text(price)))
                    quotes = [Decimal(redemption) * Decimal(m) for m in ("0.97", "1", "1.02")]
                    quotes += [Decimal(redemption) - Decimal("1e-22"), Decimal(redemption) + Decimal("1e-22")]
                    for price in quotes:
                        log_rate = (Decimal(redemption) / price).ln() / years
                        inverses.append(dict(**terms, price=text(price), expected=text(quoted(log_rate, compounding)),
                                             expected_continuous=text(log_rate)))
        for source in PERIODS:
            for target in PERIODS:
                for rate in ("-0.25", "0", "0.05", "1e-24", "0.4"):
                    value = Decimal(rate)
                    equivalent = value if source == target else quoted(continuous(value, source), target)
                    conversions.append(dict(source=source, target=target, rate=text(value), expected=text(equivalent)))
        for compounding, periods in PERIODS.items():
            if periods is None:
                continue
            for offset in ("-1e-26", "0", "1e-26"):
                rate = -Decimal(periods) + Decimal(offset)
                admitted = offset == "1e-26"
                log_rate = continuous(rate, compounding) if admitted else None
                poles.append(dict(compounding=compounding, rate=text(rate), admissible=admitted,
                                  expected_price=text(Decimal(100) * (-log_rate / Decimal(360)).exp()) if admitted else None,
                                  expected_continuous=text(log_rate) if admitted else None))
    return dict(schema_version=1, provenance="Independent Python Decimal80 equations and calendar fractions declared before candidate production edits; native values are never references.",
                primary_formula_source="https://github.com/lballabio/QuantLib/blob/master/ql/interestrate.cpp",
                units="Object prices are absolute cash in the original face/redemption units; all rates are decimal fractions.",
                bounds=dict(absolute="1e-12", relative="5e-13", preserve_nonzero_sign=True),
                price_cases=prices, inverse_cases=inverses, conversion_cases=conversions, pole_cases=poles,
                invalid_inputs=["nonpositive face/redemption/price", "settlement before issue or on/after maturity", "issue not before maturity", "unsupported context-dependent day count", "overlong horizon", "nonpositive periodic base", "Decimal output overflow or nonzero underflow"],
                certified=False)


def main():
    data = (json.dumps(document(), indent=2) + "\n").encode()
    mode = sys.argv[1] if len(sys.argv) == 2 else "check"
    if mode == "write":
        with TARGET.open("xb") as stream:
            stream.write(data)
    elif mode == "check":
        if TARGET.read_bytes() != data:
            raise ValueError("Independent reference declaration changed")
    else:
        raise ValueError("Use write or check")
    print(json.dumps(dict(status="passed", mode=mode, price_cases=300, inverse_cases=375,
                          conversion_cases=125, pole_cases=12, certified=False)))


if __name__ == "__main__":
    main()
