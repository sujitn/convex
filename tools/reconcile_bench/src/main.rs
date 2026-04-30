//! Convex side of the QuantLib reconciliation bench.
//!
//! Reads reconciliation/book.json and curves.json, prices every vanilla
//! fixed-rate bullet bond with Convex, and emits reconciliation/convex.csv.
//!
//! Scope (Milestone 2 MVP):
//! - Fixed-rate bullet bonds only. Callable, FRN, TIPS are intentionally
//!   skipped here; they land in a later milestone.
//! - For each bond, the "reference yield" is the UST CMT yield at the bond's
//!   remaining maturity (linear interpolation). Non-USD bonds fall back to
//!   a placeholder and are flagged in the output.
//! - Columns: clean_price_pct, dirty_price_pct, accrued, ytm,
//!   macaulay_duration, modified_duration, convexity, dv01.
//!
//! Run from the repo root:
//!   cargo run -p reconcile_bench
//!
//! Output: reconciliation/convex.csv (one row per (bond_id, metric)).

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::Deserialize;

use convex_analytics::calibration::{calibrate_hw1f_sigma, CoterminalSwaptionHelper};
use convex_analytics::functions::{
    clean_price_from_yield, convexity, dirty_price_from_yield, dv01, macaulay_duration,
    modified_duration, parse_day_count, yield_to_maturity,
};
use convex_analytics::spreads::{DiscountMarginCalculator, OASCalculator};
use convex_bonds::arrc::{compound_in_arrears, ArrcConfig};
use convex_bonds::fixings::OvernightFixings;
use convex_bonds::instruments::{
    CallableBond, CallableFloatingRateNote, Compounding as ZcbCompounding, FixedRateBond,
    FloatingRateNote, SinkingFundBond, SinkingFundPayment, SinkingFundSchedule, ZeroCouponBond,
};
use convex_bonds::options::HullWhite;
use convex_bonds::traits::Bond;
use convex_bonds::types::{
    CalendarId, CallEntry as BondCallEntry, CallSchedule as BondCallSchedule, CallType,
    PutEntry as BondPutEntry, PutSchedule as BondPutSchedule, PutType,
};
use convex_core::calendars::BusinessDayConvention;
use convex_core::daycounts::DayCountConvention;
use convex_core::types::{Currency, Date, Frequency};
use convex_curves::curves::{DiscountCurve, DiscountCurveBuilder};
use convex_curves::InterpolationMethod;

// ------------------------------------------------------------------ schemas

#[derive(Debug, Deserialize)]
struct Book {
    valuation_date: String,
    instruments: Vec<Instrument>,
}

#[derive(Debug, Deserialize)]
struct Instrument {
    id: String,
    category: String,
    #[allow(dead_code)]
    issuer: String,
    coupon_rate: Option<f64>,
    #[serde(default)]
    coupon_unit: Option<String>,
    issue_date: Option<String>,
    #[serde(default)]
    dated_date: Option<String>,
    maturity_date: Option<String>,
    frequency: Option<String>,
    day_count: Option<String>,
    #[allow(dead_code)]
    #[serde(default)]
    face_value: Option<f64>,
    currency: Option<String>,
    #[allow(dead_code)]
    #[serde(default)]
    synthetic: bool,
    #[serde(default)]
    call_schedule: Option<Vec<CallEntry>>,
    // FRN-specific, ignored for other categories.
    #[serde(default)]
    index_rate_pct: Option<f64>,
    #[serde(default)]
    spread_bps: Option<f64>,
    // Zero-coupon-specific: yield-quotation compounding (e.g. "semi_annual",
    // "annual", "continuous"). Ignored for other categories.
    #[serde(default)]
    compounding: Option<String>,
    // Sinker-specific: per-paydown schedule (date, amount_pct of original
    // face, price as % of par). Ignored for other categories.
    #[serde(default)]
    sinking_schedule: Option<Vec<SinkEntry>>,
    // Make-whole-specific: spread + active-until window. Bonds with
    // `make_whole.spread_bps != null` have their MW redemption price
    // reconciled against QL at fixed (call_date, treasury_rate) scenarios.
    #[serde(default)]
    make_whole: Option<MakeWholeBlock>,
    // Puttable-specific: Bermudan put schedule. Holder's YTB / workout-bullet
    // metrics are computed when present.
    #[serde(default)]
    put_schedule: Option<Vec<PutEntryRaw>>,
    // Corporate-SOFR-FRN-specific: last reset rate used for in-progress accrual.
    // Retained as a fallback / sanity field; the ARRC path prefers
    // sofr_fixings.csv.
    #[allow(dead_code)]
    #[serde(default)]
    current_reset_rate_pct: Option<f64>,
    // Linker-specific: CUSIP is looked up in the TIPS index-ratio file.
    #[serde(default)]
    identifier: Option<Identifier>,
}

#[derive(Debug, Deserialize)]
struct Identifier {
    #[serde(default)]
    value: Option<String>,
}

/// TIPS index ratio file written by pull_tips_index_ratio() in pull_market_data.py.
#[derive(Debug, Deserialize)]
struct TipsIndexRatio {
    cusip: String,
    index_ratio: f64,
}

/// QL-emitted HW1F calibration file. Convex consumes `(a, σ)` as inputs to
/// the OAS pricer and re-runs the calibration on the same strip as a parity
/// test (5.2.4); the strip is dumped here too so we don't re-derive it.
#[derive(Debug, Deserialize)]
struct Hw1fCalibrations {
    calibrations: std::collections::HashMap<String, Hw1fParams>,
}

#[derive(Debug, Deserialize, Clone)]
struct Hw1fParams {
    a: f64,
    sigma: f64,
    helpers: Vec<Hw1fHelper>,
}

#[derive(Debug, Deserialize, Clone, Copy)]
struct Hw1fHelper {
    expiry_yrs: f64,
    tail_yrs: f64,
    atm_normal_vol_bps: f64,
}

#[derive(Debug, Deserialize)]
struct CallEntry {
    call_date: String,
    price: f64,
}

#[derive(Debug, Deserialize)]
struct PutEntryRaw {
    put_date: String,
    price: f64,
}

