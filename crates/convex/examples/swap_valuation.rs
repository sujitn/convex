//! Standalone supplied IRS valuation and native par-quote projection calibration.
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
    curve_contracts::{
        build_curve_set, CalibrationMode, CurveRecipe, CurveSet, CurveSource, FitNumericalControls,
        MarketQuote, QuoteUnit,
    },
    fixings::{FixingContext, FixingObservation, FixingSeries, UnpublishedFixingPolicy},
    rates::{
        CalendarBinding, CashflowContext, FloatingLegTerms, LegCalendars, ReferencePin,
        SwapCalendars, SwapTerms,
    },
    swap_calibration::{
        build_swap_curve_set, SwapBootstrapSettings, SwapCalibrationHelper, SwapCurveBuildInput,
        SwapCurveRecipe,
    },
    swap_pricing::{price_swap, SwapValuationInputs},
    Date,
};
use convex_core::calendars::Calendar;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
type Error = Box<dyn std::error::Error>;
struct Holidays(BTreeSet<Date>);
impl Calendar for Holidays {
    fn name(&self) -> &'static str {
        "pinned synthetic swap calendar"
    }
    fn is_business_day(&self, at: Date) -> bool {
        !at.is_weekend() && !self.0.contains(&at)
    }
}
fn bind<'a>(
    pin: &'a ReferencePin,
    calendars: &'a BTreeMap<String, Holidays>,
) -> Result<CalendarBinding<'a>, Error> {
    Ok(CalendarBinding {
        pin,
        calendar: calendars
            .get(pin.id.as_str())
            .ok_or("missing exact supplied calendar")?,
    })
}
fn leg<'a>(
    pins: &'a convex::rates::LegCalendarPins,
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
fn main() -> Result<(), Error> {
    let cases: Value =
        serde_json::from_str(include_str!("../../convex-rates/tests/fixtures/swaps.json"))?;
    let fixture: Value = serde_json::from_str(include_str!(
        "../../convex-rates/tests/fixtures/valuation.json"
    ))?;
    let source = cases["cases"]
        .as_array()
        .ok_or("missing supplied cases")?
        .iter()
        .find(|c| c["id"] == "regular-eom")
        .ok_or("missing supplied regular IRS")?;
    let row = fixture["cases"]
        .as_array()
        .and_then(|a| a.first())
        .ok_or("missing independent valuation input")?;
    let terms: SwapTerms = serde_json::from_value(source["terms"].clone())?;
    let mut calendars = BTreeMap::new();
    for (id, dates) in source["calendar_holidays"]
        .as_object()
        .ok_or("missing calendars")?
    {
        let dates: Vec<String> = serde_json::from_value(dates.clone())?;
        let holidays = dates
            .iter()
            .map(|s| Date::parse(s))
            .collect::<Result<BTreeSet<_>, _>>()?;
        calendars.insert(id.clone(), Holidays(holidays));
    }
    let FloatingLegTerms::Term(term) = &terms.floating else {
        return Err("this supplied example requires term IRS".into());
    };
    let bindings = SwapCalendars {
        fixed: leg(&terms.fixed.leg.calendars, &calendars)?,
        floating: leg(&term.leg.calendars, &calendars)?,
        reset: Some(bind(&term.resets.reset_calendar, &calendars)?),
        fixing: Some(bind(&term.resets.fixing_calendar, &calendars)?),
        observation: None,
    };
    let contract = terms.clone().build(&bindings)?;
    let context = CashflowContext {
        fixings: FixingContext {
            valuation_date: serde_json::from_value(row["valuation_date"].clone())?,
            known_at: serde_json::from_value(row["known_at"].clone())?,
            unpublished: UnpublishedFixingPolicy::Forecast,
            final_only: true,
        },
        on_payment: PaymentDateEntitlement::Include,
    };
    let history = FixingSeries::new(serde_json::from_value::<Vec<FixingObservation>>(
        row["fixings"].clone(),
    )?)?;
    let recipes: Vec<CurveRecipe> = serde_json::from_value(row["curves"].clone())?;
    let curves = build_curve_set(&recipes, &CurveSet::new())?.curves;
    let price = price_swap(
        &contract,
        &SwapValuationInputs {
            curves: &curves,
            fixings: &history,
        },
        context,
        publication,
    )?;
    let reference: f64 = row["expected"]["pv"]
        .as_str()
        .ok_or("missing reference PV")?
        .parse()?;
    if (price.present_value.value().get() - reference).abs() > 1e-7 {
        return Err("independent reference PV comparison failed".into());
    }
    let mut recipe = curves
        .get(terms.floating.projection_curve())?
        .recipe()
        .clone();
    recipe.source = CurveSource::Helpers(vec!["supplied-six-month-par".into()]);
    recipe.calibration = CalibrationMode::Piecewise;
    recipe.dependencies = vec![terms.collateral.discount_curve.clone()];
    let pillar = Date::parse("2024-07-31")?;
    let product = SwapCurveRecipe {
        curve: recipe,
        pillars: vec![pillar],
        helpers: vec![SwapCalibrationHelper {
            id: "supplied-six-month-par".into(),
            pillar,
            terms: terms.clone(),
            quote: MarketQuote {
                value: 0.04,
                unit: QuoteUnit::DecimalRate,
            },
            tolerance: 1e-12,
        }],
        overnight_projection: None,
        bootstrap: Some(SwapBootstrapSettings {
            lower_zero: -0.1,
            upper_zero: 0.15,
            max_iterations: 100,
        }),
    };
    let calendars = [bindings];
    let input = SwapCurveBuildInput {
        recipe: &product,
        calendars: &calendars,
    };
    let discount = curves
        .get(&terms.collateral.discount_curve)?
        .as_ref()
        .clone();
    let fit = build_swap_curve_set(
        &[input],
        &CurveSet::new().with_curve(discount)?,
        &history,
        context,
        FitNumericalControls::default(),
        publication,
    )?;
    let calibrated = fit
        .calibrations
        .first()
        .ok_or("missing actual calibration")?;
    let native = calibrated
        .curve
        .diagnostics()
        .quotes
        .first()
        .ok_or("missing native repricing")?;
    if native.residual.abs() > native.tolerance {
        return Err("native quote repricing failed".into());
    }
    println!(
        "{}",
        json!({"status":"native_candidate","certified":false,"pv":price.present_value,"fixed_pv":price.fixed_present_value,"floating_pv":price.floating_present_value,"fixed_cashflows":price.fixed.len(),"floating_cashflows":price.floating.len(),"consumed_curves":price.consumed_curves.len(),"known_rates":price.cashflows.rates.iter().filter(|r|matches!(r.resolved.origin,convex::fixings::RateOrigin::Published(_))).count(),"calibration":calibrated.curve.diagnostics(),"price_evaluations":fit.price_evaluations,"independent_engine":"Python Decimal-60; ORE/QuantLib capture pending"})
    );
    Ok(())
}
