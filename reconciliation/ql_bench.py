"""
QuantLib side of the Convex reconciliation bench.

Mirror of tools/reconcile_bench/src/main.rs: reads book.json and curves.json,
prices every vanilla fixed-rate bullet bond with QuantLib, writes ql.csv with
the same schema so reconcile.py can diff row-for-row.

Scope (Milestone 2 MVP):
  * Fixed-rate bullet bonds only.
  * UST CMT curve is used as the discount / reference-yield source for USD
    bonds. Non-USD bonds use the coupon rate as the reference yield (curve
    placeholders only) so the run still produces a number; reconciliation
    remains valid because both sides use the SAME reference yield input.

Run from repo root:
    python reconciliation/ql_bench.py

Output: reconciliation/ql.csv
"""
from __future__ import annotations

import csv
import json
import pathlib
import sys
from datetime import date

import QuantLib as ql

HERE = pathlib.Path(__file__).parent


# ---------------------------------------------------------------- convention mapping

FREQUENCY_MAP = {
    "annual": ql.Annual,
    "semi_annual": ql.Semiannual,
    "semi-annual": ql.Semiannual,
    "quarterly": ql.Quarterly,
    "monthly": ql.Monthly,
}

# Bond frequency → matching compounding frequency for the yield quote.
COMPOUNDING_FREQUENCY = {
    ql.Annual: ql.Annual,
    ql.Semiannual: ql.Semiannual,
    ql.Quarterly: ql.Quarterly,
    ql.Monthly: ql.Monthly,
}


def day_count(name: str) -> ql.DayCounter:
    name = name.strip().upper()
    if name in ("ACT/ACT ICMA", "ACT/ACT", "ACT/ACT ISMA"):
        return ql.ActualActual(ql.ActualActual.ISMA)
    if name == "ACT/ACT ISDA":
        return ql.ActualActual(ql.ActualActual.ISDA)
    if name in ("30/360 US", "30/360"):
        return ql.Thirty360(ql.Thirty360.USA)
    if name in ("30E/360", "30/360 E"):
        return ql.Thirty360(ql.Thirty360.European)
    if name == "ACT/360":
        return ql.Actual360()
    if name in ("ACT/365F", "ACT/365", "ACT/365 FIXED"):
        return ql.Actual365Fixed()
    raise ValueError(f"unknown day count {name!r}")


def to_ql_date(s: str) -> ql.Date:
    y, m, d = map(int, s.split("-"))
    return ql.Date(d, m, y)


# ---------------------------------------------------------------- curve utilities

def interpolate_cmt(cmt: dict, tenor_yrs: float) -> float | None:
    """Linear interp on the UST_CMT quotes, matching the Rust side."""
    pts = sorted(
        (q["tenor_years"], q["rate_pct"])
        for q in cmt["quotes"]
        if q.get("rate_pct") is not None
    )
    if not pts:
        return None
    if tenor_yrs <= pts[0][0]:
        return pts[0][1] / 100.0
    for (t0, r0), (t1, r1) in zip(pts, pts[1:]):
        if t0 <= tenor_yrs <= t1:
            w = (tenor_yrs - t0) / (t1 - t0)
            return (r0 + w * (r1 - r0)) / 100.0
    return pts[-1][1] / 100.0


def years_to_maturity(valuation: ql.Date, maturity: ql.Date) -> float:
    return (maturity - valuation) / 365.25


# ---------------------------------------------------------------- price/risk helpers

def build_bullet(
    inst: dict,
    workout_date: ql.Date,
    redemption: float = 100.0,
) -> tuple[ql.FixedRateBond, ql.DayCounter, int]:
    """Build a hypothetical bullet of `inst` redeeming at `workout_date`.
    Returns (bond, day_counter, frequency_enum)."""
    issue = to_ql_date(inst.get("dated_date") or inst["issue_date"])
    freq = FREQUENCY_MAP[inst["frequency"].lower()]
    dcc = day_count(inst["day_count"])
    coupon = effective_coupon_pct(inst) / 100.0

    schedule = ql.Schedule(
        issue,
        workout_date,
        ql.Period(freq),
        ql.NullCalendar(),
        ql.Unadjusted,
        ql.Unadjusted,
        ql.DateGeneration.Backward,
        False,
    )
    bond = ql.FixedRateBond(
        0,
        100.0,
        schedule,
        [coupon],
        dcc,
        ql.Following,
        redemption,  # non-par redemption for workout-date bullets
    )
    return bond, dcc, freq


def price_bond(
    inst: dict,
    valuation: ql.Date,
    ref_yield: float,
) -> dict[str, float]:
    """Return {metric_name: value} for one fixed-rate bullet bond."""
    maturity = to_ql_date(inst["maturity_date"])
    bond, dcc, freq = build_bullet(inst, maturity, 100.0)

    comp = ql.Compounded
    cmp_freq = COMPOUNDING_FREQUENCY[freq]

    # Prices at the reference yield.
    # QL's dirtyPrice(yield, ...) overload doesn't exist; derive as clean + accrued.
    clean = ql.BondFunctions.cleanPrice(bond, ref_yield, dcc, comp, cmp_freq, valuation)
    accrued = ql.BondFunctions.accruedAmount(bond, valuation)
    dirty = clean + accrued

    # YTM round-trip from the clean price (should recover ref_yield).
    # Newer QL needs a BondPrice wrapper (clean price type = 1).
    bond_price = ql.BondPrice(clean, ql.BondPrice.Clean)
    ytm = ql.BondFunctions.bondYield(
        bond, bond_price, dcc, comp, cmp_freq, valuation
    )

    # Risk at the reference yield.
    interest_rate = ql.InterestRate(ref_yield, dcc, comp, cmp_freq)
    mac_dur = ql.BondFunctions.duration(
        bond, interest_rate, ql.Duration.Macaulay, valuation
    )
    mod_dur = ql.BondFunctions.duration(
        bond, interest_rate, ql.Duration.Modified, valuation
    )
    cvx = ql.BondFunctions.convexity(bond, interest_rate, valuation)
    # DV01 per 100 face = mod_dur * dirty_price * 0.0001, matching Convex.
    dv01 = mod_dur * dirty * 1e-4

    return {
        "clean_price_pct": clean,
        "dirty_price_pct": dirty,
        "accrued": accrued,
        "ytm_decimal": ytm,
        "macaulay_duration": mac_dur,
        "modified_duration": mod_dur,
        "convexity": cvx,
        "dv01_per_100": dv01,
    }


