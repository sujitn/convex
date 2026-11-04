//! Native export of the pinned CVX-019 external-comparison input suite.
//! This command supplies candidate outputs; it never supplies external evidence.
#![forbid(unsafe_code)]
#![deny(
    unused_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use convex::{
    cashflows::{CashflowError, PaymentDateEntitlement},
    contracts::{IndexId, UtcTimestamp},
    curve_contracts::{build_curve_set, CurveRecipe, CurveSet},
    fixings::{FixingContext, FixingObservation, FixingSeries, RateOrigin},
    rates::{
        CalendarBinding, CashflowContext, CouponCashflow, FloatingLegTerms, LegCalendarPins,
        LegCalendars, ReferencePin, SwapCalendars, SwapTerms,
    },
    swap_pricing::{price_swap, DiscountedCoupon, SwapParQuotes, SwapValuationInputs},
    Date,
};
use convex_core::calendars::Calendar;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

type Error = Box<dyn std::error::Error>;

struct Holidays(BTreeSet<Date>);

impl Calendar for Holidays {
    fn name(&self) -> &'static str {
        "pinned CVX-019 comparator input calendar"
    }

    fn is_business_day(&self, at: Date) -> bool {
        !at.is_weekend() && !self.0.contains(&at)
    }
}

fn bind<'a>(
    pin: &'a ReferencePin,
    calendars: &'a BTreeMap<String, Holidays>,
) -> Result<CalendarBinding<'a>, Error> {
    if pin.version.get() != 1 {
        return Err("the comparator suite binds only supplied calendar version 1".into());
    }
    Ok(CalendarBinding {
        pin,
        calendar: calendars
            .get(pin.id.as_str())
            .ok_or("missing supplied comparator calendar")?,
    })
}

fn leg<'a>(
    pins: &'a LegCalendarPins,
    calendars: &'a BTreeMap<String, Holidays>,
) -> Result<LegCalendars<'a>, Error> {
    Ok(LegCalendars {
        accrual: bind(&pins.accrual, calendars)?,
        payment: bind(&pins.payment, calendars)?,
    })
}

fn publication(_: &IndexId, at: Date) -> Result<UtcTimestamp, CashflowError> {
    UtcTimestamp::parse(&format!("{at}T08:00:00Z")).map_err(|e| CashflowError::Date(e.to_string()))
}

fn coupons(cash: &[CouponCashflow], pv: &[DiscountedCoupon]) -> Result<Vec<Value>, Error> {
    if cash.len() != pv.len() {
        return Err("cash/PV length mismatch".into());
    }
    Ok(cash
        .iter()
        .zip(pv)
        .map(|(cash, pv)| {
            let (start, end) = cash.period.accrual_dates();
            json!({
                "start": start,
                "end": end,
                "payment": cash.period.payment_date(),
                "year_fraction": cash.year_fraction.to_string(),
                "amount": cash.signed_amount.to_string(),
                "discount_ratio": pv.discount_ratio.get().to_string(),
                "pv": pv.present_value.value().get().to_string(),
            })
        })
        .collect())
}

