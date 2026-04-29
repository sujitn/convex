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
from typing import Optional

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

def build_sofr_curve(sofr: dict, valuation: ql.Date) -> ql.YieldTermStructureHandle:
    """Build a QL `ZeroCurve` from the SOFR OIS zero-rate panel. Linear interp
    in zero-rate space so both sides match point-for-point. Pillar days use
    half-away-from-zero rounding to match Rust's `f64::round` (Python's
    built-in `round` is banker's — they disagree at .5 boundaries like 6M)."""
    dc = ql.Actual365Fixed()
    dates = [valuation]
    rates = [sofr["quotes"][0]["rate_pct"] / 100.0]  # anchor at t=0
    for q in sofr["quotes"]:
        days = math.floor(q["tenor_years"] * 365.0 + 0.5)
        dates.append(valuation + ql.Period(days, ql.Days))
        rates.append(q["rate_pct"] / 100.0)
    curve = ql.ZeroCurve(dates, rates, dc, ql.NullCalendar(), ql.Linear())
    curve.enableExtrapolation()
    return ql.YieldTermStructureHandle(curve)


_SOFR_FIXINGS_LOADED = False


def _load_sofr_fixings(sofr_index: ql.OvernightIndex) -> int:
    """Register historical SOFR fixings with the supplied index.

    Reads reconciliation/sofr_fixings.csv (effective_date,rate_pct,...) once
    per process; subsequent calls return immediately. Returns the number of
    fixings registered on first call, 0 thereafter.
    """
    global _SOFR_FIXINGS_LOADED
    if _SOFR_FIXINGS_LOADED:
        return 0
    path = HERE / "sofr_fixings.csv"
    if not path.exists():
        _SOFR_FIXINGS_LOADED = True
        return 0
    n = 0
    with path.open() as f:
        reader = csv.DictReader(f)
        for row in reader:
            d = to_ql_date(row["effective_date"])
            r = float(row["rate_pct"]) / 100.0
            sofr_index.addFixing(d, r, True)  # forceOverwrite=True
            n += 1
    _SOFR_FIXINGS_LOADED = True
    return n


def price_corporate_frn(inst: dict, valuation: ql.Date, sofr: dict) -> dict:
    """ARRC compound-in-arrears pricing on the QL side.

    In-progress period is priced under `OvernightIndexedCoupon` with
    `applyObservationShift=True, lookbackDays=2, lockoutDays=0` consuming
    real fixings from `sofr_fixings.csv` for past business days and the
    SOFR projection curve for the rest. Future periods use the same
    machinery (forecast-only, no fixings touched). Spread is additive
    (post-compounding), matching `reconcile_bench`'s
    `price_corporate_frn`.
    """
    dated = to_ql_date(inst.get("dated_date") or inst["issue_date"])
    maturity = to_ql_date(inst["maturity_date"])
    spread = inst["spread_bps"] / 10_000.0
    face = 100.0
    dc360 = ql.Actual360()

    handle = build_sofr_curve(sofr, valuation)
    sofr_index = ql.Sofr(handle)
    _load_sofr_fixings(sofr_index)

    cal = ql.UnitedStates(ql.UnitedStates.GovernmentBond)
    schedule = ql.Schedule(
        dated, maturity, ql.Period(ql.Quarterly), cal,
        ql.Unadjusted, ql.Unadjusted, ql.DateGeneration.Backward, False,
    )

    def df(d: ql.Date) -> float:
        return 1.0 if d <= valuation else handle.discount(d)

    dirty = 0.0
    spread_annuity = 0.0
    accrued = 0.0

    for start, end in zip(schedule, list(schedule)[1:]):
        if end <= valuation:
            continue

        yf = dc360.yearFraction(start, end)

        if start <= valuation:
            # In-progress period: real ARRC compounded coupon.
            coupon_obj = ql.OvernightIndexedCoupon(
                cal.adjust(end, ql.Following),  # paymentDate
                face,                           # nominal
                start, end,                     # startDate, endDate
                sofr_index,
                1.0,                            # gearing
                0.0,                            # spread (we add it manually, additive)
                ql.Date(), ql.Date(),           # refPeriodStart/End
                dc360,
                False,                          # telescopicValueDates
                ql.RateAveraging.Compound,
                2,                              # lookbackDays
                0,                              # lockoutDays
                True,                           # applyObservationShift
            )
            comp_rate = coupon_obj.rate()       # annualized compounded SOFR
            comp_minus_one = comp_rate * yf
            coupon = face * (comp_minus_one + spread * yf)

            # Accrued portion: same coupon construction over [start, valuation].
            accr_obj = ql.OvernightIndexedCoupon(
                cal.adjust(valuation, ql.Following),
                face,
                start, valuation,
                sofr_index,
                1.0, 0.0,
                ql.Date(), ql.Date(),
                dc360,
                False,
                ql.RateAveraging.Compound,
                2, 0, True,
            )
            accr_yf = dc360.yearFraction(start, valuation)
            accrued = face * (accr_obj.rate() * accr_yf + spread * accr_yf)
        else:
            # Future period: curve-forward equals deterministic compound rate.
            coupon = face * (df(start) / df(end) - 1.0 + spread * yf)

        amount = coupon + (face if end == maturity else 0.0)
        dirty += amount * df(end)
        spread_annuity += yf * df(end)

    clean = dirty - accrued
    dm = (dirty - 100.0 - accrued) / (spread_annuity * face) if abs(spread_annuity) > 1e-12 else 0.0

    return {
        "clean_price_pct": clean,
        "dirty_price_pct": dirty,
        "accrued": accrued,
        "discount_margin_bps": dm * 10_000.0,
    }