#[derive(Debug, Deserialize)]
struct MakeWholeBlock {
    #[serde(default)]
    spread_bps: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct SinkEntry {
    date: String,
    amount_pct: f64,
    #[serde(default = "default_par_price")]
    price: f64,
}

fn default_par_price() -> f64 {
    100.0
}

#[derive(Debug, Deserialize)]
struct Curves {
    curves: Vec<Curve>,
}

#[derive(Debug, Deserialize)]
struct Curve {
    id: String,
    #[allow(dead_code)]
    #[serde(default)]
    currency: String,
    #[serde(default)]
    quotes: Vec<Quote>,
}

#[derive(Debug, Deserialize)]
struct Quote {
    tenor_years: f64,
    rate_pct: Option<f64>,
}

// ------------------------------------------------------------------ helpers

fn parse_date(s: &str) -> Result<Date> {
    // "YYYY-MM-DD"
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 3 {
        return Err(anyhow!("bad date {s}"));
    }
    let y: i32 = parts[0].parse()?;
    let m: u32 = parts[1].parse()?;
    let d: u32 = parts[2].parse()?;
    Date::from_ymd(y, m, d).map_err(|e| anyhow!("Date::from_ymd failed for {s}: {e}"))
}

fn parse_frequency(s: &str) -> Result<Frequency> {
    match s.to_ascii_lowercase().replace('_', "-").as_str() {
        "annual" => Ok(Frequency::Annual),
        "semi-annual" | "semi" => Ok(Frequency::SemiAnnual),
        "quarterly" => Ok(Frequency::Quarterly),
        "monthly" => Ok(Frequency::Monthly),
        other => Err(anyhow!("unsupported frequency {other}")),
    }
}

fn parse_currency(s: &str) -> Result<Currency> {
    match s {
        "USD" => Ok(Currency::USD),
        "GBP" => Ok(Currency::GBP),
        "EUR" => Ok(Currency::EUR),
        "JPY" => Ok(Currency::JPY),
        other => Err(anyhow!("unsupported currency {other}")),
    }
}

/// Book.json coupon is percent unless `coupon_unit` says otherwise. Returns a decimal.
fn coupon_to_decimal(pct: f64, unit: Option<&str>) -> Decimal {
    let is_decimal = matches!(unit, Some(u) if u.contains("decimal"));
    let rate = if is_decimal { pct } else { pct / 100.0 };
    Decimal::try_from(rate).unwrap_or(Decimal::ZERO)
}

fn years_to_maturity(valuation: Date, maturity: Date) -> f64 {
    valuation.days_between(&maturity) as f64 / 365.25
}

/// Matches QL's `Schedule` "snap back to month-end after a short month"
/// behaviour (Oct 31 → Apr 30 → Jul 31 → Oct 31) so both sides agree on
/// coupon dates when the maturity lands on a month-end.
fn is_end_of_month(date: Date) -> bool {
    let Ok(next_day) = Date::from_ymd(date.year(), date.month(), date.day() + 1) else {
        return true;
    };
    next_day.month() != date.month()
}

/// YYYYMMDD so the date diffs as a plain number against the Python side.
fn workout_date_to_f64(d: Date) -> f64 {
    (d.year() * 10000 + d.month() as i32 * 100 + d.day() as i32) as f64
}

fn interpolate_cmt(cmt: &Curve, tenor_yrs: f64) -> Option<f64> {
    let pts: Vec<(f64, f64)> = cmt
        .quotes
        .iter()
        .filter_map(|q| q.rate_pct.map(|r| (q.tenor_years, r / 100.0)))
        .collect();
    if pts.is_empty() {
        return None;
    }
    // Below min → flat at min; above max → flat at max.
    if tenor_yrs <= pts[0].0 {
        return Some(pts[0].1);
    }
    for w in pts.windows(2) {
        let (t0, r0) = w[0];
        let (t1, r1) = w[1];
        if tenor_yrs >= t0 && tenor_yrs <= t1 {
            let w = (tenor_yrs - t0) / (t1 - t0);
            return Some(r0 + w * (r1 - r0));
        }
    }
    Some(pts.last().unwrap().1)
}

/// Same bond re-maturing at `workout_date` with `redemption` as the final
/// principal. Running YTM on this gives YTC at that call date; iterating gives YTW.
fn build_workout_bullet(
    inst: &Instrument,
    workout_date: Date,
    redemption: f64,
) -> Result<FixedRateBond> {
    let coupon = inst
        .coupon_rate
        .ok_or_else(|| anyhow!("{}: missing coupon_rate", inst.id))?;
    let anchor = inst
        .dated_date
        .as_deref()
        .or(inst.issue_date.as_deref())
        .ok_or_else(|| anyhow!("{}: missing dated_date / issue_date", inst.id))?;
    let issue = parse_date(anchor)?;
    let freq = parse_frequency(
        inst.frequency
            .as_deref()
            .ok_or_else(|| anyhow!("{}: missing frequency", inst.id))?,
    )?;
    let dcc_str = inst
        .day_count
        .as_deref()
        .ok_or_else(|| anyhow!("{}: missing day_count", inst.id))?;
    let dcc: DayCountConvention = parse_day_count(dcc_str)
        .map_err(|e| anyhow!("{}: bad day_count {dcc_str}: {e}", inst.id))?;
    let ccy = parse_currency(
        inst.currency
            .as_deref()
            .ok_or_else(|| anyhow!("{}: missing currency", inst.id))?,
    )?;

    FixedRateBond::builder()
        .cusip_unchecked(&format!("{}_to_{}", inst.id, workout_date))
        .coupon_rate(coupon_to_decimal(coupon, inst.coupon_unit.as_deref()))
        .issue_date(issue)
        .maturity(workout_date)
        .frequency(freq)
        .day_count(dcc)
        .currency(ccy)
        .face_value(dec!(100))
        // Match QL's NullCalendar + Unadjusted so coupon dates aren't shifted
        // off weekends. EOM driven by the workout date (new maturity).
        .calendar(CalendarId::new(""))
        .business_day_convention(BusinessDayConvention::Unadjusted)
        .end_of_month(is_end_of_month(workout_date))
        .redemption_value(Decimal::try_from(redemption).unwrap_or(dec!(100)))
        .build()
        .with_context(|| format!("building {} workout bullet", inst.id))
}

/// Build a `CallableBond` from the bench `Instrument`. Mirrors the QL-side
/// `_build_callable_bond` so both pricers see the same call schedule and base
/// fixed-rate bond.
fn build_callable_bond(inst: &Instrument) -> Result<CallableBond> {
    let maturity = parse_date(
        inst.maturity_date
            .as_deref()
            .ok_or_else(|| anyhow!("{}: missing maturity_date", inst.id))?,
    )?;
    let base = build_workout_bullet(inst, maturity, 100.0)?;
    let mut schedule = BondCallSchedule::new(CallType::American);
    if let Some(entries) = &inst.call_schedule {
        for e in entries {
            let d = parse_date(&e.call_date)?;
            schedule = schedule.with_entry(BondCallEntry::new(d, e.price));
        }
    }
    Ok(CallableBond::new(base, schedule))
}

/// HW1F trinomial OAS metrics for one callable. `(a, sigma)` come from a
/// per-snapshot, per-bond swaption-strip calibration produced by the QL
/// side (`reconciliation/hw1f_params_<snapshot>.json`); both pricers run
/// against the same parameters so reconciliation tests pricing parity, not
/// optimizer parity.
fn callable_oas_rows(
    inst: &Instrument,
    valuation: Date,
    sofr_curve: &DiscountCurve,
    a: f64,
    sigma: f64,
) -> Result<Vec<(String, f64)>> {
    const HW_TREE_STEPS: usize = 500;
    const TARGET_CLEAN: f64 = 99.0;

    let bond = build_callable_bond(inst)?;
    let calc = OASCalculator::new(HullWhite::new(a, sigma), HW_TREE_STEPS);

    // OASCalculator returns dirty PV; subtract accrued so emitted prices
    // are clean (matches QL `bond.cleanPrice()`).
    let accrued = bond
        .base_bond()
        .accrued_interest(valuation)
        .to_f64()
        .unwrap_or(0.0);

    let mut rows = Vec::new();

    for bps in [25_i32, 50, 100] {
        let oas = bps as f64 / 10_000.0;
        let dirty = calc
            .price_with_oas(&bond, sofr_curve, oas, valuation)
            .with_context(|| format!("price_with_oas {}bps for {}", bps, inst.id))?;
        rows.push((format!("price_at_oas_{}bps", bps), dirty - accrued));
    }

    let target = Decimal::try_from(TARGET_CLEAN + accrued).unwrap_or(Decimal::ONE_HUNDRED);
    let oas_spread = calc
        .calculate(&bond, target, sofr_curve, valuation)
        .with_context(|| format!("OAS solve for {}", inst.id))?;
    let oas_bps = oas_spread.as_bps().to_f64().unwrap_or(0.0);
    rows.push(("oas_bps_at_market".to_string(), oas_bps));

    let oas_dec = oas_bps / 10_000.0;
    let eff_dur = calc
        .effective_duration(&bond, sofr_curve, oas_dec, valuation)
        .with_context(|| format!("effective_duration for {}", inst.id))?;
    let eff_cvx = calc
        .effective_convexity(&bond, sofr_curve, oas_dec, valuation)
        .with_context(|| format!("effective_convexity for {}", inst.id))?;
    rows.push(("effective_duration_at_oas".to_string(), eff_dur));
    rows.push(("effective_convexity_at_oas".to_string(), eff_cvx));

    Ok(rows)
}

/// Sovereign FRN projection: flat (index + spread). Fixed-rate bonds return
/// their book coupon directly.
fn effective_coupon_percent(inst: &Instrument) -> Result<f64> {
    if inst.category == "sovereign_frn" {
        let idx = inst
            .index_rate_pct
            .ok_or_else(|| anyhow!("{}: FRN missing index_rate_pct", inst.id))?;
        let spread = inst.spread_bps.unwrap_or(0.0);
        Ok(idx + spread / 100.0)
    } else {
        inst.coupon_rate
            .ok_or_else(|| anyhow!("{}: missing coupon_rate", inst.id))
    }
}

// --------------------------------------------------- SOFR curve + FRN pricing

/// Pillars placed on integer days (`round(tenor_years × 365) / 365`) so
/// `DiscreteCurve` lines up with QL's Date-based `ZeroCurve` to 1e-10.
fn build_sofr_curve(curve: &Curve, reference_date: Date) -> Result<DiscountCurve> {
    let mut builder = DiscountCurveBuilder::new(reference_date)
        .with_interpolation(InterpolationMethod::Linear)
        .with_extrapolation();
    for q in &curve.quotes {
        let rate = q
            .rate_pct
            .ok_or_else(|| anyhow!("{}: null rate_pct at {}Y", curve.id, q.tenor_years))?;
        let days = (q.tenor_years * 365.0).round() as i64;
        let t = days as f64 / 365.0;
        builder = builder.add_zero_rate(t, rate / 100.0);
    }
    builder
        .build()
        .with_context(|| format!("building {}", curve.id))
}

fn df(curve: &DiscountCurve, reference: Date, date: Date) -> f64 {
    if date <= reference {
        return 1.0;
    }
    let t = reference.days_between(&date) as f64 / 365.0;
    curve.discount_factor_at_tenor(t).unwrap_or(1.0)
}

struct FrnMetrics {
    clean_price_pct: f64,
    dirty_price_pct: f64,
    accrued: f64,
    discount_margin_bps: f64,
}

/// See SOURCES.md § "SOFR FRN projection convention" for the shared
/// pricing model. Paired with `ql_bench.py::price_corporate_frn`.
///
/// The in-progress period is priced under ARRC compound-in-arrears using
/// `fixings` for already-published business days and the SOFR projection
/// curve for the remainder. Future periods stay on the curve-implied
/// forward (mathematically equivalent to compound-in-arrears under
/// deterministic curves). Spread is additive (post-compounding).
fn price_corporate_frn(
    inst: &Instrument,
    valuation: Date,
    sofr_curve: &DiscountCurve,
    fixings: &OvernightFixings,
) -> Result<FrnMetrics> {
    let spread = inst.spread_bps.unwrap_or(0.0) / 10_000.0;
    let face = 100.0_f64;

    let dated = parse_date(
        inst.dated_date
            .as_deref()
            .or(inst.issue_date.as_deref())
            .ok_or_else(|| anyhow!("{}: missing dated/issue date", inst.id))?,
    )?;
    let maturity = parse_date(
        inst.maturity_date
            .as_deref()
            .ok_or_else(|| anyhow!("{}: missing maturity_date", inst.id))?,
    )?;
    let frn = FloatingRateNote::builder()
        .cusip_unchecked(&inst.id)
        .spread_decimal(Decimal::try_from(spread).unwrap_or(Decimal::ZERO))
        .issue_date(dated)
        .maturity(maturity)
        .corporate_sofr() // ARRC defaults: obs-shift on, lookback=2, lockout=0, NY cal
        .face_value(dec!(100))
        .build()
        .with_context(|| format!("building FRN {}", inst.id))?;

    let arrc = ArrcConfig::usd_corporate_sofr();
    let calendar = CalendarId::us_government();
    let calendar_obj = calendar.to_calendar();
    let dc360 = DayCountConvention::Act360.to_day_count();

    // Daily forward callback: derives an annualized 1-business-day SOFR rate
    // from the projection curve. Only invoked on business days that have no
    // published fixing.
    let curve_daily_forward = |d: Date| -> Decimal {
        let next_bd = calendar_obj.add_business_days(d, 1);
        let df_d = df(sofr_curve, valuation, d);
        let df_n = df(sofr_curve, valuation, next_bd);
        let yf = dc360.year_fraction(d, next_bd).to_f64().unwrap_or(0.0);
        if yf <= 0.0 {
            return Decimal::ZERO;
        }
        let rate = (df_d / df_n - 1.0) / yf;
        Decimal::try_from(rate).unwrap_or(Decimal::ZERO)
    };

    let mut dirty = 0.0;
    let mut spread_annuity = 0.0;
    let mut accrued = 0.0;

    for cf in frn.cash_flows(dated) {
        let (start, end) = match (cf.accrual_start, cf.accrual_end) {
            (Some(s), Some(e)) => (s, e),
            _ => continue,
        };
        if end <= valuation {
            continue;
        }

        let df_e = df(sofr_curve, valuation, end);
        let yf = dc360.year_fraction(start, end).to_f64().unwrap_or(0.0);

        let coupon = if start < valuation {
            // In-progress period: real ARRC compounding (fixings ⨁ curve).
            let comp = compound_in_arrears(
                start,
                end,
                DayCountConvention::Act360,
                calendar_obj.as_ref(),
                arrc,
                fixings,
                &curve_daily_forward,
            );
            let comp_rate_minus_one: f64 =
                comp.compounded_rate_minus_one().try_into().unwrap_or(0.0);
            let coupon_amount = face * (comp_rate_minus_one + spread * yf);

            // Accrued portion: compound through valuation only.
            let accrued_comp = compound_in_arrears(
                start,
                valuation,
                DayCountConvention::Act360,
                calendar_obj.as_ref(),
                arrc,
                fixings,
                &curve_daily_forward,
            );
            let accrued_minus_one: f64 = accrued_comp
                .compounded_rate_minus_one()
                .try_into()
                .unwrap_or(0.0);
            let accrued_yf: f64 = accrued_comp.period_year_fraction.try_into().unwrap_or(0.0);
            accrued = face * (accrued_minus_one + spread * accrued_yf);

            coupon_amount
        } else {
            // Future period: curve-projection equivalent of compound-in-arrears.
            let df_s = df(sofr_curve, valuation, start);
            face * (df_s / df_e - 1.0 + spread * yf)
        };

        let amount = if end == maturity {
            coupon + face
        } else {
            coupon
        };
        dirty += amount * df_e;
        spread_annuity += yf * df_e;
    }

    let clean = dirty - accrued;

    // DM that reprices to clean = 100: first-order inversion of the spread
    // annuity. Exact because the floating leg PV is spread-independent.
    let dm = if spread_annuity.abs() > 1e-12 {
        (dirty - 100.0 - accrued) / (spread_annuity * face)
    } else {
        0.0
    };

    Ok(FrnMetrics {
        clean_price_pct: clean,
        dirty_price_pct: dirty,
        accrued,
        discount_margin_bps: dm * 10_000.0,
    })
}

fn build_callable_frn_base(inst: &Instrument) -> Result<FloatingRateNote> {
    let spread = inst.spread_bps.unwrap_or(0.0) / 10_000.0;
    let dated = parse_date(
        inst.dated_date
            .as_deref()
            .or(inst.issue_date.as_deref())
            .ok_or_else(|| anyhow!("{}: missing dated/issue date", inst.id))?,
    )?;
    let maturity = parse_date(
        inst.maturity_date
            .as_deref()
            .ok_or_else(|| anyhow!("{}: missing maturity_date", inst.id))?,
    )?;
    FloatingRateNote::builder()
        .cusip_unchecked(&inst.id)
        .spread_decimal(Decimal::try_from(spread).unwrap_or(Decimal::ZERO))
        .issue_date(dated)
        .maturity(maturity)
        .corporate_sofr()
        .face_value(dec!(100))
        .build()
        .with_context(|| format!("building callable FRN base {}", inst.id))
}

fn build_callable_frn_schedule(inst: &Instrument) -> Result<BondCallSchedule> {
    let entries = inst
        .call_schedule
        .as_deref()
        .ok_or_else(|| anyhow!("{}: missing call_schedule", inst.id))?;
    let mut sched = BondCallSchedule::new(CallType::Bermudan);
    for e in entries {
        let d = parse_date(&e.call_date)?;
        sched = sched.with_entry(BondCallEntry::new(d, e.price));
    }
    Ok(sched)
}

/// ARRC compound-in-arrears coupon for [start, end) in face units, with the
/// bond's quoted spread folded in.
fn arrc_in_progress_coupon<'a>(
    sofr_curve: &'a DiscountCurve,
    fixings: &'a OvernightFixings,
    valuation: Date,
    spread_decimal: f64,
    face: f64,
) -> impl Fn(Date, Date) -> f64 + 'a {
    let arrc = ArrcConfig::usd_corporate_sofr();
    let calendar = CalendarId::us_government().to_calendar();
    let dc360 = DayCountConvention::Act360.to_day_count();

    move |start, end| {
        let cal = calendar.as_ref();
        let curve_daily_forward = |d: Date| -> Decimal {
            let next_bd = cal.add_business_days(d, 1);
            let df_d = df(sofr_curve, valuation, d);
            let df_n = df(sofr_curve, valuation, next_bd);
            let yf = dc360.year_fraction(d, next_bd).to_f64().unwrap_or(0.0);
            if yf <= 0.0 {
                return Decimal::ZERO;
            }
            Decimal::try_from((df_d / df_n - 1.0) / yf).unwrap_or(Decimal::ZERO)
        };
        let comp = compound_in_arrears(
            start,
            end,
            DayCountConvention::Act360,
            cal,
            arrc,
            fixings,
            &curve_daily_forward,
        );
        let r_minus_1: f64 = comp.compounded_rate_minus_one().try_into().unwrap_or(0.0);
        let yf = dc360.year_fraction(start, end).to_f64().unwrap_or(0.0);
        face * (r_minus_1 + spread_decimal * yf)
    }
}

