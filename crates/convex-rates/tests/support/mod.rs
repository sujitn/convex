//! Shared test-only actual supplied contracts. No production code imports this.
#![allow(dead_code)]
use convex_core::{
    calendars::Calendar,
    cashflows::{CashflowError, PaymentDateEntitlement},
    contracts::{IndexId, UtcTimestamp},
    fixings::{FixingContext, FixingObservation, FixingSeries, UnpublishedFixingPolicy},
    Date,
};
use convex_curves::contracts::{build_curve_set, CurveRecipe, CurveSet};
use convex_rates::{
    CalendarBinding, CashflowContext, FloatingLegTerms, LegCalendars, ReferencePin, SwapCalendars,
    SwapContract, SwapTerms,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub fn d(s: &str) -> Date {
    Date::parse(s).unwrap()
}
pub fn ts(s: &str) -> UtcTimestamp {
    serde_json::from_value(json!(s)).unwrap()
}
pub fn publication(_: &IndexId, at: Date) -> Result<UtcTimestamp, CashflowError> {
    Ok(ts(&format!("{at}T08:00:00Z")))
}
pub fn context(at: Date) -> CashflowContext {
    CashflowContext {
        fixings: FixingContext {
            valuation_date: at,
            known_at: ts(&format!("{at}T23:59:59Z")),
            unpublished: UnpublishedFixingPolicy::Forecast,
            final_only: true,
        },
        on_payment: PaymentDateEntitlement::Include,
    }
}
pub fn case(id: &str) -> Value {
    serde_json::from_str::<Value>(include_str!("../fixtures/swaps.json")).unwrap()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == id)
        .unwrap()
        .clone()
}
pub fn terms(case: &Value) -> SwapTerms {
    serde_json::from_value(case["terms"].clone()).unwrap()
}
pub struct Holidays(BTreeSet<Date>);
impl Calendar for Holidays {
    fn name(&self) -> &'static str {
        "pinned swap test calendar"
    }
    fn is_business_day(&self, at: Date) -> bool {
        !at.is_weekend() && !self.0.contains(&at)
    }
}
pub fn calendars(case: &Value) -> BTreeMap<String, Holidays> {
    case["calendar_holidays"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(id, dates)| {
            (
                id.clone(),
                Holidays(
                    dates
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|v| d(v.as_str().unwrap()))
                        .collect(),
                ),
            )
        })
        .collect()
}
pub fn bindings<'a>(
    terms: &'a SwapTerms,
    calendars: &'a BTreeMap<String, Holidays>,
) -> SwapCalendars<'a> {
    fn bind<'a>(
        pin: &'a ReferencePin,
        calendars: &'a BTreeMap<String, Holidays>,
    ) -> CalendarBinding<'a> {
        CalendarBinding {
            pin,
            calendar: calendars.get(pin.id.as_str()).unwrap(),
        }
    }
    let leg = |pins: &'a convex_rates::LegCalendarPins| LegCalendars {
        accrual: bind(&pins.accrual, calendars),
        payment: bind(&pins.payment, calendars),
    };
    let (reset, fixing, observation) = match &terms.floating {
        FloatingLegTerms::Term(t) => (
            Some(bind(&t.resets.reset_calendar, calendars)),
            Some(bind(&t.resets.fixing_calendar, calendars)),
            None,
        ),
        FloatingLegTerms::Overnight(t) => {
            (None, None, Some(bind(&t.observation_calendar, calendars)))
        }
    };
    SwapCalendars {
        fixed: leg(&terms.fixed.leg.calendars),
        floating: leg(&terms.floating.leg().calendars),
        reset,
        fixing,
        observation,
    }
}
pub fn build(terms: &SwapTerms, calendars: &BTreeMap<String, Holidays>) -> SwapContract {
    terms.clone().build(&bindings(terms, calendars)).unwrap()
}
pub fn history(case: &Value, contract: &SwapContract, context: CashflowContext) -> FixingSeries {
    let index = contract.terms().floating.index_id();
    let values = contract
        .fixing_requirements(context, publication)
        .unwrap()
        .into_iter()
        .filter(|r| r.observation_date <= context.fixings.valuation_date)
        .map(|r| FixingObservation {
            index_id: index.clone(),
            observation_date: r.observation_date,
            rate: case["rates"][r.observation_date.to_string()]
                .as_str()
                .or_else(|| case["constant_rate"].as_str())
                .unwrap()
                .parse()
                .unwrap(),
            published_at: r.expected_publication,
            known_at: r.expected_publication,
            revision: convex_core::contracts::Version::new(1).unwrap(),
            status: convex_core::fixings::FixingStatus::Final,
            source_id: convex_core::contracts::SourceId::new("fixture").unwrap(),
        })
        .collect();
    FixingSeries::new(values).unwrap()
}
pub fn flat_curve(terms: &SwapTerms, reference: Date, discount: f64, projection: f64) -> CurveSet {
    let common = |key: &convex_curves::contracts::CurveKey, role: Value, zero: f64| {
        serde_json::from_value::<CurveRecipe>(json!({
        "key":key,"role":role,"currency":terms.currency,"reference_date":reference,"time_basis":"Act365Fixed",
        "representation":{"ZeroRates":{"compounding":"Continuous"}},"interpolation":"Linear","extrapolation":"None","dependencies":[],"calibration":"Direct",
        "source":{"Nodes":[{"pillar":{"Date":reference},"quote":{"value":zero,"unit":"DecimalRate"}},{"pillar":{"Date":"2030-01-01"},"quote":{"value":zero,"unit":"DecimalRate"}}]}
    })).unwrap()
    };
    build_curve_set(
        &[
            common(
                &terms.collateral.discount_curve,
                json!({"kind":"Discount","collateral_currency":terms.currency}),
                discount,
            ),
            common(
                terms.floating.projection_curve(),
                json!({"kind":"Projection","index_id":terms.floating.index_id()}),
                projection,
            ),
        ],
        &CurveSet::new(),
    )
    .unwrap()
    .curves
}