def years_to_maturity(valuation: ql.Date, maturity: ql.Date) -> float:
    return (maturity - valuation) / 365.25


# ---------------------------------------------------------------- callable OAS

# Hull-White 1F mean reversion is fixed exogenously (industry standard for
# corporate callable OAS — Bloomberg OAS1, Strata, QL's CallableBonds.cpp
# example all do this). Volatility σ is calibrated per-bond against a
# co-terminal ATM USD SOFR swaption strip via SwaptionHelper + LM.
HW_MEAN_REVERSION_FIXED = 0.03
# Initial guess for σ before calibration — a reasonable post-2022 USD prior.
HW_VOLATILITY_INITIAL = 0.008
# 500 timesteps to match the Convex side. QL's TimeGrid auto-injects all
# event dates regardless, but lifting numTimeSteps from 100→500 keeps QL
# from being limited by the parameter when the auto-injected grid is sparse.
HW_TREE_STEPS = 500


def load_swaption_surface(path: pathlib.Path) -> list[tuple[float, float]]:
    """Read an ATM normal-vol surface CSV (expiry_years, atm_normal_vol_bps).

    Lines starting with ``#`` are comments. Returns sorted (expiry, vol_bp)
    pairs for linear interpolation.
    """
    pts: list[tuple[float, float]] = []
    with path.open() as fh:
        for row in csv.reader(fh):
            if not row or row[0].lstrip().startswith("#"):
                continue
            if row[0].strip() == "expiry_years":
                continue
            pts.append((float(row[0]), float(row[1])))
    pts.sort()
    if not pts:
        raise ValueError(f"swaption surface {path} is empty")
    return pts


def interp_vol_bp(surface: list[tuple[float, float]], expiry_yrs: float) -> float:
    """Linear interp on (expiry_years, atm_normal_vol_bps); flat extrapolation."""
    if expiry_yrs <= surface[0][0]:
        return surface[0][1]
    for (t0, v0), (t1, v1) in zip(surface, surface[1:]):
        if t0 <= expiry_yrs <= t1:
            w = (expiry_yrs - t0) / (t1 - t0)
            return v0 + w * (v1 - v0)
    return surface[-1][1]