fn main() -> Result<(), Error> {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/comparisons/swap-quantlib-inputs.json"
    ))?;
    let mut calendars = BTreeMap::new();
    for (id, dates) in fixture["calendar_holidays"]
        .as_object()
        .ok_or("missing supplied calendars")?
    {
        let dates: Vec<Date> = serde_json::from_value(dates.clone())?;
        calendars.insert(id.clone(), Holidays(dates.into_iter().collect()));
    }
    let rows = fixture["cases"]
        .as_array()
        .ok_or("missing supplied cases")?;
    if rows.len() != 10 {
        return Err("comparator suite must retain all ten supplied cases".into());
    }
    let mut outputs = Vec::new();
    let mut identities = BTreeSet::new();
    for row in rows {
        let id = row["id"].as_str().ok_or("missing case identity")?;
        if !identities.insert(id) {
            return Err("duplicate comparator case".into());
        }
        let terms: SwapTerms = serde_json::from_value(row["terms"].clone())?;
        let (reset, fixing, observation) = match &terms.floating {
            FloatingLegTerms::Term(t) => (
                Some(bind(&t.resets.reset_calendar, &calendars)?),
                Some(bind(&t.resets.fixing_calendar, &calendars)?),
                None,
            ),
            FloatingLegTerms::Overnight(t) => {
                (None, None, Some(bind(&t.observation_calendar, &calendars)?))
            }
        };
        let bindings = SwapCalendars {
            fixed: leg(&terms.fixed.leg.calendars, &calendars)?,
            floating: leg(&terms.floating.leg().calendars, &calendars)?,
            reset,
            fixing,
            observation,
        };
        let contract = terms.clone().build(&bindings)?;
        let curve_id = row["curve_set"].as_str().ok_or("missing curve set")?;
        let recipes: Vec<CurveRecipe> =
            serde_json::from_value(fixture["curve_sets"][curve_id].clone())?;
        let curves = build_curve_set(&recipes, &CurveSet::new())?.curves;
        let history_id = row["fixing_set"].as_str().ok_or("missing fixing set")?;
        let history = FixingSeries::new(serde_json::from_value::<Vec<FixingObservation>>(
            fixture["fixing_sets"][history_id].clone(),
        )?)?;
        let context = CashflowContext {
            fixings: FixingContext {
                valuation_date: serde_json::from_value(row["valuation_date"].clone())?,
                known_at: serde_json::from_value(row["known_at"].clone())?,
                unpublished: contract.terms().floating.fixing_policy().unpublished,
                final_only: true,
            },
            on_payment: serde_json::from_value::<PaymentDateEntitlement>(
                row["on_payment"].clone(),
            )?,
        };
        let value = price_swap(
            &contract,
            &SwapValuationInputs {
                curves: &curves,
                fixings: &history,
            },
            context,
            publication,
        )?;
        let SwapParQuotes::Available {
            fixed_rate,
            floating_spread,
            fixed_rate_shift,
            floating_spread_shift,
            fixed_annuity,
            floating_annuity,
        } = &value.par
        else {
            return Err("the pinned linear comparator suite requires available par quotes".into());
        };
        let rates: Vec<_> = value
            .cashflows
            .rates
            .iter()
            .map(|used| {
                let (origin, revision) = match &used.resolved.origin {
                    RateOrigin::Published(observation) => {
                        ("Published", Some(observation.revision.get()))
                    }
                    RateOrigin::Projected(_) => ("Projected", None),
                };
                json!({
                    "index": used.request.requirement.index_id,
                    "observation": used.request.requirement.observation_date,
                    "rate_start": used.request.rate_start,
                    "rate_end": used.request.rate_end,
                    "rate": used.resolved.rate.to_string(),
                    "origin": origin,
                    "revision": revision,
                })
            })
            .collect();
        outputs.push(json!({
            "id": id,
            "currency": value.cashflows.currency,
            "valuation_date": context.fixings.valuation_date,
            "known_at": context.fixings.known_at,
            "side": value.cashflows.terms.side,
            "pv": value.present_value.value().get().to_string(),
            "fixed_pv": value.fixed_present_value.value().get().to_string(),
            "floating_pv": value.floating_present_value.value().get().to_string(),
            "fixed_accrued": value.fixed_accrued.value().decimal().to_string(),
            "floating_accrued": value.floating_accrued.value().decimal().to_string(),
            "par": {
                "fixed_rate": fixed_rate.value().get().to_string(),
                "floating_spread": floating_spread.value().get().to_string(),
                "fixed_rate_shift": fixed_rate_shift.value().get().to_string(),
                "floating_spread_shift": floating_spread_shift.value().get().to_string(),
                "fixed_annuity": fixed_annuity.value().get().to_string(),
                "floating_annuity": floating_annuity.value().get().to_string(),
            },
            "fixed": coupons(&value.cashflows.fixed, &value.fixed)?,
            "floating": coupons(&value.cashflows.floating, &value.floating)?,
            "rates": rates,
        }));
    }
    println!(
        "{}",
        serde_json::to_string(&json!({
            "schema_version": 1,
            "engine": "Convex",
            "package_version": env!("CARGO_PKG_VERSION"),
            "qualification": "native_candidate_not_external_evidence",
            "inputs": fixture,
            "cases": outputs,
        }))?
    );
    Ok(())
}
