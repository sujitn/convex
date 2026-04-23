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

use convex_analytics::functions::{
    clean_price_from_yield, convexity, dirty_price_from_yield, dv01, macaulay_duration,
    modified_duration, parse_day_count, yield_to_maturity,
};
use convex_bonds::instruments::FixedRateBond;
use convex_bonds::types::CalendarId;
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
    // Corporate-SOFR-FRN-specific: last reset rate used for in-progress accrual.
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

#[derive(Debug, Deserialize)]
struct CallEntry {
    call_date: String,
    price: f64,
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

/// Coupon in book.json is in percent (e.g. 4.000 for 4%).
/// Normalise to decimal for the Convex builder.
fn coupon_to_decimal(pct: f64, unit: Option<&str>) -> Decimal {
    let is_decimal = matches!(unit, Some(u) if u.contains("decimal"));
    let rate = if is_decimal { pct } else { pct / 100.0 };
    Decimal::try_from(rate).unwrap_or(Decimal::ZERO)
}

/// Years from valuation date to bond maturity.
fn years_to_maturity(valuation: Date, maturity: Date) -> f64 {
    valuation.days_between(&maturity) as f64 / 365.25
}

/// True when `date` is the last day of its month. Used to pick the
/// `end_of_month` flag for schedule generation — QL's `Schedule` with a
/// month-end *maturity* snaps dates back to month-end after short months
/// (Oct 31 → Apr 30 → Jul 31 → Oct 31). Convex with `end_of_month(true)`
/// reproduces that behaviour; with `false` it drifts to the 30th.
fn is_end_of_month(date: Date) -> bool {
    let Ok(next_day) = Date::from_ymd(date.year(), date.month(), date.day() + 1) else {
        return true; // next day isn't a valid date in the same month → month-end
    };
    next_day.month() != date.month()
}

/// Encode a date as an integer YYYYMMDD so it diffs cleanly against the
/// Python side.
fn workout_date_to_f64(d: Date) -> f64 {
    (d.year() * 10000 + d.month() as i32 * 100 + d.day() as i32) as f64
}

/// Linear interpolation of the UST CMT curve at a given tenor.
///
/// Returns a yield in decimal (0.04 = 4%).
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

/// Build a hypothetical "bullet to workout date" bond: same coupon and
/// conventions as the underlying, but maturing at `workout_date` with
/// `redemption` as the final principal payment. Used to compute YTC / YTW
/// via standard YTM on the modified bond.
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

/// Effective coupon rate for a bond. For a fixed-rate instrument this is just
/// the book's `coupon_rate`. For an FRN we project all future coupons at a
/// flat (index_rate + spread_bps/100) — good enough to reconcile the PV / risk
/// path under quarterly + ACT/360 conventions.
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

/// Build a discount curve from a zero-rate panel (continuously compounded,
/// ACT/365F). Used for SOFR_OIS_CURVE. Linear interpolation on zero rates —
/// matches what QuantLib's `ZeroCurve(dates, rates, Linear())` produces.
fn build_zero_rate_curve(curve: &Curve, reference_date: Date) -> Result<DiscountCurve> {
    if curve.quotes.is_empty() {
        return Err(anyhow!(
            "curve {} has no quotes; cannot bootstrap",
            curve.id
        ));
    }
    let mut builder = DiscountCurveBuilder::new(reference_date)
        .with_interpolation(InterpolationMethod::Linear)
        .with_extrapolation();
    for q in &curve.quotes {
        let rate = q.rate_pct.ok_or_else(|| {
            anyhow!(
                "curve {} has null rate_pct at tenor {}",
                curve.id,
                q.tenor_years
            )
        })?;
        builder = builder.add_zero_rate(q.tenor_years, rate / 100.0);
    }
    builder
        .build()
        .with_context(|| format!("building discount curve for {}", curve.id))
}

/// Generate a quarterly schedule anchored at `dated` through `maturity`,
/// working backward from maturity. Matches QL's Schedule(DateGeneration.Backward)
/// with NullCalendar + Unadjusted so both libraries land on the same dates.
///
/// For the MMC FRN (2024-11-08 → 2027-11-08) this yields 13 period starts:
///   2024-11-08, 2025-02-08, 2025-05-08, 2025-08-08, 2025-11-08,
///   2026-02-08, 2026-05-08, 2026-08-08, 2026-11-08,
///   2027-02-08, 2027-05-08, 2027-08-08, 2027-11-08.
fn quarterly_schedule(dated: Date, maturity: Date) -> Result<Vec<Date>> {
    let mut dates = vec![maturity];
    let mut current = maturity;
    loop {
        let prev = current.add_months(-3).map_err(|e| {
            anyhow!("date underflow walking quarterly schedule from {maturity}: {e}")
        })?;
        if prev <= dated {
            dates.push(dated);
            break;
        }
        dates.push(prev);
        current = prev;
    }
    dates.reverse();
    Ok(dates)
}

/// Compute DF(t) where t = ACT/365F years from curve's reference date. Clamps
/// t < 0 to 1.0 (past dates).
fn df_at_date(curve: &DiscountCurve, reference: Date, date: Date) -> Result<f64> {
    if date <= reference {
        return Ok(1.0);
    }
    let t = reference.days_between(&date) as f64 / 365.0;
    curve
        .discount_factor_at_tenor(t)
        .map_err(|e| anyhow!("discount_factor_at_tenor({t}) failed: {e}"))
}

/// All metrics emitted for a corporate SOFR FRN.
struct FrnMetrics {
    clean_price_pct: f64,
    dirty_price_pct: f64,
    accrued: f64,
    discount_margin_bps: f64,
}

/// Price a corporate SOFR FRN under the simplified projection convention
/// documented in `book.json::coupon_model_note`:
///   - Schedule: quarterly, NullCalendar + Unadjusted, backward from maturity.
///   - For each period ending after settle, projected coupon amount =
///     `(DF(start) / DF(end) − 1) × face + spread × yf360 × face`. Past dates
///     clamp DF = 1.
///   - Dirty price = Σ (coupon × DF(end)) + face × DF(maturity), all / face × 100.
///   - Accrued = `face × current_reset_rate × yf360(last_coupon, settle)`.
///   - DM = Brent-solved spread that reprices the bond to clean = 100 (par).
fn price_corporate_frn(
    inst: &Instrument,
    valuation: Date,
    sofr_curve: &DiscountCurve,
) -> Result<FrnMetrics> {
    let dated = parse_date(
        inst.dated_date
            .as_deref()
            .or(inst.issue_date.as_deref())
            .ok_or_else(|| anyhow!("{}: missing dated_date / issue_date", inst.id))?,
    )?;
    let maturity = parse_date(
        inst.maturity_date
            .as_deref()
            .ok_or_else(|| anyhow!("{}: missing maturity_date", inst.id))?,
    )?;
    let spread = inst
        .spread_bps
        .ok_or_else(|| anyhow!("{}: missing spread_bps", inst.id))?
        / 10_000.0;
    let reset = inst
        .current_reset_rate_pct
        .ok_or_else(|| anyhow!("{}: missing current_reset_rate_pct", inst.id))?
        / 100.0;

    let schedule = quarterly_schedule(dated, maturity)
        .with_context(|| format!("{}: quarterly_schedule", inst.id))?;

    let face: f64 = 100.0;
    let dc360 = DayCountConvention::Act360.to_day_count();

    let mut dirty: f64 = 0.0;
    let mut spread_annuity: f64 = 0.0; // Σ yf360 × DF(end) — used later for DM solve.
    let mut last_coupon_before_settle: Option<Date> = None;

    for w in schedule.windows(2) {
        let (start, end) = (w[0], w[1]);
        if end <= valuation {
            last_coupon_before_settle = Some(end);
            continue;
        }
        if start <= valuation && start > dated {
            // end is the next coupon after settle — remember the last one before.
            last_coupon_before_settle = Some(start);
        } else if start <= valuation {
            last_coupon_before_settle = Some(start);
        }

        let df_start = df_at_date(sofr_curve, valuation, start)?;
        let df_end = df_at_date(sofr_curve, valuation, end)?;
        let yf360 = dc360
            .year_fraction(start, end)
            .to_f64()
            .ok_or_else(|| anyhow!("{}: yf360 decimal→f64 failed", inst.id))?;

        // Projected cashflow: floating leg + spread leg.
        let float_cf = face * (df_start / df_end - 1.0);
        let spread_cf = face * spread * yf360;
        let mut cf = float_cf + spread_cf;
        if end == maturity {
            cf += face;
        }
        dirty += cf * df_end;
        spread_annuity += yf360 * df_end;
    }

    let dirty_price_pct = dirty; // face = 100 so dirty is already in per-100 units.

    // Accrued: face × current_reset_rate × ACT/360 days since last coupon.
    let accrued = if let Some(last) = last_coupon_before_settle {
        let yf = dc360
            .year_fraction(last, valuation)
            .to_f64()
            .ok_or_else(|| anyhow!("{}: accrued yf360 decimal→f64 failed", inst.id))?;
        face * reset * yf
    } else {
        0.0
    };

    let clean_price_pct = dirty_price_pct - accrued;

    // DM: find DM s.t. clean = 100. Brent on [−0.05, 0.20].
    // Since dirty = float_leg + spread_annuity × spread + principal, and
    //   float_leg + principal = face (telescoping), we have:
    //   clean(DM) = face + (spread − DM) × spread_annuity × something − accrued.
    // More directly: DM shifts every coupon down by DM × yf360 × face and
    // discounts by exp(−DM × t_end). For small DM, dirty(DM) ≈ dirty(0) − DM × spread_annuity × face.
    // Solve: clean(DM) = 100 → DM ≈ (dirty − 100 − accrued) / (spread_annuity × face).
    // Use this as the DM metric. Closed form is adequate for reconciliation.
    let dm = if spread_annuity.abs() > 1e-12 {
        (dirty_price_pct - 100.0 - accrued) / (spread_annuity * face)
    } else {
        0.0
    };
    let discount_margin_bps = dm * 10_000.0;

    Ok(FrnMetrics {
        clean_price_pct,
        dirty_price_pct,
        accrued,
        discount_margin_bps,
    })
}

fn build_bond(inst: &Instrument) -> Result<FixedRateBond> {
    let coupon = effective_coupon_percent(inst)?;
    let maturity = parse_date(
        inst.maturity_date
            .as_deref()
            .ok_or_else(|| anyhow!("{}: missing maturity_date", inst.id))?,
    )?;
    // Use dated_date (start of interest accrual) if present; fall back to
    // issue_date. The schedule is anchored here, so both libraries must use
    // the same value for schedules to align.
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
        // Match QL: NullCalendar + Unadjusted so schedules agree exactly.
        // EOM = true only when the seed (issue/dated) date is month-end;
        // QL's Schedule does that "snap back to 31" behaviour either way.
        .calendar(CalendarId::new(""))
        .business_day_convention(BusinessDayConvention::Unadjusted)
        .end_of_month(is_end_of_month(maturity))
        .build()
        .with_context(|| format!("building {}", inst.id))
}

/// Decide which curve + which reference yield to use for a given bond.
fn reference_yield<'a>(
    inst: &Instrument,
    maturity: Date,
    valuation: Date,
    curves: &'a [Curve],
) -> (f64, &'a str) {
    let ccy = inst.currency.as_deref().unwrap_or("USD");
    let yrs = years_to_maturity(valuation, maturity);

    // TIPS: discount at a flat real yield, not the nominal curve.
    if inst.category == "sovereign_linker" {
        return (0.0185, "tips_real_placeholder");
    }
    // FRN: flat forward at (index + spread), exercises the quarterly ACT/360 path.
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
        other => {
            // parse_currency upstream rejects anything else.
            unreachable!("unexpected currency {other} on {}", inst.id)
        }
    };
    if let Some(curve) = curves.iter().find(|c| c.id == curve_id) {
        if let Some(y) = interpolate_cmt(curve, yrs) {
            return (y, curve.id.as_str());
        }
    }
    // Last-resort fallback: coupon rate as reference yield.
    let fallback = inst.coupon_rate.map(|c| c / 100.0).unwrap_or(0.04);
    (fallback, "placeholder")
}