def coterminal_helpers(
    valuation: ql.Date,
    maturity: ql.Date,
    surface: list[tuple[float, float]],
    base_handle: ql.YieldTermStructureHandle,
) -> list[tuple[ql.SwaptionHelper, int, int, float]]:
    """Build a co-terminal ATM SOFR swaption strip for this bond.

    Strip structure follows the QuantLib bermudan-swaption.py example: integer-
    year expiries 1, 2, ..., floor(residual_yrs - 0.5), with tail = maturity -
    expiry. Each helper is ATM (strike = fair forward swap rate) with normal/
    Bachelier vol pulled from the surface by linear interp on expiry. Returns
    (helper, expiry_yrs, tail_yrs, vol_bp) so we can record the strip used.
    """
    residual = (maturity - valuation) / 365.25
    max_expiry = int(math.floor(residual - 0.5))
    if max_expiry < 1:
        return []

    sofr_index = ql.OvernightIndex(
        "SOFR", 2, ql.USDCurrency(), ql.UnitedStates(ql.UnitedStates.SOFR),
        ql.Actual360(), base_handle,
    )

    helpers: list[tuple[ql.SwaptionHelper, int, int, float]] = []
    for expiry in range(1, max_expiry + 1):
        tail = max(1, int(round(residual - expiry)))
        vol_bp = interp_vol_bp(surface, float(expiry))
        vol_quote = ql.QuoteHandle(ql.SimpleQuote(vol_bp / 1e4))
        helper = ql.SwaptionHelper(
            ql.Period(expiry, ql.Years),
            ql.Period(tail, ql.Years),
            vol_quote,
            sofr_index,
            ql.Period(1, ql.Years),       # fixed-leg tenor (annual is fine for ATM strike)
            ql.Thirty360(ql.Thirty360.USA),
            ql.Actual360(),
            base_handle,
            ql.BlackCalibrationHelper.RelativePriceError,
            ql.nullDouble(),               # ATM strike
            1.0,                           # nominal
            ql.Normal,                     # Bachelier/normal vol
        )
        helpers.append((helper, expiry, tail, vol_bp))
    return helpers


def calibrate_hw1f(
    helpers: list[ql.SwaptionHelper],
    base_handle: ql.YieldTermStructureHandle,
) -> tuple[float, float, float]:
    """Calibrate HW1F σ against the supplied helpers, holding a = 0.03 fixed.

    Returns (a, sigma, rmse_price). The fixParameters mask [True, False] is
    QuantLib's canonical "fix a, float σ" switch.
    """
    model = ql.HullWhite(base_handle, HW_MEAN_REVERSION_FIXED, HW_VOLATILITY_INITIAL)
    engine = ql.JamshidianSwaptionEngine(model, base_handle)
    for h in helpers:
        h.setPricingEngine(engine)
    optimizer = ql.LevenbergMarquardt(1e-8, 1e-8, 1e-8)
    end_criteria = ql.EndCriteria(400, 100, 1e-8, 1e-8, 1e-8)
    model.calibrate(
        helpers, optimizer, end_criteria,
        ql.NoConstraint(), [], [True, False],
    )
    a, sigma = model.params()[0], model.params()[1]
    sse = 0.0
    for h in helpers:
        diff = h.modelValue() - h.marketValue()
        sse += diff * diff
    rmse = math.sqrt(sse / max(len(helpers), 1))
    return a, sigma, rmse