fn parse_compounding(s: &str) -> Result<ZcbCompounding> {
    match s.to_ascii_lowercase().replace('-', "_").as_str() {
        "annual" => Ok(ZcbCompounding::Annual),
        "semi_annual" | "semi" | "semiannual" => Ok(ZcbCompounding::SemiAnnual),
        "quarterly" => Ok(ZcbCompounding::Quarterly),
        "monthly" => Ok(ZcbCompounding::Monthly),
        "continuous" => Ok(ZcbCompounding::Continuous),
        other => Err(anyhow!("unsupported compounding {other}")),
    }
}

/// Builds a ZeroCouponBond from a sovereign_strip book entry.
fn build_zero(inst: &Instrument) -> Result<ZeroCouponBond> {
    let maturity = parse_date(
        inst.maturity_date
            .as_deref()
            .ok_or_else(|| anyhow!("{}: missing maturity_date", inst.id))?,
    )?;
    let issue = parse_date(
        inst.dated_date
            .as_deref()
            .or(inst.issue_date.as_deref())
            .ok_or_else(|| anyhow!("{}: missing dated_date / issue_date", inst.id))?,
    )?;
    let dcc_str = inst
        .day_count
        .as_deref()
        .ok_or_else(|| anyhow!("{}: missing day_count", inst.id))?;
    let dcc: DayCountConvention = parse_day_count(dcc_str)
        .map_err(|e| anyhow!("{}: bad day_count {dcc_str}: {e}", inst.id))?;
    let ccy = parse_currency(
        inst.currency
            .as_deref()
            .ok_or_else(|| anyhow!("{}: missing currency", inst.id))?,
    )?;
    let cmp = match inst.compounding.as_deref() {
        Some(s) => parse_compounding(s)?,
        None => ZcbCompounding::SemiAnnual,
    };

    ZeroCouponBond::builder()
        .cusip_unchecked(&inst.id)
        .maturity(maturity)
        .issue_date(issue)
        .day_count(dcc)
        .compounding(cmp)
        .currency(ccy)
        .face_value(dec!(100))
        .build()
        .with_context(|| format!("building zero {}", inst.id))
}

