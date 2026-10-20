//! Offline synthetic original cash resources shared by the example and tests.
#![allow(dead_code)]

use convex_analytics::{cash_calibration::*, credit_calibration::*};
use convex_bonds::contracts::BondCalendars;
use convex_core::{
    calendars::{DynamicCalendar, WeekendType},
    contracts::{ExactDecimal, Version},
    Date,
};
use convex_credit::{CdsCalendars, CdsTerms, InitialPaymentState};
use convex_curves::contracts::*;
use convex_rates::{CalendarBinding, ReferencePin};
use serde_json::Value;

pub type Error = Box<dyn std::error::Error>;
pub fn text<'a>(v: &'a Value, field: &str) -> Result<&'a str, Error> {
    v[field]
        .as_str()
        .ok_or_else(|| format!("missing {field}").into())
}
pub fn number(v: &Value) -> Result<f64, Error> {
    Ok(v.as_str()
        .ok_or("missing exact independent number")?
        .parse()?)
}
pub struct Fixture {
    pub data: Value,
    pub row: Value,
    pub recipes: Vec<CashCurveRecipe>,
    pub calendar: DynamicCalendar,
    pub calendar_pin: ReferencePin,
    pub direct_funding: Vec<CurveRecipe>,
    pub forwards: Vec<RepoBondForwardTerms>,
}
impl Fixture {
    pub fn load(id: &str) -> Result<Self, Error> {
        let data: Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/curves/cash-curves.json"
        ))?;
        let row = data["cases"]
            .as_array()
            .ok_or("missing cases")?
            .iter()
            .find(|r| r["id"] == id)
            .ok_or("missing case")?
            .clone();
        let holidays: Vec<Date> = serde_json::from_value(row["holidays"].clone())?;
        let calendar = DynamicCalendar::from_dates(
            "supplied synthetic cash v1",
            WeekendType::SaturdaySunday,
            holidays,
        );
        let recipes = serde_json::from_value(row["recipes"].clone())?;
        let direct_funding = serde_json::from_value(row["direct_funding"].clone())?;
        let forwards = row["carry"]
            .as_array()
            .ok_or("missing original carry")?
            .iter()
            .map(|v| serde_json::from_value(v["terms"].clone()))
            .collect::<Result<_, _>>()?;
        let calendar_pin = serde_json::from_value(row["calendar_pin"].clone())?;
        Ok(Self {
            data,
            row,
            recipes,
            calendar,
            calendar_pin,
            direct_funding,
            forwards,
        })
    }
    pub fn supplied(&self) -> Result<CurveSet, Error> {
        Ok(build_curve_set(&self.direct_funding, &CurveSet::new())?.curves)
    }
    pub fn resources<'a>(
        &'a self,
        recipes: &'a [CashCurveRecipe],
    ) -> Vec<Vec<CashHelperResources<'a>>> {
        resources(recipes, &self.calendar)
    }
    pub fn forward_resources<'a>(&'a self, t: &'a RepoBondForwardTerms) -> CashHelperResources<'a> {
        bond_resources(&t.spot, &self.calendar)
    }
    pub fn delivery_calendar(&self) -> CalendarBinding<'_> {
        CalendarBinding {
            pin: &self.calendar_pin,
            calendar: &self.calendar,
        }
    }
}
pub fn bond_resources<'a>(
    h: &'a CashBondHelper,
    c: &'a DynamicCalendar,
) -> CashHelperResources<'a> {
    CashHelperResources::Bond {
        ids: &h.terms.calendars,
        calendars: BondCalendars {
            payment: c,
            reset: c,
            observation: c,
            exercise: c,
        },
    }
}
pub fn resources<'a>(
    recipes: &'a [CashCurveRecipe],
    calendar: &'a DynamicCalendar,
) -> Vec<Vec<CashHelperResources<'a>>> {
    recipes
        .iter()
        .map(|r| {
            r.helpers
                .iter()
                .map(|h| match h {
                    CashCalibrationHelper::Bond { input } => bond_resources(input, calendar),
                    CashCalibrationHelper::Repo { input } => {
                        CashHelperResources::Repo(CalendarBinding {
                            pin: &input.calendar,
                            calendar,
                        })
                    }
                })
                .collect()
        })
        .collect()
}
pub fn build(
    recipes: &[CashCurveRecipe],
    resources: &[Vec<CashHelperResources<'_>>],
    supplied: &CurveSet,
) -> Result<CashCurveBuildOutput, CashCalibrationError> {
    let inputs: Vec<_> = recipes
        .iter()
        .zip(resources)
        .map(|(recipe, resources)| CashCurveBuildInput { recipe, resources })
        .collect();
    build_cash_curve_set(&inputs, supplied, FitNumericalControls::default())
}
pub fn global(recipes: &[CashCurveRecipe]) -> Vec<CashCurveRecipe> {
    recipes
        .iter()
        .map(|r| {
            let mut r = r.clone();
            r.bootstrap = None;
            let scales = r
                .helpers
                .iter()
                .map(|h| match h.quote().unit {
                    QuoteUnit::DecimalRate => 0.01,
                    QuoteUnit::PercentRate => 1.,
                    QuoteUnit::BasisPoints => 100.,
                    _ => 1.,
                })
                .collect();
            r.curve.calibration = CalibrationMode::Global(GlobalFitSettings {
                max_iterations: 256,
                objective_tolerance: 1e-12,
                initial_zeros: vec![0.; r.pillars.len()],
                lower_zero: -0.05,
                upper_zero: 0.1,
                quote_scales: scales,
                weights: vec![1.; r.helpers.len()],
            });
            r
        })
        .collect()
}
pub fn compare_price(
    value: &convex_analytics::bond_contract::ContractPrice,
    expected: &Value,
    tolerance: &Value,
) -> Result<(), Error> {
    let absolute = tolerance["per100_absolute"]
        .as_f64()
        .ok_or("missing absolute tolerance")?;
    let relative = tolerance["relative"]
        .as_f64()
        .ok_or("missing relative tolerance")?;
    for (actual, field) in [
        (value.clean_price, "clean_per100"),
        (value.dirty_price, "dirty_per100"),
        (value.accrued, "accrued_per100"),
    ] {
        compare(actual, number(&expected[field])?, absolute, relative)?;
    }
    let coupons = expected["coupons"]
        .as_array()
        .ok_or("missing independent coupons")?;
    let principal = expected["principal"]
        .as_array()
        .ok_or("missing independent principal")?;
    if coupons.len() != value.projection.coupons.len()
        || principal.len() != value.projection.principal.len()
    {
        return Err("actual cash counts differ".into());
    }
    let cash_absolute = tolerance["currency_absolute"]
        .as_f64()
        .ok_or("missing cash tolerance")?;
    for (actual, expected) in value.projection.coupons.iter().zip(coupons) {
        if actual.payment_date.to_string() != text(expected, "payment_date")?
            || actual.accrual_start.to_string() != text(expected, "start")?
            || actual.accrual_end.to_string() != text(expected, "end")?
            || !actual.entitled
        {
            return Err("actual coupon dates/entitlement differ".into());
        }
        compare(
            ExactDecimal::new(actual.amount).to_model()?.get(),
            number(&expected["amount"])?,
            cash_absolute,
            relative,
        )?;
        compare(
            ExactDecimal::new(actual.accrued).to_model()?.get(),
            number(&expected["accrued"])?,
            cash_absolute,
            relative,
        )?;
    }
    for (actual, expected) in value.projection.principal.iter().zip(principal) {
        if actual.payment_date.to_string() != text(expected, "payment_date")?
            || actual.effective_date.to_string() != text(expected, "effective_date")?
        {
            return Err("actual principal dates differ".into());
        }
        compare(
            ExactDecimal::new(actual.amount).to_model()?.get(),
            number(&expected["amount"])?,
            cash_absolute,
            relative,
        )?;
    }
    Ok(())
}
pub fn compare(actual: f64, expected: f64, absolute: f64, relative: f64) -> Result<(), Error> {
    if !actual.is_finite() || (actual - expected).abs() > absolute + relative * expected.abs() {
        return Err(format!(
            "actual {actual:.15} expected {expected:.15} bound {absolute}+{relative}relative"
        )
        .into());
    }
    Ok(())
}
pub fn compare_reference(f: &Fixture, output: &CashCurveBuildOutput) -> Result<(), Error> {
    let tol = &f.data["tolerances"];
    if output.calibrations.len() != f.recipes.len() {
        return Err("complete independent curve count differs".into());
    }
    for calibration in &output.calibrations {
        let expected = f.row["expected_parameters"][&calibration.curve.recipe().key.id]
            .as_array()
            .ok_or("missing independent curve parameters")?;
        if calibration.annualized_parameters.len() != expected.len() {
            return Err("complete independent parameter count differs".into());
        }
        for (&actual, expected) in calibration.annualized_parameters.iter().zip(expected) {
            compare(
                actual,
                number(expected)?,
                tol["parameter_absolute"]
                    .as_f64()
                    .ok_or("missing parameter tolerance")?,
                tol["parameter_relative"]
                    .as_f64()
                    .ok_or("missing relative tolerance")?,
            )?;
        }
        let group = match calibration.curve.recipe().role {
            CurveRole::Government { .. } => Some("government_expected"),
            CurveRole::IssuerSpread { .. } => Some("basis_expected"),
            _ => None,
        };
        if let Some(group) = group {
            let original = f.row[group]
                .as_array()
                .ok_or("missing actual original cash")?;
            if calibration.repriced_helpers.len() != original.len() {
                return Err("complete original independent cash helper count differs".into());
            }
            for (actual, expected) in calibration.repriced_helpers.iter().zip(original) {
                let CashHelperValuation::Bond { value, .. } = actual else {
                    return Err("missing original bond model price".into());
                };
                compare_price(value, expected, tol)?;
            }
        }
    }
    Ok(())
}

pub struct CreditFixture {
    pub data: Value,
    pub recipe: CashCurveRecipe,
    pub calendar: DynamicCalendar,
    pub supplied: CurveSet,
    pub cds: CdsCurveRecipe,
}
impl CreditFixture {
    pub fn load() -> Result<Self, Error> {
        let data: Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/curves/cash-curves.json"
        ))?;
        let source: Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/comparisons/credit-isda-1.8.3/inputs.json"
        ))?;
        let template: CdsTerms = serde_json::from_value(source["template"].clone())?;
        let pillars: Vec<Date> = serde_json::from_value(source["pillars"].clone())?;
        let spreads: Vec<f64> = serde_json::from_value(source["running_spreads_decimal"].clone())?;
        let discount: CurveRecipe = serde_json::from_value(source["discount_recipe"].clone())?;
        let holidays: Vec<Date> = serde_json::from_value(source["calendar_holidays"].clone())?;
        let calendar = DynamicCalendar::from_dates(
            "supplied original NYM",
            WeekendType::SaturdaySunday,
            holidays,
        );
        let mut helpers = Vec::new();
        for (i, (&pillar, &spread)) in pillars.iter().zip(&spreads).enumerate() {
            let mut terms = template.clone();
            terms.premium_schedule.end_date = pillar;
            helpers.push(CdsCalibrationHelper {
                id: format!("original.cds.{i}"),
                quote_version: Version::new(1)?,
                pillar,
                context: terms.trade_context(InitialPaymentState::NotContracted),
                terms,
                quote_kind: CdsQuoteKind::RunningSpread,
                quote: MarketQuote {
                    value: spread * 10000.,
                    unit: QuoteUnit::BasisPoints,
                },
                tolerance: 1e-7,
            });
        }
        let mut curve = discount.clone();
        curve.key = template.survival_curve.clone();
        curve.role = CurveRole::Credit {
            entity_id: template.reference.entity_id.as_str().into(),
            seniority: template.reference.seniority.as_str().into(),
            recovery: template.reference.recovery_fraction.get(),
        };
        curve.representation = CurveRepresentation::SurvivalProbabilities;
        curve.calibration = CalibrationMode::Piecewise;
        curve.source = CurveSource::Helpers(helpers.iter().map(|h| h.id.clone()).collect());
        curve.dependencies = vec![discount.key.clone()];
        let cds = CdsCurveRecipe {
            curve,
            pillars,
            helpers,
            bootstrap: Some(CdsBootstrapSettings {
                lower_hazard: 0.,
                upper_hazard: 1.,
                max_iterations: 100,
            }),
        };
        let bindings: Vec<_> = cds
            .helpers
            .iter()
            .map(|h| CdsCalendars {
                premium: convex_credit::CalendarBinding {
                    pin: &h.terms.premium_calendar,
                    calendar: &calendar,
                },
                settlement: convex_credit::CalendarBinding {
                    pin: &h.terms.settlement_calendar,
                    calendar: &calendar,
                },
            })
            .collect();
        let supplied = build_curve_set(&[discount], &CurveSet::new())?.curves;
        let supplied = build_credit_curve_set(
            &[CdsCurveBuildInput {
                recipe: &cds,
                calendars: &bindings,
            }],
            &supplied,
            FitNumericalControls::default(),
        )?
        .curves;
        let recipe = serde_json::from_value(data["credit_case"]["recipe"].clone())?;
        Ok(Self {
            data,
            recipe,
            calendar,
            supplied,
            cds,
        })
    }
}