def _build_callable_bond(inst: dict) -> ql.CallableFixedRateBond:
    """Constructs a `ql.CallableFixedRateBond` from a book.json callable
    record. Mirrors `build_callable_bond` on the Convex side.

    Convex's `CallType::American` is continuously callable from the first
    `call_date` with a step-down price. QL's `CallabilitySchedule` is
    Bermudan — each entry is a single-date right. To match the American
    semantics we densify the schedule to a monthly grid from the first
    `call_date` through maturity, with each grid date carrying the
    prevailing step-down price.
    """
    issue = to_ql_date(inst.get("dated_date") or inst["issue_date"])
    maturity = to_ql_date(inst["maturity_date"])
    freq = FREQUENCY_MAP[inst["frequency"].lower()]
    dcc = day_count(inst["day_count"])
    coupon = effective_coupon_pct(inst) / 100.0

    schedule = ql.Schedule(
        issue, maturity, ql.Period(freq), ql.NullCalendar(),
        ql.Unadjusted, ql.Unadjusted, ql.DateGeneration.Backward, False,
    )
    callability = ql.CallabilitySchedule()

    raw = sorted(
        inst.get("call_schedule") or [],
        key=lambda e: to_ql_date(e["call_date"]),
    )
    if raw:
        first = to_ql_date(raw[0]["call_date"])
        # Walk a *daily* grid from the first call date to (maturity - 1d).
        # Skip past `maturity` itself — QL doesn't accept a callability on or
        # after the bond's maturity. Daily granularity is required: any
        # coarser grid (monthly, weekly) leaves QL exercising on fewer dates
        # than Convex's American-callable backward induction, which checks
        # call optionality at every tree step.
        dt = first
        while dt < maturity:
            applicable = [e for e in raw if to_ql_date(e["call_date"]) <= dt]
            price = applicable[-1]["price"]
            callability.append(
                ql.Callability(
                    ql.BondPrice(price, ql.BondPrice.Clean),
                    ql.Callability.Call,
                    dt,
                )
            )
            dt = dt + ql.Period(1, ql.Days)

    return ql.CallableFixedRateBond(
        0, 100.0, schedule, [coupon], dcc, ql.Following,
        100.0, issue, callability,
    )


def _hw_engine_for_curve(
    base_handle: ql.YieldTermStructureHandle,
    total_shift: float,
    a: float,
    sigma: float,
) -> ql.TreeCallableFixedRateBondEngine:
    """Builds an HW1F-tree pricing engine for a curve = base + total_shift
    (continuous-compounded parallel shift). `total_shift` packs OAS plus
    any rate-bump (used for effective duration / convexity). `(a, sigma)`
    come from per-bond swaption-strip calibration."""
    if abs(total_shift) < 1e-15:
        handle = base_handle
    else:
        spread_quote = ql.QuoteHandle(ql.SimpleQuote(total_shift))
        handle = ql.YieldTermStructureHandle(
            ql.ZeroSpreadedTermStructure(base_handle, spread_quote)
        )
    model = ql.HullWhite(handle, a, sigma)
    return ql.TreeCallableFixedRateBondEngine(model, HW_TREE_STEPS)


def _ql_callable_price(
    inst: dict,
    base_handle: ql.YieldTermStructureHandle,
    oas: float,
    a: float,
    sigma: float,
    rate_shift: float = 0.0,
) -> float:
    """Clean-price the callable under OAS (and optional parallel rate
    bump) using `TreeCallableFixedRateBondEngine` on a HW1F tree."""
    bond = _build_callable_bond(inst)
    engine = _hw_engine_for_curve(base_handle, oas + rate_shift, a, sigma)
    bond.setPricingEngine(engine)
    return bond.cleanPrice()


def _ql_solve_oas_at_price(
    inst: dict,
    base_handle: ql.YieldTermStructureHandle,
    target_clean: float,
    a: float,
    sigma: float,
) -> float:
    """Brent solver: OAS such that tree-price(OAS) == `target_clean`.
    Mirrors `OASCalculator.calculate()` on the Convex side."""
    def f(oas: float) -> float:
        return _ql_callable_price(inst, base_handle, oas, a, sigma) - target_clean
    solver = ql.Brent()
    solver.setMaxEvaluations(100)
    return solver.solve(f, 1e-8, 0.0, -0.05, 0.10)


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


# QuantLib's BondFunctions don't define a frequency-typed yield quote on a
# strict zero-coupon bond (Frequency::Zero confuses Compounded yields). The
# convention here is the same as Convex's bench: reconcile a STRIP / T-Bill
# under the user-chosen yield-quotation compounding (typically Semiannual
# for US conventions).
COMPOUNDING_FROM_NAME = {
    "annual": ql.Annual,
    "semi_annual": ql.Semiannual,
    "semi-annual": ql.Semiannual,
    "quarterly": ql.Quarterly,
    "monthly": ql.Monthly,
}