/// Build a put-only CallableBond. `CallableBond` requires a call schedule,
/// so an empty `American` one is passed — fragile. See NEXT_STEPS.md.
/// Standard 8 metrics from yield: clean/dirty/accrued, YTM, mac/mod dur,
/// convexity, DV01. Used by every shape that prices off `&dyn Bond`.
fn standard_metrics(
    bond: &dyn Bond,
    valuation: Date,
    ref_yield: f64,
    freq: Frequency,
) -> Result<Vec<(String, f64)>> {
    let clean = clean_price_from_yield(bond, valuation, ref_yield, freq)?;
    let dirty = dirty_price_from_yield(bond, valuation, ref_yield, freq)?;
    let accrued = dirty - clean;
    let clean_dec = Decimal::from_f64_retain(clean).unwrap_or(Decimal::ONE_HUNDRED);
    let ytm = yield_to_maturity(bond, valuation, clean_dec, freq)?.yield_value;
    let mac = macaulay_duration(bond, valuation, ref_yield, freq)?;
    let modd = modified_duration(bond, valuation, ref_yield, freq)?;
    let cvx = convexity(bond, valuation, ref_yield, freq)?;
    let dv01_v = dv01(bond, valuation, ref_yield, dirty, freq)?;
    Ok(vec![
        ("clean_price_pct".to_string(), clean),
        ("dirty_price_pct".to_string(), dirty),
        ("accrued".to_string(), accrued),
        ("ytm_decimal".to_string(), ytm),
        ("macaulay_duration".to_string(), mac),
        ("modified_duration".to_string(), modd),
        ("convexity".to_string(), cvx),
        ("dv01_per_100".to_string(), dv01_v),
    ])
}

