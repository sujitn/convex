//! Standalone offline comparison adapter over existing pure library functions.
//! Build: cargo build --locked -p convex-analytics --example offline_compare
//! Input/output: the schema-1 protocol documented in reconciliation/COMPARISON.md.
#![forbid(unsafe_code)]
#![deny(
    unused_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use std::collections::BTreeMap;
use std::error::Error;
use std::io::{self, Read};

use convex_analytics::functions::{
    clean_price_from_yield, convexity, dirty_price_from_yield, macaulay_duration, modified_duration,
};
use convex_bonds::instruments::FixedRateBond;
use convex_bonds::traits::Bond;
use convex_bonds::types::CalendarId;
use convex_core::calendars::BusinessDayConvention;
use convex_core::daycounts::DayCountConvention;
use convex_core::types::{Currency, Date, Frequency};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const MAX_BYTES: u64 = 8 * 1024 * 1024;
const MAX_CASES: usize = 2048;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema_version: u32,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    id: String,
    operation: String,
    terms: Value,
    conventions: Value,
    context: Value,
    model: Value,
    context_hash: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DayCountTerms {
    start_date: String,
    end_date: String,
    convention: DayCountConvention,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BondTerms {
    evaluation_date: String,
    settlement_date: String,
    issue_date: String,
    maturity_date: String,
    coupon_rate: f64,
    frequency: Frequency,
    day_count: DayCountConvention,
    face_value: f64,
    settlement_days: u32,
    yield_to_maturity: f64,
}

#[derive(Serialize)]
struct Metric {
    value: String,
    unit: &'static str,
}

#[derive(Serialize)]
struct Output {
    id: String,
    context_hash: String,
    status: &'static str,
    conventions: Value,
    metrics: BTreeMap<&'static str, Metric>,
    error: Option<String>,
}

fn metric(value: f64, unit: &'static str) -> Result<Metric> {
    if !value.is_finite() {
        return Err("nonfinite library output".into());
    }
    Ok(Metric {
        value: value.to_string(),
        unit,
    })
}

fn day_count(terms: Value) -> Result<(Value, BTreeMap<&'static str, Metric>)> {
    let terms: DayCountTerms = serde_json::from_value(terms)?;
    if !matches!(
        terms.convention,
        DayCountConvention::Act360
            | DayCountConvention::Act365Fixed
            | DayCountConvention::ActActIsda
            | DayCountConvention::Thirty360US
            | DayCountConvention::Thirty360E
    ) {
        return Err("reference-period/model admission is required for this convention".into());
    }
    let start = Date::parse(&terms.start_date)?;
    let end = Date::parse(&terms.end_date)?;
    if !(1..=9999).contains(&start.year())
        || !(1..=9999).contains(&end.year())
        || start.days_between(&end).abs() > 150 * 366
    {
        return Err("day-count dates exceed the comparison bounds".into());
    }
    let dc = terms.convention.to_day_count();
    let fraction = dc
        .year_fraction(start, end)
        .to_f64()
        .ok_or("decimal conversion failed")?;
    let values = BTreeMap::from([
        (
            "day_count",
            metric(dc.day_count(start, end) as f64, "days")?,
        ),
        ("year_fraction", metric(fraction, "years")?),
    ]);
    Ok((json!({"day_count": terms.convention}), values))
}

fn fixed_bond(terms: Value) -> Result<(Value, BTreeMap<&'static str, Metric>)> {
    let terms: BondTerms = serde_json::from_value(terms)?;
    let evaluation = Date::parse(&terms.evaluation_date)?;
    let settlement = Date::parse(&terms.settlement_date)?;
    let issue = Date::parse(&terms.issue_date)?;
    let maturity = Date::parse(&terms.maturity_date)?;
    if evaluation > settlement
        || issue > settlement
        || settlement >= maturity
        || issue.days_between(&maturity) > 150 * 366
        || terms.face_value != 100.0
        || !terms.coupon_rate.is_finite()
        || !(0.0..=1.0).contains(&terms.coupon_rate)
        || !terms.yield_to_maturity.is_finite()
        || !(-0.5..=1.0).contains(&terms.yield_to_maturity)
        || !matches!(terms.frequency, Frequency::Annual | Frequency::SemiAnnual)
        || terms.settlement_days > 10
        || [evaluation, settlement, issue, maturity]
            .iter()
            .any(|date| !(1..=9999).contains(&date.year()))
    {
        return Err("unsupported/bad bond economics for this comparison adapter".into());
    }
    let bond = FixedRateBond::builder()
        .cusip_unchecked("SYNTHETIC-COMPARISON")
        .issue_date(issue)
        .dated_date(issue)
        .maturity(maturity)
        .coupon_rate(Decimal::try_from(terms.coupon_rate)?)
        .face_value(Decimal::ONE_HUNDRED)
        .redemption_value(Decimal::ONE_HUNDRED)
        .currency(Currency::USD)
        .frequency(terms.frequency)
        .day_count(terms.day_count)
        .settlement_days(terms.settlement_days)
        .calendar(CalendarId::us_government())
        .business_day_convention(BusinessDayConvention::Following)
        .end_of_month(false)
        .build()?;
    let ytm = terms.yield_to_maturity;
    let frequency = terms.frequency;
    let values = BTreeMap::from([
        (
            "clean_price",
            metric(
                clean_price_from_yield(&bond, settlement, ytm, frequency)?,
                "price_per_100",
            )?,
        ),
        (
            "dirty_price",
            metric(
                dirty_price_from_yield(&bond, settlement, ytm, frequency)?,
                "price_per_100",
            )?,
        ),
        (
            "accrued_interest",
            metric(
                bond.accrued_interest(settlement)
                    .to_f64()
                    .ok_or("decimal conversion failed")?,
                "price_per_100",
            )?,
        ),
        (
            "macaulay_duration",
            metric(
                macaulay_duration(&bond, settlement, ytm, frequency)?,
                "years",
            )?,
        ),
        (
            "modified_duration",
            metric(
                modified_duration(&bond, settlement, ytm, frequency)?,
                "years",
            )?,
        ),
        (
            "convexity",
            metric(
                convexity(&bond, settlement, ytm, frequency)?,
                "years_squared",
            )?,
        ),
    ]);
    let conventions = json!({
        "day_count": terms.day_count,
        "frequency": frequency,
        "accrual_adjustment": "Unadjusted",
        "payment_adjustment": "Following",
        "payment_calendar": "USGovernment",
        "schedule_rule": "Backward",
        "end_of_month": false,
        "yield_compounding": "Compounded",
        "include_settlement_date_flows": false,
        "redemption_per_100": 100
    });
    Ok((conventions, values))
}

fn run() -> Result<()> {
    let mut content = Vec::new();
    io::stdin().take(MAX_BYTES + 1).read_to_end(&mut content)?;
    if content.len() as u64 > MAX_BYTES {
        return Err("input exceeds 8 MiB".into());
    }
    let request: Request = serde_json::from_slice(&content)?;
    if request.schema_version != 1 || request.cases.is_empty() || request.cases.len() > MAX_CASES {
        return Err("unknown schema or missing/unbounded cases".into());
    }
    let mut outputs = Vec::with_capacity(request.cases.len());
    for case in request.cases {
        if case.id.is_empty()
            || case.id.len() > 256
            || case.context_hash.len() != 64
            || !case.context_hash.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err("invalid case identity/context".into());
        }
        if matches!(case.operation.as_str(), "day_count" | "fixed_bond")
            && (!case.conventions.is_object()
                || !case.model["settings"].is_object()
                || !case.model["approximations"]
                    .as_array()
                    .is_some_and(Vec::is_empty)
                || !case.context["reference_versions"].is_object()
                || ["curves", "quotes", "fixings", "calendars"]
                    .iter()
                    .any(|key| !case.context[*key].as_array().is_some_and(Vec::is_empty)))
        {
            return Err(
                "this adapter requires explicit settings and no external data or approximations"
                    .into(),
            );
        }
        let settlement_key = if case.operation == "day_count" {
            "start_date"
        } else {
            "settlement_date"
        };
        if (case.operation == "day_count" || case.operation == "fixed_bond")
            && (case.context["settlement_date"].as_str().is_none()
                || case.context["settlement_date"] != case.terms[settlement_key])
        {
            return Err("settlement context differs from supplied economics".into());
        }
        let result = match case.operation.as_str() {
            "day_count" => day_count(case.terms),
            "fixed_bond" => fixed_bond(case.terms),
            _ => {
                outputs.push(Output {
                    id: case.id,
                    context_hash: case.context_hash,
                    status: "unsupported",
                    conventions: json!({}),
                    metrics: BTreeMap::new(),
                    error: Some("no admitted adapter for this operation".into()),
                });
                continue;
            }
        };
        let (status, conventions, metrics, error) = match result {
            Ok((conventions, metrics)) => ("ok", conventions, metrics, None),
            Err(error) => ("error", json!({}), BTreeMap::new(), Some(error.to_string())),
        };
        outputs.push(Output {
            id: case.id,
            context_hash: case.context_hash,
            status,
            conventions,
            metrics,
            error,
        });
    }
    serde_json::to_writer(
        io::stdout(),
        &json!({"schema_version": 1,
            "producer": {"name": "Convex pure library comparison adapter",
                "version": env!("CARGO_PKG_VERSION"), "approximations": []},
            "cases": outputs}),
    )?;
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("offline adapter rejected: {error}");
        std::process::exit(2);
    }
}