def make_whole_redemption_qq(
    inst: dict,
    call_date: ql.Date,
    treasury_rate: float,
) -> float:
    """Mirror of CallableBond::make_whole_call_price (ACT/365F × bond freq)."""
    spread_bps = (inst.get("make_whole") or {}).get("spread_bps") or 0.0
    discount_rate = treasury_rate + spread_bps / 10_000.0
    coupon = inst["coupon_rate"] / 100.0
    freq = FREQUENCY_MAP[inst["frequency"].lower()]
    periods_per_year = {ql.Annual: 1, ql.Semiannual: 2, ql.Quarterly: 4, ql.Monthly: 12}[freq]
    issue = to_ql_date(inst.get("dated_date") or inst["issue_date"])
    maturity = to_ql_date(inst["maturity_date"])

    schedule = ql.Schedule(
        issue, maturity, ql.Period(freq), ql.NullCalendar(),
        ql.Unadjusted, ql.Unadjusted, ql.DateGeneration.Backward, False,
    )

    pv = 0.0
    period_coupon = coupon * 100.0 / periods_per_year
    schedule_dates = list(schedule)
    for i in range(1, len(schedule_dates)):
        pay_date = schedule_dates[i]
        if pay_date <= call_date:
            continue
        amount = period_coupon
        if pay_date == maturity:
            amount += 100.0
        t_years = (pay_date - call_date) / 365.0
        df = 1.0 / (1.0 + discount_rate / periods_per_year) ** (periods_per_year * t_years)
        pv += amount * df

    # Floor at first call entry's price (typically par), matching Convex semantics.
    floor = 100.0
    call_schedule = inst.get("call_schedule") or []
    if call_schedule:
        floor = call_schedule[0].get("price", 100.0)
    return max(pv, floor)


def price_sinker(inst: dict, valuation: ql.Date, ref_yield: float) -> dict[str, float]:
    """ql.AmortizingFixedRateBond pricing. Coupon at sink date accrues on
    pre-paydown notional."""
    issue = to_ql_date(inst.get("dated_date") or inst["issue_date"])
    maturity = to_ql_date(inst["maturity_date"])
    freq = FREQUENCY_MAP[inst["frequency"].lower()]
    dcc = day_count(inst["day_count"])
    coupon = inst["coupon_rate"] / 100.0

    schedule = ql.Schedule(
        issue, maturity, ql.Period(freq), ql.NullCalendar(),
        ql.Unadjusted, ql.Unadjusted, ql.DateGeneration.Backward, False,
    )

    # Build the per-period notional vector. `outstanding` tracks the original-face
    # fraction outstanding during each interval `[schedule[i], schedule[i+1])`.
    sink_by_date: dict[int, float] = {}  # serialNumber -> amount_pct paid AT that date
    for entry in inst.get("sinking_schedule") or []:
        sink_by_date[to_ql_date(entry["date"]).serialNumber()] = entry["amount_pct"]

    notionals: list[float] = []
    outstanding_pct = 100.0
    schedule_dates = list(schedule)
    for i in range(len(schedule_dates) - 1):
        notionals.append(outstanding_pct)  # outstanding during this period
        # Paydown that happens AT schedule_dates[i+1] reduces the next period's
        # notional. The principal payment AT schedule_dates[i+1] is
        # `notionals[i] - notionals[i+1]` per QL semantics.
        next_date_serial = schedule_dates[i + 1].serialNumber()
        if next_date_serial in sink_by_date:
            outstanding_pct -= sink_by_date[next_date_serial]
            outstanding_pct = max(0.0, outstanding_pct)

    bond = ql.AmortizingFixedRateBond(
        0,                # settlementDays
        notionals,
        schedule,
        [coupon],
        dcc,
        ql.Following,
        issue,
    )

    comp = ql.Compounded
    cmp_freq = COMPOUNDING_FREQUENCY[freq]
    clean = ql.BondFunctions.cleanPrice(bond, ref_yield, dcc, comp, cmp_freq, valuation)
    accrued = ql.BondFunctions.accruedAmount(bond, valuation)
    dirty = clean + accrued

    bond_price = ql.BondPrice(clean, ql.BondPrice.Clean)
    ytm = ql.BondFunctions.bondYield(bond, bond_price, dcc, comp, cmp_freq, valuation)

    interest_rate = ql.InterestRate(ref_yield, dcc, comp, cmp_freq)
    mac_dur = ql.BondFunctions.duration(bond, interest_rate, ql.Duration.Macaulay, valuation)
    mod_dur = ql.BondFunctions.duration(bond, interest_rate, ql.Duration.Modified, valuation)
    cvx = ql.BondFunctions.convexity(bond, interest_rate, valuation)
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