fn build_puttable(inst: &Instrument) -> Result<CallableBond> {
    let maturity = parse_date(
        inst.maturity_date
            .as_deref()
            .ok_or_else(|| anyhow!("{}: missing maturity_date", inst.id))?,
    )?;
    let base = build_workout_bullet(inst, maturity, 100.0)?;
    let entries = inst
        .put_schedule
        .as_deref()
        .ok_or_else(|| anyhow!("{}: puttable missing put_schedule", inst.id))?;
    let mut put_sched = BondPutSchedule::new(PutType::Bermudan);
    for e in entries {
        let d = parse_date(&e.put_date)?;
        put_sched = put_sched.with_entry(BondPutEntry::new(d, e.price));
    }
    Ok(CallableBond::new_putable(base).with_put_schedule(put_sched))
}

/// Bullet bond + a MakeWhole call schedule carrying the MW spread.
fn build_mw_callable(inst: &Instrument, spread_bps: f64) -> Result<CallableBond> {
    let maturity = parse_date(
        inst.maturity_date
            .as_deref()
            .ok_or_else(|| anyhow!("{}: missing maturity_date", inst.id))?,
    )?;
    let base = build_workout_bullet(inst, maturity, 100.0)?;
    let schedule = BondCallSchedule::make_whole(spread_bps);
    Ok(CallableBond::new(base, schedule))
}

/// Builds a SinkingFundBond from a synthetic_sinker book entry.
fn build_sinker(inst: &Instrument) -> Result<SinkingFundBond> {
    let base = build_bond(inst)?;
    let mut schedule = SinkingFundSchedule::new();
    let entries = inst
        .sinking_schedule
        .as_deref()
        .ok_or_else(|| anyhow!("{}: sinker missing sinking_schedule", inst.id))?;
    for e in entries {
        let d = parse_date(&e.date)?;
        schedule = schedule.with_payment(SinkingFundPayment::with_price(d, e.amount_pct, e.price));
    }
    Ok(SinkingFundBond::new(base, schedule))
}

fn build_bond(inst: &Instrument) -> Result<FixedRateBond> {
    let coupon = effective_coupon_percent(inst)?;
    let maturity = parse_date(
        inst.maturity_date
            .as_deref()
            .ok_or_else(|| anyhow!("{}: missing maturity_date", inst.id))?,
    )?;
    // `dated_date` wins; `issue_date` is the fallback (both libraries must agree).
    let anchor = inst
        .dated_date
        .as_deref()
        .or(inst.issue_date.as_deref())
        .ok_or_else(|| anyhow!("{}: missing dated_date / issue_date", inst.id))?;
    let issue = parse_date(anchor)?;
    let freq = parse_frequency(
        inst.frequency
            .as_deref()
            .ok_or_else(|| anyhow!("{}: missing frequency", inst.id))?,
    )?;
    let dcc_str = inst
        .day_count
        .as_deref()
        .ok_or_else(|| anyhow!("{}: missing day_count", inst.id))?;
    let dcc: DayCountConvention = parse_day_count(dcc_str)
        .map_err(|e| anyhow!("{}: bad day_count {dcc_str}: {e}", inst.id))?;
    let ccy = parse_currency(
        inst.currency
            .as_deref()
            .ok_or_else(|| anyhow!("{}: missing currency", inst.id))?,
    )?;

    FixedRateBond::builder()
        .cusip_unchecked(&inst.id)
        .coupon_rate(coupon_to_decimal(coupon, inst.coupon_unit.as_deref()))
        .issue_date(issue)
        .maturity(maturity)
        .frequency(freq)
        .day_count(dcc)
        .currency(ccy)
        .face_value(dec!(100))
        // NullCalendar + Unadjusted so coupon dates don't shift off weekends
        // (QL's Schedule reproduces the same dates). EOM flag from the
        // maturity so month-end maturities snap back after short months.
        .calendar(CalendarId::new(""))
        .business_day_convention(BusinessDayConvention::Unadjusted)
        .end_of_month(is_end_of_month(maturity))
        .build()
        .with_context(|| format!("building {}", inst.id))
}

fn reference_yield<'a>(
    inst: &Instrument,
    maturity: Date,
    valuation: Date,
    curves: &'a [Curve],
) -> (f64, &'a str) {
    let ccy = inst.currency.as_deref().unwrap_or("USD");
    let yrs = years_to_maturity(valuation, maturity);

    if inst.category == "sovereign_linker" {
        return (0.0185, "tips_real_placeholder");
    }
    if inst.category == "sovereign_frn" {
        if let (Some(idx), spread) = (inst.index_rate_pct, inst.spread_bps.unwrap_or(0.0)) {
            return ((idx + spread / 100.0) / 100.0, "frn_flat_projection");
        }
    }

    let curve_id = match ccy {
        "USD" => "UST_CMT",
        "GBP" => "UK_GILT_CURVE",
        "EUR" => "DE_BUND_CURVE",
        "JPY" => "JP_JGB_CURVE",
        other => unreachable!("unexpected currency {other} on {}", inst.id),
    };
    if let Some(curve) = curves.iter().find(|c| c.id == curve_id) {
        if let Some(y) = interpolate_cmt(curve, yrs) {
            return (y, curve.id.as_str());
        }
    }
    let fallback = inst.coupon_rate.map(|c| c / 100.0).unwrap_or(0.04);
    (fallback, "placeholder")
}