# ---------------------------------------------------------------- main

SKIP_CATEGORIES: dict[str, str] = {}  # all categories now handled

CALLABLE_CATEGORIES = {"corporate_callable", "synthetic_callable"}


def effective_coupon_pct(inst: dict) -> float:
    """For an FRN, project future coupons at a flat (index + spread)."""
    if inst["category"] == "sovereign_frn":
        idx = inst.get("index_rate_pct")
        if idx is None:
            raise ValueError(f"{inst['id']}: FRN missing index_rate_pct")
        spread = inst.get("spread_bps", 0.0)
        return idx + spread / 100.0
    return inst["coupon_rate"]


def main() -> int:
    book = json.loads((HERE / "book.json").read_text())
    curves = json.loads((HERE / "curves.json").read_text())
    cmt = next(c for c in curves["curves"] if c["id"] == "UST_CMT")

    valuation = to_ql_date(book["valuation_date"])
    ql.Settings.instance().evaluationDate = valuation

    rows: list[dict] = []
    skipped: list[str] = []

    for inst in book["instruments"]:
        cat = inst["category"]
        if cat in SKIP_CATEGORIES:
            skipped.append(f"{inst['id']} ({cat}) — {SKIP_CATEGORIES[cat]}")
            continue
        is_callable = cat in CALLABLE_CATEGORIES
        is_linker = cat == "sovereign_linker"
        is_frn = cat == "sovereign_frn"
        known = {"sovereign", "corporate_bullet_mw", "sovereign_linker", "sovereign_frn"}
        if cat not in known and not is_callable:
            skipped.append(f"{inst['id']} ({cat}) — unknown category")
            continue

        ccy = inst.get("currency", "?")
        maturity = to_ql_date(inst["maturity_date"])
        yrs = years_to_maturity(valuation, maturity)

        if is_linker:
            # TIPS priced on real yield. Placeholder: 1.85% (10Y TIPS real
            # yield from the 2025-11-20 reopening auction).
            y = 0.0185
            curve_used = "tips_real_placeholder"
        elif is_frn:
            # Reconcile the FRN as a flat-forward proxy: discount at the same
            # projected coupon (index + spread). That makes the bond price at
            # par on the first-coupon anniversary and tests the quarterly
            # ACT/360 convention path on both libraries.
            y = effective_coupon_pct(inst) / 100.0
            curve_used = "frn_flat_projection"
        elif ccy == "USD":
            y = interpolate_cmt(cmt, yrs)
            curve_used = "UST_CMT"
            if y is None:
                y = inst["coupon_rate"] / 100.0
                curve_used = "placeholder"
        else:
            y = inst["coupon_rate"] / 100.0
            curve_used = "placeholder"

        # Base metrics (treating the bond as if calls never happen).
        metrics = price_bond(inst, valuation, y)
        emitted: list[tuple[str, float]] = list(metrics.items())

        # Callable add-ons: YTC per call date + YTW + workout date.
        if is_callable:
            clean = metrics["clean_price_pct"]
            ytm = metrics["ytm_decimal"]
            bond_price = ql.BondPrice(clean, ql.BondPrice.Clean)
            worst_yield = ytm
            worst_date = maturity
            for entry in inst.get("call_schedule") or []:
                call_date = to_ql_date(entry["call_date"])
                if call_date <= valuation:
                    continue
                wb, wb_dcc, wb_freq = build_bullet(
                    inst, call_date, entry["price"]
                )
                comp = ql.Compounded
                cmp_freq = COMPOUNDING_FREQUENCY[wb_freq]
                ytc = ql.BondFunctions.bondYield(
                    wb, bond_price, wb_dcc, comp, cmp_freq, valuation
                )
                key = f"ytc_{entry['call_date'].replace('-', '')}_decimal"
                emitted.append((key, ytc))
                if ytc < worst_yield:
                    worst_yield = ytc
                    worst_date = call_date
            emitted.append(("ytw_decimal", worst_yield))
            wd = worst_date
            yyyymmdd = wd.year() * 10000 + wd.month() * 100 + wd.dayOfMonth()
            emitted.append(("ytw_workout_date_yyyymmdd", float(yyyymmdd)))

        for metric, value in emitted:
            rows.append(
                {
                    "bond_id": inst["id"],
                    "currency": ccy,
                    "metric": metric,
                    "value": f"{value:.10f}",
                    "reference_yield": f"{y:.10f}",
                    "curve_used": curve_used,
                    "notes": "",
                }
            )

    out = HERE / "ql.csv"
    with out.open("w", newline="") as fh:
        w = csv.DictWriter(
            fh,
            fieldnames=[
                "bond_id",
                "currency",
                "metric",
                "value",
                "reference_yield",
                "curve_used",
                "notes",
            ],
        )
        w.writeheader()
        w.writerows(rows)

    print(f"ql_bench: wrote {out} — {len(rows) // 8} bonds priced", file=sys.stderr)
    if skipped:
        print("ql_bench: skipped:", file=sys.stderr)
        for s in skipped:
            print(f"  - {s}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