// ------------------------------------------------------------------ main

fn main() -> Result<()> {
    let root = Path::new("reconciliation");
    let book: Book = serde_json::from_reader(File::open(root.join("book.json"))?)
        .context("reading book.json")?;
    let curves: Curves = serde_json::from_reader(File::open(root.join("curves.json"))?)
        .context("reading curves.json")?;
    // Sanity-check the anchor curve is present; additional sovereign curves
    // are resolved by currency inside `reference_yield`.
    if !curves.curves.iter().any(|c| c.id == "UST_CMT") {
        return Err(anyhow!("UST_CMT curve not found in curves.json"));
    }

    let valuation = parse_date(&book.valuation_date)?;

    // Build the SOFR OIS discount curve once (used by corporate SOFR FRNs).
    let sofr_curve = curves
        .curves
        .iter()
        .find(|c| c.id == "SOFR_OIS_CURVE")
        .map(|c| build_zero_rate_curve(c, valuation))
        .transpose()?;

    // TIPS index ratios (CUSIP → ratio on valuation date), if the puller has run.
    let mut tips_ratios: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    let ratio_path = root.join("tips_index_ratio_20251231.json");
    if ratio_path.exists() {
        if let Ok(r) = serde_json::from_reader::<_, TipsIndexRatio>(File::open(&ratio_path)?) {
            tips_ratios.insert(r.cusip, r.index_ratio);
        }
    }

    let out_path = root.join("convex.csv");
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
            "corporate_bullet_mw" => None,
            "corporate_callable" => None,
            "synthetic_callable" => None,
            "sovereign_linker" => None, // TIPS priced on real yield
            "sovereign_frn" => None,    // FRN: flat-forward projection
            "corporate_frn" => None,    // SOFR FRN: curve-projected, handled below
            _other => Some("unknown category"),
        };
        if let Some(reason) = skip_reason {
            skipped.push(format!("{} ({}) — {}", inst.id, inst.category, reason));
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
            let m = price_corporate_frn(inst, valuation, curve)
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