// ------------------------------------------------------------------ main

/// Snapshot definition: which book + curves to load, and where to write
/// the Convex output. The default 2025-12-31 snapshot covers the full
/// mixed book; additional snapshots can carry a focused subset (see e.g.
/// `book_20250630.json` for the FRN-only mid-period snapshot from Tier
/// 2.3.2).
struct Snapshot<'a> {
    book: &'a str,
    curves: &'a str,
    out_csv: &'a str,
    require_ust_cmt: bool,
    /// QL-emitted HW1F calibration file. `None` ⇒ snapshot has no callables.
    hw1f_params: Option<&'a str>,
}

const SNAPSHOTS: &[Snapshot<'static>] = &[
    Snapshot {
        book: "book.json",
        curves: "curves.json",
        out_csv: "convex.csv",
        require_ust_cmt: true,
        hw1f_params: Some("hw1f_params_20251231.json"),
    },
    Snapshot {
        book: "book_20250630.json",
        curves: "curves_20250630.json",
        out_csv: "convex_20250630.csv",
        require_ust_cmt: false,
        hw1f_params: None,
    },
];

fn main() -> Result<()> {
    let root = Path::new("reconciliation");
    for snap in SNAPSHOTS {
        run_snapshot(root, snap).with_context(|| format!("snapshot {}", snap.book))?;
    }
    Ok(())
}

