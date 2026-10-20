//! Standalone supplied CDS quotes -> curves -> trade price -> signed quote risk.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use convex::{
    credit::{CalendarBinding, CdsCalendars, CdsTerms, InitialPaymentState},
    credit_calibration::*,
    credit_risk::*,
    curve_contracts::*,
    Date,
};
use convex_core::{
    calendars::{DynamicCalendar, WeekendType},
    contracts::{
        CurveId, FactorDefinition, FactorFamily, FactorTarget, FactorTenor, ModelId, QuoteType,
        RiskBasis, RiskFactorKey, Version,
    },
};
use serde_json::Value;
use std::error::Error;
fn calendars<'a>(terms: &'a CdsTerms, calendar: &'a DynamicCalendar) -> CdsCalendars<'a> {
    CdsCalendars {
        premium: CalendarBinding {
            pin: &terms.premium_calendar,
            calendar,
        },
        settlement: CalendarBinding {
            pin: &terms.settlement_calendar,
            calendar,
        },
    }
}
fn main() -> Result<(), Box<dyn Error>> {
    let data: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/comparisons/credit-isda-1.8.3/inputs.json"
    ))?;
    let template: CdsTerms = serde_json::from_value(data["template"].clone())?;
    let dates: Vec<Date> = serde_json::from_value(data["pillars"].clone())?;
    let spreads: Vec<f64> = serde_json::from_value(data["running_spreads_decimal"].clone())?;
    let discount: CurveRecipe = serde_json::from_value(data["discount_recipe"].clone())?;
    let holidays: Vec<Date> = serde_json::from_value(data["calendar_holidays"].clone())?;
    let calendar =
        DynamicCalendar::from_dates("supplied NYM", WeekendType::SaturdaySunday, holidays);
    let supplied = build_curve_set(std::slice::from_ref(&discount), &CurveSet::new())?.curves;
    let mut helpers = Vec::new();
    for (i, (date, spread)) in dates.iter().zip(&spreads).enumerate() {
        let mut terms = template.clone();
        terms.premium_schedule.end_date = *date;
        helpers.push(CdsCalibrationHelper {
            id: format!("supplied.cds.{i}"),
            quote_version: Version::new(1)?,
            pillar: *date,
            context: terms.trade_context(InitialPaymentState::NotContracted),
            terms,
            quote_kind: CdsQuoteKind::RunningSpread,
            quote: MarketQuote {
                value: spread * 10000.0,
                unit: QuoteUnit::BasisPoints,
            },
            tolerance: 1e-7,
        });
    }
    let mut curve = discount;
    curve.key = template.survival_curve.clone();
    curve.role = CurveRole::Credit {
        entity_id: template.reference.entity_id.as_str().into(),
        seniority: template.reference.seniority.as_str().into(),
        recovery: template.reference.recovery_fraction.get(),
    };
    curve.representation = CurveRepresentation::SurvivalProbabilities;
    curve.source = CurveSource::Helpers(helpers.iter().map(|h| h.id.clone()).collect());
    curve.calibration = CalibrationMode::Piecewise;
    curve.dependencies = vec![template.discount_curve.clone()];
    let recipe = CdsCurveRecipe {
        curve,
        pillars: dates,
        helpers,
        bootstrap: Some(CdsBootstrapSettings {
            lower_hazard: 0.0,
            upper_hazard: 1.0,
            max_iterations: 100,
        }),
    };
    let bindings: Vec<_> = recipe
        .helpers
        .iter()
        .map(|h| calendars(&h.terms, &calendar))
        .collect();
    let last = recipe.helpers.last().ok_or("missing supplied quote")?;
    let trade = last.terms.build(&calendars(&last.terms, &calendar))?;
    let context = trade
        .terms()
        .trade_context(convex::credit::InitialPaymentState::NotContracted);
    let prepared = prepare_credit_risk(CreditRiskInput {
        recipe: &recipe,
        helper_calendars: &bindings,
        supplied: &supplied,
        trade: &trade,
        trade_calendars: calendars(trade.terms(), &calendar),
        context: &context,
        controls: FitNumericalControls::default(),
        risk_model: ModelId::new("supplied.credit.central.v1")?,
    })?;
    let bump = CreditRiskBump::RunningQuote {
        helper_id: last.id.clone(),
        step_bps: 1.0,
    };
    let factor = RiskFactorKey::new(FactorDefinition {
        family: FactorFamily::CreditSpread,
        target: FactorTarget::Curve {
            curve_id: CurveId::new(&recipe.curve.key.id)?,
        },
        currency: template.currency,
        tenor: FactorTenor::new(format!(
            "{}D",
            recipe.curve.reference_date.days_between(&last.pillar)
        ))?,
        quote_type: QuoteType::CdsSpread,
        basis: RiskBasis::MarketQuoteRecalibrated,
        convention_version: template.reference.document.version,
        model_version: ModelId::new("supplied.credit.central.v1")?,
    })?;
    let keys = CreditRiskRevisions {
        plus_id: "supplied.plus".into(),
        minus_id: "supplied.minus".into(),
        plus_credit: CurveKey {
            id: recipe.curve.key.id.clone(),
            version: "supplied.plus".into(),
        },
        minus_credit: CurveKey {
            id: recipe.curve.key.id.clone(),
            version: "supplied.minus".into(),
        },
        plus_discount: None,
        minus_discount: None,
    };
    let risk = credit_sensitivity(&prepared, &factor, bump, &keys)?;
    validate_credit_risk_vector(std::slice::from_ref(&risk))?;
    println!(
        "{}",
        serde_json::json!({"qualification":"supplied pure engineering example; no production/library certificate","model_notice":convex::credit::STANDARD_MODEL_NOTICE,"credit_curve":prepared.baseline().curve.recipe().key,"native_repricing":prepared.baseline().curve.diagnostics().quotes,"net_pv":prepared.base_value().net_pv,"risk_factor":risk.factor,"cs01_signed_currency_per_bp":risk.value})
    );
    Ok(())
}