def price_zero_coupon(inst: dict, valuation: ql.Date, ref_yield: float) -> dict[str, float]:
    """ql.ZeroCouponBond pricing for sovereign_strip entries (T+0 settle)."""
    issue = to_ql_date(inst.get("dated_date") or inst["issue_date"])
    maturity = to_ql_date(inst["maturity_date"])
    dcc = day_count(inst["day_count"])
    cmp_freq = COMPOUNDING_FROM_NAME[inst["frequency"].lower()]

    bond = ql.ZeroCouponBond(
        0,                       # settlementDays — T+0, valuation is the analysis date
        ql.NullCalendar(),
        100.0,                   # face value
        maturity,
        ql.Following,            # paymentConvention (no coupons; only redemption)
        100.0,                   # redemption
        issue,                   # issueDate
    )

    comp = ql.Compounded
    clean = ql.BondFunctions.cleanPrice(bond, ref_yield, dcc, comp, cmp_freq, valuation)
    accrued = ql.BondFunctions.accruedAmount(bond, valuation)  # always 0 for zero
    dirty = clean + accrued

    bond_price = ql.BondPrice(clean, ql.BondPrice.Clean)
    ytm = ql.BondFunctions.bondYield(bond, bond_price, dcc, comp, cmp_freq, valuation)

    interest_rate = ql.InterestRate(ref_yield, dcc, comp, cmp_freq)
    mac_dur = ql.BondFunctions.duration(bond, interest_rate, ql.Duration.Macaulay, valuation)
    mod_dur = ql.BondFunctions.duration(bond, interest_rate, ql.Duration.Modified, valuation)
    cvx = ql.BondFunctions.convexity(bond, interest_rate, valuation)
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


SNAPSHOTS = [
    {
        "book": "book.json",
        "curves": "curves.json",
        "out": "ql.csv",
        "require_ust_cmt": True,
        "swaptions": "swaptions_20251231.csv",
        "hw1f_params": "hw1f_params_20251231.json",
    },
    {
        "book": "book_20250630.json",
        "curves": "curves_20250630.json",
        "out": "ql_20250630.csv",
        "require_ust_cmt": False,
        # 2025-06-30 mid-period snapshot is FRN-focused; no callables in book.
        "swaptions": None,
        "hw1f_params": None,
    },
]


def main() -> int:
    global _SOFR_FIXINGS_LOADED  # reset per-snapshot so SOFR fixings re-register on the new index
    rc = 0
    for snap in SNAPSHOTS:
        _SOFR_FIXINGS_LOADED = False
        rc |= _run_snapshot(snap)
    return rc


