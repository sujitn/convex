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
import math
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

def interpolate_curve(curve: dict, tenor_yrs: float) -> float | None:
    """Linear interp on a discount-curve's `quotes`, matching the Rust side.

    Returns a decimal yield (0.04 = 4%) or None if the curve has no quotes.
    """
    pts = sorted(
        (q["tenor_years"], q["rate_pct"])
        for q in curve.get("quotes", [])
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


CCY_TO_CURVE_ID = {
    "USD": "UST_CMT",
    "GBP": "UK_GILT_CURVE",
    "EUR": "DE_BUND_CURVE",
    "JPY": "JP_JGB_CURVE",
}


# ---------------------------------------------------------------- SOFR FRN pricing

def build_zero_rate_df(curve: dict, valuation: ql.Date) -> callable:
    """Return a DF(date) closure for a zero-rate panel (cont. ACT/365F).

    Linear interpolation on zero rates, flat extrapolation. Matches the Convex
    side (`DiscountCurveBuilder::with_interpolation(Linear)`) point-for-point.
    """
    pts = sorted(
        (q["tenor_years"], q["rate_pct"] / 100.0)
        for q in curve.get("quotes", [])
        if q.get("rate_pct") is not None
    )

    def zero_rate(t: float) -> float:
        if t <= pts[0][0]:
            return pts[0][1]
        if t >= pts[-1][0]:
            return pts[-1][1]
        for (t0, r0), (t1, r1) in zip(pts, pts[1:]):
            if t0 <= t <= t1:
                w = (t - t0) / (t1 - t0)
                return r0 + w * (r1 - r0)
        return pts[-1][1]

    def df(date: ql.Date) -> float:
        if date <= valuation:
            return 1.0
        t = (date - valuation) / 365.0
        return math.exp(-zero_rate(t) * t)

    return df


def quarterly_schedule_dates(dated: ql.Date, maturity: ql.Date) -> list[ql.Date]:
    """Generate a quarterly schedule walking backward from maturity, mirroring
    the Convex side's `quarterly_schedule`. NullCalendar + Unadjusted.
    """
    dates = [maturity]
    current = maturity
    while True:
        prev = current - ql.Period(3, ql.Months)
        if prev <= dated:
            dates.append(dated)
            break
        dates.append(prev)
        current = prev
    dates.reverse()
    return dates


def price_corporate_frn(inst: dict, valuation: ql.Date, sofr_curve: dict) -> dict:
    """Mirror of the Rust `price_corporate_frn` — see book.json::coupon_model_note.

    Returns a dict of metric-name → value, matching the Convex emitter.
    """
    dated = to_ql_date(inst.get("dated_date") or inst["issue_date"])
    maturity = to_ql_date(inst["maturity_date"])
    spread = inst["spread_bps"] / 10_000.0
    reset = inst["current_reset_rate_pct"] / 100.0

    df = build_zero_rate_df(sofr_curve, valuation)

    schedule = quarterly_schedule_dates(dated, maturity)
    face = 100.0
    dc360 = ql.Actual360()

    dirty = 0.0
    spread_annuity = 0.0
    last_coupon_before_settle = None

    for start, end in zip(schedule[:-1], schedule[1:]):
        if end <= valuation:
            last_coupon_before_settle = end
            continue
        if start <= valuation:
            last_coupon_before_settle = start

        df_start = df(start)
        df_end = df(end)
        yf360 = dc360.yearFraction(start, end)

        float_cf = face * (df_start / df_end - 1.0)
        spread_cf = face * spread * yf360
        cf = float_cf + spread_cf
        if end == maturity:
            cf += face
        dirty += cf * df_end
        spread_annuity += yf360 * df_end

    dirty_price_pct = dirty

    accrued = 0.0
    if last_coupon_before_settle is not None:
        yf = dc360.yearFraction(last_coupon_before_settle, valuation)
        accrued = face * reset * yf

    clean_price_pct = dirty_price_pct - accrued

    dm = (dirty_price_pct - 100.0 - accrued) / (spread_annuity * face) if abs(spread_annuity) > 1e-12 else 0.0
    discount_margin_bps = dm * 10_000.0

    return {
        "clean_price_pct": clean_price_pct,
        "dirty_price_pct": dirty_price_pct,
        "accrued": accrued,
        "discount_margin_bps": discount_margin_bps,
    }


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


def load_tips_index_ratios() -> dict[str, float]:
    """Map CUSIP → index ratio on the valuation date, from pull_market_data.py output."""
    out = {}
    ratio_file = HERE / "tips_index_ratio_20251231.json"
    if ratio_file.exists():
        data = json.loads(ratio_file.read_text())
        if data.get("index_ratio") is not None:
            out[data["cusip"]] = float(data["index_ratio"])
    return out


def main() -> int:
    book = json.loads((HERE / "book.json").read_text())
    curves = json.loads((HERE / "curves.json").read_text())
    curve_by_id = {c["id"]: c for c in curves["curves"]}
    if "UST_CMT" not in curve_by_id:
        raise RuntimeError("UST_CMT curve not found in curves.json")
    index_ratios = load_tips_index_ratios()

    valuation = to_ql_date(book["valuation_date"])
    ql.Settings.instance().evaluationDate = valuation

    rows: list[dict] = []
    skipped: list[str] = []

    sofr_curve = curve_by_id.get("SOFR_OIS_CURVE")

    for inst in book["instruments"]:
        cat = inst["category"]
        if cat in SKIP_CATEGORIES:
            skipped.append(f"{inst['id']} ({cat}) — {SKIP_CATEGORIES[cat]}")
            continue

        # Corporate SOFR FRN — dedicated pricing path off the SOFR OIS zero curve.
        if cat == "corporate_frn":
            if sofr_curve is None:
                raise RuntimeError(
                    f"{inst['id']}: SOFR_OIS_CURVE required for corporate_frn pricing"
                )
            m = price_corporate_frn(inst, valuation, sofr_curve)
            spread_dec = inst["spread_bps"] / 10_000.0
            for metric, value in m.items():
                rows.append(
                    {
                        "bond_id": inst["id"],
                        "currency": inst.get("currency", "USD"),
                        "metric": metric,
                        "value": f"{value:.10f}",
                        "reference_yield": f"{spread_dec:.10f}",
                        "curve_used": "SOFR_OIS_CURVE",
                        "notes": "",
                    }
                )
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
        else:
            curve_id = CCY_TO_CURVE_ID.get(ccy, "UST_CMT")
            curve = curve_by_id.get(curve_id)
            y = interpolate_curve(curve, yrs) if curve is not None else None
            if y is None:
                y = inst["coupon_rate"] / 100.0
                curve_used = "placeholder"
            else:
                curve_used = curve_id

        # Base metrics (treating the bond as if calls never happen).
        metrics = price_bond(inst, valuation, y)
        emitted: list[tuple[str, float]] = list(metrics.items())

        # Linker add-ons: nominal price/accrued = real × CPI index ratio.
        if is_linker:
            cusip = (inst.get("identifier") or {}).get("value")
            ratio = index_ratios.get(cusip)
            if ratio is not None:
                emitted.append(("cpi_index_ratio", ratio))
                emitted.append(("nominal_clean_price_pct", metrics["clean_price_pct"] * ratio))
                emitted.append(("nominal_dirty_price_pct", metrics["dirty_price_pct"] * ratio))
                emitted.append(("nominal_accrued", metrics["accrued"] * ratio))

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