fn run_snapshot(root: &Path, snap: &Snapshot<'_>) -> Result<()> {
    let book: Book = serde_json::from_reader(File::open(root.join(snap.book))?)
        .with_context(|| format!("reading {}", snap.book))?;
    let curves: Curves = serde_json::from_reader(File::open(root.join(snap.curves))?)
        .with_context(|| format!("reading {}", snap.curves))?;
    if snap.require_ust_cmt && !curves.curves.iter().any(|c| c.id == "UST_CMT") {
        return Err(anyhow!("UST_CMT curve not found in {}", snap.curves));
    }

    let valuation = parse_date(&book.valuation_date)?;

    let sofr_curve = curves
        .curves
        .iter()
        .find(|c| c.id == "SOFR_OIS_CURVE")
        .map(|c| build_sofr_curve(c, valuation))
        .transpose()?;

    let sofr_fixings = {
        let path = root.join("sofr_fixings.csv");
        let raw = if path.exists() {
            OvernightFixings::from_csv(&path)
                .with_context(|| format!("loading {}", path.display()))?
        } else {
            OvernightFixings::new()
        };
        raw.with_as_of(valuation)
    };

    let mut tips_ratios: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    let ratio_path = root.join("tips_index_ratio_20251231.json");
    if ratio_path.exists() {
        if let Ok(r) = serde_json::from_reader::<_, TipsIndexRatio>(File::open(&ratio_path)?) {
            tips_ratios.insert(r.cusip, r.index_ratio);
        }
    }

    let hw1f_calibrations: std::collections::HashMap<String, Hw1fParams> = match snap.hw1f_params {
        Some(name) => {
            let path = root.join(name);
            let raw: Hw1fCalibrations = serde_json::from_reader(File::open(&path)?)
                .with_context(|| format!("reading {}", path.display()))?;
            raw.calibrations
        }
        None => std::collections::HashMap::new(),
    };

    let out_path = root.join(snap.out_csv);
    let mut out = BufWriter::new(File::create(&out_path)?);
    writeln!(
        out,
        "bond_id,currency,metric,value,reference_yield,curve_used,notes"
    )?;

    let mut ok_count = 0usize;
    let mut skipped: Vec<String> = Vec::new();

    for inst in &book.instruments {
        let is_callable = matches!(
            inst.category.as_str(),
            "corporate_callable" | "synthetic_callable"
        );
        let skip_reason = match inst.category.as_str() {
            "sovereign" => None,
            "sovereign_strip" => None, // Pure zero-coupon, ZeroCouponBond path
            "synthetic_sinker" => None, // SinkingFundBond / ql.AmortizingFixedRateBond
            "synthetic_puttable" => None, // CallableBond + put schedule, YTB workout-bullet
            "corporate_bullet_mw" => None,
            "corporate_callable" => None,
            "synthetic_callable" => None,
            "sovereign_linker" => None, // TIPS priced on real yield
            "sovereign_frn" => None,    // FRN: flat-forward projection
            "corporate_frn" => None,    // SOFR FRN: curve-projected, handled below
            "synthetic_callable_frn" => None, // CallableFloatingRateNote: workout-bullet DM
            _other => Some("unknown category"),
        };
        if let Some(reason) = skip_reason {
            skipped.push(format!("{} ({}) — {}", inst.id, inst.category, reason));
            continue;
        }

        if inst.category == "sovereign_strip" {
            let bond = build_zero(inst)?;
            let maturity = parse_date(inst.maturity_date.as_deref().unwrap())?;
            let (ref_yield, curve_used) =
                reference_yield(inst, maturity, valuation, &curves.curves);
            let freq = parse_frequency(inst.frequency.as_deref().unwrap())?;
            for (metric, value) in standard_metrics(&bond, valuation, ref_yield, freq)
                .with_context(|| format!("standard_metrics {}", inst.id))?
            {
                writeln!(
                    out,
                    "{},{},{},{:.10},{:.10},{},",
                    inst.id,
                    inst.currency.as_deref().unwrap_or("?"),
                    metric,
                    value,
                    ref_yield,
                    curve_used,
                )?;
            }
            ok_count += 1;
            continue;
        }

        if inst.category == "synthetic_puttable" {
            let bond = build_puttable(inst)?;
            let maturity = parse_date(inst.maturity_date.as_deref().unwrap())?;
            let (ref_yield, curve_used) =
                reference_yield(inst, maturity, valuation, &curves.curves);
            let freq = parse_frequency(inst.frequency.as_deref().unwrap())?;

            let base = bond.base_bond();
            let mut put_rows = standard_metrics(base, valuation, ref_yield, freq)
                .with_context(|| format!("standard_metrics {}", inst.id))?;

            let clean = put_rows[0].1; // clean_price_pct
            let ytm = put_rows[3].1;   // ytm_decimal
            let clean_dec = Decimal::from_f64_retain(clean).unwrap_or(Decimal::ONE_HUNDRED);

            // YTB = best for holder = max yield over YTM and all YTPs.
            let mut best_yield = ytm;
            let mut best_date = maturity;
            if let Some(entries) = inst.put_schedule.as_deref() {
                for e in entries {
                    let put_date = parse_date(&e.put_date)?;
                    if put_date <= valuation {
                        continue;
                    }
                    let workout_bullet = build_workout_bullet(inst, put_date, e.price)?;
                    let ytp = yield_to_maturity(&workout_bullet, valuation, clean_dec, freq)
                        .with_context(|| format!("YTP {} @ {}", inst.id, e.put_date))?
                        .yield_value;
                    put_rows.push((format!("ytp_{}_decimal", e.put_date.replace('-', "")), ytp));
                    if ytp > best_yield {
                        best_yield = ytp;
                        best_date = put_date;
                    }
                }
            }
            put_rows.push(("ytb_decimal".to_string(), best_yield));
            put_rows.push((
                "ytb_workout_date_yyyymmdd".to_string(),
                workout_date_to_f64(best_date),
            ));

            for (metric, value) in put_rows {
                writeln!(
                    out,
                    "{},{},{},{:.10},{:.10},{},",
                    inst.id,
                    inst.currency.as_deref().unwrap_or("?"),
                    metric,
                    value,
                    ref_yield,
                    curve_used,
                )?;
            }
            ok_count += 1;
            continue;
        }

        if inst.category == "synthetic_sinker" {
            let bond = build_sinker(inst)?;
            let maturity = parse_date(inst.maturity_date.as_deref().unwrap())?;
            let (ref_yield, curve_used) =
                reference_yield(inst, maturity, valuation, &curves.curves);
            let freq = parse_frequency(inst.frequency.as_deref().unwrap())?;
            for (metric, value) in standard_metrics(&bond, valuation, ref_yield, freq)
                .with_context(|| format!("standard_metrics {}", inst.id))?
            {
                writeln!(
                    out,
                    "{},{},{},{:.10},{:.10},{},",
                    inst.id,
                    inst.currency.as_deref().unwrap_or("?"),
                    metric,
                    value,
                    ref_yield,
                    curve_used,
                )?;
            }
            ok_count += 1;
            continue;
        }

        // Corporate SOFR FRN: separate pricing path using the SOFR OIS curve.
        if inst.category == "corporate_frn" {
            let curve = sofr_curve.as_ref().ok_or_else(|| {
                anyhow!(
                    "{}: SOFR_OIS_CURVE required for corporate_frn pricing",
                    inst.id
                )
            })?;
            let m = price_corporate_frn(inst, valuation, curve, &sofr_fixings)
                .with_context(|| format!("price_corporate_frn {}", inst.id))?;
            let frn_rows: [(&str, f64); 4] = [
                ("clean_price_pct", m.clean_price_pct),
                ("dirty_price_pct", m.dirty_price_pct),
                ("accrued", m.accrued),
                ("discount_margin_bps", m.discount_margin_bps),
            ];
            let spread_dec = inst.spread_bps.unwrap_or(0.0) / 10_000.0;
            for (metric, value) in frn_rows {
                writeln!(
                    out,
                    "{},{},{},{:.10},{:.10},SOFR_OIS_CURVE,",
                    inst.id,
                    inst.currency.as_deref().unwrap_or("?"),
                    metric,
                    value,
                    spread_dec,
                )?;
            }
            ok_count += 1;
            continue;
        }

        // Per-call DM uses scenario_dirty=100; using the calculated dirty
        // would collapse all branches to ~0bps and lose the workout signal.
        if inst.category == "synthetic_callable_frn" {
            let curve = sofr_curve.as_ref().ok_or_else(|| {
                anyhow!("{}: SOFR_OIS_CURVE required", inst.id)
            })?;
            let m = price_corporate_frn(inst, valuation, curve, &sofr_fixings)
                .with_context(|| format!("price_corporate_frn {}", inst.id))?;
            let spread_dec = inst.spread_bps.unwrap_or(0.0) / 10_000.0;
            let maturity = parse_date(inst.maturity_date.as_deref().unwrap())?;

            let mut rows: Vec<(String, f64)> = vec![
                ("clean_price_pct".to_string(), m.clean_price_pct),
                ("dirty_price_pct".to_string(), m.dirty_price_pct),
                ("accrued".to_string(), m.accrued),
                ("discount_margin_bps".to_string(), m.discount_margin_bps),
            ];

            let frn = build_callable_frn_base(inst)?;
            let cfrn = CallableFloatingRateNote::new(frn.clone(), build_callable_frn_schedule(inst)?);

            let forward = convex_curves::curves::ForwardCurve::from_months(
                std::sync::Arc::new(curve.clone()) as std::sync::Arc<dyn convex_curves::RateCurveDyn>,
                3,
            );
            let arrc_coupon = arrc_in_progress_coupon(curve, &sofr_fixings, valuation, spread_dec, 100.0);
            let calc = DiscountMarginCalculator::new(&forward, curve)
                .with_in_progress_coupon(arrc_coupon);

            // Seed at dirty=100 (same as per-call branches); m.discount_margin_bps
            // targets dirty=100+accrued, which would mix scenarios.
            let dm_mat = calc
                .calculate_to_workout(&frn, dec!(100.0), valuation, maturity, 100.0)
                .with_context(|| format!("DM-to-maturity {}", inst.id))?;
            let mut worst_bps = dm_mat.as_bps().to_f64().unwrap_or(0.0);
            let mut worst_date = maturity;
            for call_date in cfrn.all_workout_dates(valuation) {
                let call_price = cfrn
                    .call_price_on(call_date)
                    .ok_or_else(|| anyhow!("{}: no call price for {}", inst.id, call_date))?;
                let dm = calc
                    .calculate_to_workout(&frn, dec!(100.0), valuation, call_date, call_price)
                    .with_context(|| format!("DM-to-call {} @ {}", inst.id, call_date))?;
                let dm_bps = dm.as_bps().to_f64().unwrap_or(0.0);
                let key = format!(
                    "dm_to_call_{:04}{:02}{:02}_bps",
                    call_date.year(),
                    call_date.month(),
                    call_date.day(),
                );
                rows.push((key, dm_bps));
                if dm_bps < worst_bps {
                    worst_bps = dm_bps;
                    worst_date = call_date;
                }
            }
            rows.push(("dm_to_worst_bps".to_string(), worst_bps));
            rows.push((
                "dm_to_worst_workout_yyyymmdd".to_string(),
                workout_date_to_f64(worst_date),
            ));

            for (metric, value) in rows {
                writeln!(
                    out,
                    "{},{},{},{:.10},{:.10},SOFR_OIS_CURVE,",
                    inst.id,
                    inst.currency.as_deref().unwrap_or("?"),
                    metric,
                    value,
                    spread_dec,
                )?;
            }

            ok_count += 1;
            continue;
        }

        let bond = build_bond(inst).with_context(|| format!("build {}", inst.id))?;
        let maturity = parse_date(inst.maturity_date.as_deref().unwrap())?;
        let (ref_yield, curve_used) = reference_yield(inst, maturity, valuation, &curves.curves);
        let freq = parse_frequency(inst.frequency.as_deref().unwrap())?;

        // 1) Clean and dirty price at the reference yield.
        let clean = clean_price_from_yield(&bond, valuation, ref_yield, freq)
            .with_context(|| format!("clean_price_from_yield {}", inst.id))?;
        let dirty = dirty_price_from_yield(&bond, valuation, ref_yield, freq)
            .with_context(|| format!("dirty_price_from_yield {}", inst.id))?;
        let accrued = dirty - clean;

        // 2) Round-trip: YTM from the clean price should recover ref_yield.
        let clean_dec = Decimal::from_f64_retain(clean).unwrap_or(Decimal::ONE_HUNDRED);
        let ytm_result = yield_to_maturity(&bond, valuation, clean_dec, freq)
            .with_context(|| format!("yield_to_maturity {}", inst.id))?;
        let ytm = ytm_result.yield_value;

        // 3) Risk at the reference yield.
        let mac = macaulay_duration(&bond, valuation, ref_yield, freq)?;
        let modd = modified_duration(&bond, valuation, ref_yield, freq)?;
        let cvx = convexity(&bond, valuation, ref_yield, freq)?;
        let dv01_v = dv01(&bond, valuation, ref_yield, dirty, freq)?;

        let mut rows: Vec<(String, f64)> = vec![
            ("clean_price_pct".to_string(), clean),
            ("dirty_price_pct".to_string(), dirty),
            ("accrued".to_string(), accrued),
            ("ytm_decimal".to_string(), ytm),
            ("macaulay_duration".to_string(), mac),
            ("modified_duration".to_string(), modd),
            ("convexity".to_string(), cvx),
            ("dv01_per_100".to_string(), dv01_v),
        ];

        // 4a) For linkers: emit nominal (= real × CPI index ratio) metrics
        // when the index ratio is available.
        if inst.category == "sovereign_linker" {
            let cusip = inst.identifier.as_ref().and_then(|i| i.value.as_deref());
            if let Some(ratio) = cusip.and_then(|c| tips_ratios.get(c)) {
                rows.push(("cpi_index_ratio".to_string(), *ratio));
                rows.push(("nominal_clean_price_pct".to_string(), clean * ratio));
                rows.push(("nominal_dirty_price_pct".to_string(), dirty * ratio));
                rows.push(("nominal_accrued".to_string(), accrued * ratio));
            }
        }

        // 4b) For callables: compute YTC at each call date and YTW.
        if is_callable {
            let clean_dec = Decimal::from_f64_retain(clean).unwrap_or(Decimal::ONE_HUNDRED);
            let mut worst = ytm;
            let mut worst_date = maturity;

            if let Some(schedule) = &inst.call_schedule {
                for entry in schedule {
                    let call_date = parse_date(&entry.call_date)?;
                    if call_date <= valuation {
                        continue; // call date already passed
                    }
                    let workout_bullet = build_workout_bullet(inst, call_date, entry.price)?;
                    // Build a synthetic "clean price" that treats the workout bullet as
                    // having the same current clean market price as the actual callable.
                    let ytc_result = yield_to_maturity(&workout_bullet, valuation, clean_dec, freq)
                        .with_context(|| {
                            format!("yield_to_call {} @ {}", inst.id, entry.call_date)
                        })?;
                    let ytc = ytc_result.yield_value;
                    let key = format!("ytc_{}_decimal", entry.call_date.replace('-', ""));
                    rows.push((key, ytc));
                    if ytc < worst {
                        worst = ytc;
                        worst_date = call_date;
                    }
                }
            }
            rows.push(("ytw_decimal".to_string(), worst));
            rows.push((
                "ytw_workout_date_yyyymmdd".to_string(),
                workout_date_to_f64(worst_date),
            ));

            // HW1F OAS metrics + native Rust σ calibration (5.2.2 / 5.2.4).
            // The QL-emitted JSON gives us both the per-bond (a, σ) used by
            // the OAS pricer and the swaption strip used to fit them, so the
            // Rust calibrator runs against the same helpers QL did.
            if let (Some(curve), Some(params)) =
                (sofr_curve.as_ref(), hw1f_calibrations.get(&inst.id))
            {
                let oas_rows = callable_oas_rows(inst, valuation, curve, params.a, params.sigma)
                    .with_context(|| format!("callable_oas_rows {}", inst.id))?;
                rows.extend(oas_rows);

                let helpers: Vec<CoterminalSwaptionHelper> = params
                    .helpers
                    .iter()
                    .map(|h| CoterminalSwaptionHelper {
                        expiry_years: h.expiry_yrs,
                        tail_years: h.tail_yrs,
                        atm_normal_vol_bps: h.atm_normal_vol_bps,
                    })
                    .collect();
                let cal = calibrate_hw1f_sigma(curve, params.a, &helpers)
                    .with_context(|| format!("calibrate_hw1f_sigma {}", inst.id))?;
                rows.push(("hw1f_a_calibrated".to_string(), cal.a));
                rows.push(("hw1f_sigma_calibrated".to_string(), cal.sigma));
            }
        }

        // Make-whole redemption: one ITM scenario (UST 3% → MW well above par)
        // and one near-ATM (UST 5% ≈ coupon → MW close to par).
        if let Some(spread_bps) = inst.make_whole.as_ref().and_then(|mw| mw.spread_bps) {
            let mw_bond = build_mw_callable(inst, spread_bps)?;
            for (metric, call_date_str, treasury_rate) in [
                ("mw_redemption_call_2026_06_15_ust_300bps", "2026-06-15", 0.030),
                ("mw_redemption_call_2026_06_15_ust_500bps", "2026-06-15", 0.050),
            ] {
                let call_date = parse_date(call_date_str)?;
                let mw = mw_bond
                    .make_whole_call_price(call_date, treasury_rate)
                    .with_context(|| format!("MW price {} @ {}", inst.id, call_date_str))?
                    .to_f64()
                    .unwrap_or(0.0);
                rows.push((metric.to_string(), mw));
            }
        }

        for (metric, value) in rows {
            writeln!(
                out,
                "{},{},{},{:.10},{:.10},{},",
                inst.id,
                inst.currency.as_deref().unwrap_or("?"),
                metric,
                value,
                ref_yield,
                curve_used,
            )?;
        }
        ok_count += 1;
    }

    out.flush()?;
    eprintln!(
        "convex_bench: wrote {} — {} bonds priced",
        out_path.display(),
        ok_count
    );
    if !skipped.is_empty() {
        eprintln!("convex_bench: skipped:");
        for s in &skipped {
            eprintln!("  - {s}");
        }
    }
    Ok(())
}