def _run_snapshot(snap: dict) -> int:
    book = json.loads((HERE / snap["book"]).read_text())
    curves = json.loads((HERE / snap["curves"]).read_text())
    curve_by_id = {c["id"]: c for c in curves["curves"]}
    if snap["require_ust_cmt"] and "UST_CMT" not in curve_by_id:
        raise RuntimeError(f"UST_CMT curve not found in {snap['curves']}")
    index_ratios = load_tips_index_ratios()

    valuation = to_ql_date(book["valuation_date"])
    ql.Settings.instance().evaluationDate = valuation

    rows: list[dict] = []
    skipped: list[str] = []

    sofr_curve = curve_by_id.get("SOFR_OIS_CURVE")

    # HW1F per-bond calibration: load swaption surface once for this snapshot,
    # accumulate calibrated (a, σ) per callable, dump to hw1f_params_*.json.
    swaption_surface: Optional[list[tuple[float, float]]] = None
    if snap.get("swaptions"):
        swaption_surface = load_swaption_surface(HERE / snap["swaptions"])
    hw1f_calibrations: dict[str, dict] = {}

    for inst in book["instruments"]:
        cat = inst["category"]
        if cat in SKIP_CATEGORIES:
            skipped.append(f"{inst['id']} ({cat}) — {SKIP_CATEGORIES[cat]}")
            continue

        if cat == "synthetic_puttable":
            ccy = inst.get("currency", "USD")
            maturity = to_ql_date(inst["maturity_date"])
            yrs = years_to_maturity(valuation, maturity)
            curve_id = CCY_TO_CURVE_ID.get(ccy, "UST_CMT")
            curve = curve_by_id.get(curve_id)
            y = interpolate_curve(curve, yrs) if curve is not None else None
            if y is None:
                y = inst["coupon_rate"] / 100.0
                curve_used = "placeholder"
            else:
                curve_used = curve_id

            metrics = price_bond(inst, valuation, y)
            emitted: list[tuple[str, float]] = list(metrics.items())

            # YTP at each put date + YTB (best for holder = max yield).
            clean = metrics["clean_price_pct"]
            ytm = metrics["ytm_decimal"]
            bond_price = ql.BondPrice(clean, ql.BondPrice.Clean)
            best_yield = ytm
            best_date = maturity
            for entry in inst.get("put_schedule") or []:
                put_date = to_ql_date(entry["put_date"])
                if put_date <= valuation:
                    continue
                wb, wb_dcc, wb_freq = build_bullet(inst, put_date, entry["price"])
                comp = ql.Compounded
                cmp_freq = COMPOUNDING_FREQUENCY[wb_freq]
                ytp = ql.BondFunctions.bondYield(
                    wb, bond_price, wb_dcc, comp, cmp_freq, valuation
                )
                key = f"ytp_{entry['put_date'].replace('-', '')}_decimal"
                emitted.append((key, ytp))
                if ytp > best_yield:
                    best_yield = ytp
                    best_date = put_date
            emitted.append(("ytb_decimal", best_yield))
            yyyymmdd = best_date.year() * 10000 + best_date.month() * 100 + best_date.dayOfMonth()
            emitted.append(("ytb_workout_date_yyyymmdd", float(yyyymmdd)))

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
            continue

        if cat == "synthetic_sinker":
            ccy = inst.get("currency", "USD")
            maturity = to_ql_date(inst["maturity_date"])
            yrs = years_to_maturity(valuation, maturity)
            curve_id = CCY_TO_CURVE_ID.get(ccy, "UST_CMT")
            curve = curve_by_id.get(curve_id)
            y = interpolate_curve(curve, yrs) if curve is not None else None
            if y is None:
                y = inst.get("coupon_rate", 0.0) / 100.0
                curve_used = "placeholder"
            else:
                curve_used = curve_id
            metrics = price_sinker(inst, valuation, y)
            for metric, value in metrics.items():
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
            continue

        if cat == "sovereign_strip":
            ccy = inst.get("currency", "USD")
            maturity = to_ql_date(inst["maturity_date"])
            yrs = years_to_maturity(valuation, maturity)
            curve_id = CCY_TO_CURVE_ID.get(ccy, "UST_CMT")
            curve = curve_by_id.get(curve_id)
            y = interpolate_curve(curve, yrs) if curve is not None else None
            if y is None:
                y = inst.get("coupon_rate", 0.0) / 100.0
                curve_used = "placeholder"
            else:
                curve_used = curve_id
            metrics = price_zero_coupon(inst, valuation, y)
            for metric, value in metrics.items():
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

            # Tier 5.2.2: HW1F per-bond σ calibrated against an ATM USD SOFR
            # co-terminal swaption strip (a fixed at 0.03). Both sides use the
            # SOFR_OIS_CURVE as the discount curve (continuously-compounded).
            if sofr_curve is not None and swaption_surface is not None:
                base_handle = build_sofr_curve(sofr_curve, valuation)
                strip = coterminal_helpers(
                    valuation, maturity, swaption_surface, base_handle,
                )
                if not strip:
                    raise RuntimeError(
                        f"{inst['id']}: residual maturity too short for a "
                        "co-terminal swaption strip"
                    )
                helpers = [h for (h, _, _, _) in strip]
                a_cal, sigma_cal, rmse = calibrate_hw1f(helpers, base_handle)
                hw1f_calibrations[inst["id"]] = {
                    "a": a_cal,
                    "sigma": sigma_cal,
                    "rmse_price": rmse,
                    "helpers": [
                        {"expiry_yrs": e, "tail_yrs": t, "atm_normal_vol_bps": v}
                        for (_, e, t, v) in strip
                    ],
                }
                # Tier 5.2.4: emit calibrated (a, σ) as reconciliation rows so
                # the Rust independent calibration can diff against QL's σ to
                # ~1e-5 tolerance (calibration parity, separate from pricing
                # parity which uses these same params).
                emitted.append(("hw1f_a_calibrated", a_cal))
                emitted.append(("hw1f_sigma_calibrated", sigma_cal))
                # Stage 1: OAS-given parity. Three reference spreads.
                for bps in (25, 50, 100):
                    px = _ql_callable_price(
                        inst, base_handle, bps / 10_000.0, a_cal, sigma_cal,
                    )
                    emitted.append((f"price_at_oas_{bps}bps", px))
                # Stage 2: OAS-from-price parity. Use a synthetic 99.0 target
                # so the solver runs end-to-end and Convex/QL agree on the
                # implied spread.
                target = 99.0
                oas = _ql_solve_oas_at_price(
                    inst, base_handle, target, a_cal, sigma_cal,
                )
                emitted.append(("oas_bps_at_market", oas * 10_000.0))
                # Effective duration + convexity at the solved OAS, sticky-strike
                # (hold OAS fixed, parallel-shift the rate curve ±1 bp).
                shift = 1e-4
                px0 = _ql_callable_price(inst, base_handle, oas, a_cal, sigma_cal, 0.0)
                px_up = _ql_callable_price(inst, base_handle, oas, a_cal, sigma_cal, shift)
                px_dn = _ql_callable_price(inst, base_handle, oas, a_cal, sigma_cal, -shift)
                eff_dur = (px_dn - px_up) / (2.0 * px0 * shift)
                eff_cnv = (px_dn + px_up - 2.0 * px0) / (px0 * shift * shift)
                emitted.append(("effective_duration_at_oas", eff_dur))
                emitted.append(("effective_convexity_at_oas", eff_cnv))

        mw_block = inst.get("make_whole") or {}
        if mw_block.get("spread_bps") is not None:
            for metric, call_date_str, treasury_rate in [
                ("mw_redemption_call_2026_06_15_ust_300bps", "2026-06-15", 0.030),
                ("mw_redemption_call_2026_06_15_ust_500bps", "2026-06-15", 0.050),
            ]:
                mw = make_whole_redemption_qq(inst, to_ql_date(call_date_str), treasury_rate)
                emitted.append((metric, mw))

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

    out = HERE / snap["out"]
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

    if snap.get("hw1f_params") and hw1f_calibrations:
        params_path = HERE / snap["hw1f_params"]
        params_path.write_text(
            json.dumps(
                {
                    "snapshot_date": book["valuation_date"],
                    "swaption_strip_source": snap.get("swaptions"),
                    "calibrations": hw1f_calibrations,
                },
                indent=2,
            )
        )
        print(
            f"ql_bench: wrote {params_path} — {len(hw1f_calibrations)} HW1F calibrations",
            file=sys.stderr,
        )

    print(f"ql_bench: wrote {out} — {len(rows) // 8} bonds priced", file=sys.stderr)
    if skipped:
        print("ql_bench: skipped:", file=sys.stderr)
        for s in skipped:
            print(f"  - {s}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
