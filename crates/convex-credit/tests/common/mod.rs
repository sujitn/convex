//! Test-only supplied original grid inputs, independent of expected results.
use convex_core::{
    calendars::{DynamicCalendar, WeekendType},
    contracts::{ExactDecimal, FiniteF64, TradeId},
    Date,
};
use convex_credit::{CalendarBinding, CdsCalendars, CdsContract, CdsTerms};
use convex_curves::contracts::{build_curve_set, CurveRecipe, CurveSet};
use rust_decimal::Decimal;
use serde::Deserialize;
use std::error::Error;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inputs {
    pub schema_version: u32,
    pub qualification: String,
    pub source_archive_sha256: String,
    pub reference_model_version: String,
    pub template: CdsTerms,
    pub calendar_holidays: Vec<Date>,
    pub discount_recipe: CurveRecipe,
    pub cases: Vec<InputCase>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputCase {
    pub row: u32,
    pub trade_date: Date,
    pub maturity_date: Date,
    pub currency: String,
    pub coupon_bps: String,
    pub spread_bps: String,
    pub recovery: String,
    pub cash_settlement_date: Date,
    pub step_in_date: Date,
}
pub fn inputs() -> Result<Inputs, Box<dyn Error>> {
    let data: Inputs = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/comparisons/cds-isda-1.8.3/inputs.json"
    ))?;
    if data.schema_version != 1
        || data.reference_model_version != "1.8.3"
        || data.source_archive_sha256
            != "4ecf867697c1a6b4e0966743727820bf8b81f9db60e0e4fe55c1c804f4b981da"
        || data.qualification.is_empty()
        || data.cases.len() != 7164
    {
        return Err("unexpected supplied original input pack".into());
    }
    Ok(data)
}
pub fn market(input: &Inputs) -> Result<CurveSet, Box<dyn Error>> {
    Ok(build_curve_set(
        std::slice::from_ref(&input.discount_recipe),
        &CurveSet::new(),
    )?
    .curves)
}
pub fn calendar(input: &Inputs) -> DynamicCalendar {
    DynamicCalendar::from_dates(
        "ISDA.NYM.2022",
        WeekendType::SaturdaySunday,
        input.calendar_holidays.clone(),
    )
}
pub fn build(terms: &CdsTerms, cal: &DynamicCalendar) -> Result<CdsContract, Box<dyn Error>> {
    Ok(terms.build(&CdsCalendars {
        premium: CalendarBinding {
            pin: &terms.premium_calendar,
            calendar: cal,
        },
        settlement: CalendarBinding {
            pin: &terms.settlement_calendar,
            calendar: cal,
        },
    })?)
}
pub fn terms(input: &Inputs, c: &InputCase) -> Result<CdsTerms, Box<dyn Error>> {
    if c.currency != "USD" {
        return Err("wrong original case currency".into());
    }
    let mut t = input.template.clone();
    t.trade_id = TradeId::new(format!("ISDA.USD.20220621.row{}", c.row))?;
    t.trade_date = c.trade_date;
    t.step_in_date = c.step_in_date;
    t.protection_start_date = c.step_in_date;
    t.cash_settlement_date = c.cash_settlement_date;
    t.premium_schedule.end_date = c.maturity_date;
    t.coupon_decimal = ExactDecimal::new(
        ExactDecimal::parse(&c.coupon_bps)?
            .decimal()
            .checked_div(Decimal::from(10000))
            .ok_or("coupon unit conversion")?,
    );
    t.reference.recovery_fraction = FiniteF64::new(c.recovery.parse()?)?;
    Ok(t)
}
