//! Test/example-only supplied inputs. This module never reads expected prices.
#![allow(dead_code)]
use convex_analytics::credit_calibration::*;
use convex_core::{
    calendars::{DynamicCalendar, WeekendType},
    contracts::{ExactDecimal, TradeId, Version},
    Date,
};
use convex_credit::{CalendarBinding, CdsCalendars, CdsContract, CdsTerms, InitialPaymentState};
use convex_curves::contracts::*;
use serde::Deserialize;
use std::error::Error;

pub fn factor(
    recipe: &CdsCurveRecipe,
    bump: &convex_analytics::credit_risk::CreditRiskBump,
) -> Result<convex_core::contracts::RiskFactorKey, Box<dyn Error>> {
    use convex_analytics::credit_risk::CreditRiskBump;
    use convex_core::contracts::*;
    let (family, target, tenor, quote_type, basis) = match bump {
        CreditRiskBump::DiscountZero { .. } => (
            FactorFamily::InterestRate,
            FactorTarget::Curve {
                curve_id: CurveId::new(recipe.helpers[0].terms.discount_curve.id.clone())?,
            },
            "parallel".into(),
            QuoteType::ZeroRate,
            RiskBasis::ZeroNode,
        ),
        CreditRiskBump::Recovery { .. } => (
            FactorFamily::Recovery,
            FactorTarget::Entity {
                entity_id: recipe.helpers[0].terms.reference.entity_id.clone(),
            },
            "parallel".into(),
            QuoteType::RecoveryRate,
            RiskBasis::ModelParameter,
        ),
        CreditRiskBump::RunningQuote { helper_id, .. }
        | CreditRiskBump::UpfrontQuote { helper_id, .. } => {
            let h = recipe
                .helpers
                .iter()
                .find(|h| &h.id == helper_id)
                .ok_or("missing supplied helper")?;
            (
                FactorFamily::CreditSpread,
                FactorTarget::Curve {
                    curve_id: CurveId::new(recipe.curve.key.id.clone())?,
                },
                format!("{}D", recipe.curve.reference_date.days_between(&h.pillar)),
                if matches!(bump, CreditRiskBump::RunningQuote { .. }) {
                    QuoteType::CdsSpread
                } else {
                    QuoteType::CdsUpfront
                },
                RiskBasis::MarketQuoteRecalibrated,
            )
        }
    };
    Ok(RiskFactorKey::new(FactorDefinition {
        family,
        target,
        currency: recipe.curve.currency,
        tenor: FactorTenor::new(tenor)?,
        quote_type,
        basis,
        convention_version: recipe.helpers[0].terms.reference.document.version,
        model_version: ModelId::new("supplied.credit.central.v1")?,
    })?)
}
pub fn revisions(
    recipe: &CdsCurveRecipe,
    label: &str,
    rates: bool,
) -> convex_analytics::credit_risk::CreditRiskRevisions {
    use convex_analytics::credit_risk::CreditRiskRevisions;
    CreditRiskRevisions {
        plus_id: format!("{label}.plus"),
        minus_id: format!("{label}.minus"),
        plus_credit: CurveKey {
            id: recipe.curve.key.id.clone(),
            version: format!("{label}.credit.plus"),
        },
        minus_credit: CurveKey {
            id: recipe.curve.key.id.clone(),
            version: format!("{label}.credit.minus"),
        },
        plus_discount: rates.then(|| CurveKey {
            id: recipe.helpers[0].terms.discount_curve.id.clone(),
            version: format!("{label}.discount.plus"),
        }),
        minus_discount: rates.then(|| CurveKey {
            id: recipe.helpers[0].terms.discount_curve.id.clone(),
            version: format!("{label}.discount.minus"),
        }),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inputs {
    pub schema_version: u32,
    pub qualification: String,
    pub model_version: String,
    pub original_discount_inputs_sha256: String,
    pub template: CdsTerms,
    pub discount_recipe: CurveRecipe,
    pub calendar_holidays: Vec<Date>,
    pub pillars: Vec<Date>,
    pub running_spreads_decimal: Vec<f64>,
    pub experiments: Vec<Experiment>,
    pub bounds: serde_json::Value,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Experiment {
    pub id: String,
    pub discount_shift_decimal: f64,
    pub recovery_fraction: f64,
    pub hold_survival: bool,
    pub running_spreads_decimal: Vec<f64>,
}
pub fn inputs() -> Result<Inputs, Box<dyn Error>> {
    Ok(serde_json::from_str(include_str!(
        "../../../../tests/fixtures/comparisons/credit-isda-1.8.3/inputs.json"
    ))?)
}
pub fn calendar(data: &Inputs) -> DynamicCalendar {
    DynamicCalendar::from_dates(
        "ISDA.NYM.2022",
        WeekendType::SaturdaySunday,
        data.calendar_holidays.clone(),
    )
}
pub fn supplied(data: &Inputs) -> Result<CurveSet, Box<dyn Error>> {
    Ok(build_curve_set(
        std::slice::from_ref(&data.discount_recipe),
        &CurveSet::new(),
    )?
    .curves)
}
pub fn recipe(data: &Inputs) -> Result<CdsCurveRecipe, Box<dyn Error>> {
    let key = CurveKey {
        id: data.template.survival_curve.id.clone(),
        version: "cvx021-supplied-1".into(),
    };
    let mut curve = data.discount_recipe.clone();
    curve.key = key.clone();
    curve.role = CurveRole::Credit {
        entity_id: data.template.reference.entity_id.as_str().into(),
        seniority: data.template.reference.seniority.as_str().into(),
        recovery: data.template.reference.recovery_fraction.get(),
    };
    curve.representation = CurveRepresentation::SurvivalProbabilities;
    curve.dependencies = vec![data.template.discount_curve.clone()];
    curve.calibration = CalibrationMode::Piecewise;
    let mut helpers = Vec::new();
    for (i, (&pillar, &rate)) in data
        .pillars
        .iter()
        .zip(&data.running_spreads_decimal)
        .enumerate()
    {
        let mut terms = data.template.clone();
        terms.survival_curve = key.clone();
        terms.premium_schedule.end_date = pillar;
        terms.trade_id = TradeId::new(format!("Supplied.CDS.Pillar.{i}"))?;
        let quote = match i % 3 {
            0 => MarketQuote {
                value: rate,
                unit: QuoteUnit::DecimalRate,
            },
            1 => MarketQuote {
                value: rate * 10000.0,
                unit: QuoteUnit::BasisPoints,
            },
            _ => MarketQuote {
                value: rate * 100.0,
                unit: QuoteUnit::PercentRate,
            },
        };
        let tolerance = match quote.unit {
            QuoteUnit::BasisPoints => 1e-7,
            QuoteUnit::PercentRate => 1e-9,
            _ => 1e-11,
        };
        helpers.push(CdsCalibrationHelper {
            id: format!("CDS.Pillar.{i}"),
            quote_version: Version::new(100 + i as u64)?,
            pillar,
            context: terms.trade_context(InitialPaymentState::NotContracted),
            terms,
            quote_kind: CdsQuoteKind::RunningSpread,
            quote,
            tolerance,
        });
    }
    curve.source = CurveSource::Helpers(helpers.iter().map(|h| h.id.clone()).collect());
    Ok(CdsCurveRecipe {
        curve,
        pillars: data.pillars.clone(),
        helpers,
        bootstrap: Some(CdsBootstrapSettings {
            lower_hazard: 0.0,
            upper_hazard: 1.0,
            max_iterations: 100,
        }),
    })
}
pub fn calendars<'a>(
    recipe: &'a CdsCurveRecipe,
    cal: &'a DynamicCalendar,
) -> Vec<CdsCalendars<'a>> {
    recipe
        .helpers
        .iter()
        .map(|h| CdsCalendars {
            premium: CalendarBinding {
                pin: &h.terms.premium_calendar,
                calendar: cal,
            },
            settlement: CalendarBinding {
                pin: &h.terms.settlement_calendar,
                calendar: cal,
            },
        })
        .collect()
}
pub fn binding<'a>(terms: &'a CdsTerms, cal: &'a DynamicCalendar) -> CdsCalendars<'a> {
    CdsCalendars {
        premium: CalendarBinding {
            pin: &terms.premium_calendar,
            calendar: cal,
        },
        settlement: CalendarBinding {
            pin: &terms.settlement_calendar,
            calendar: cal,
        },
    }
}
pub fn trade(
    recipe: &CdsCurveRecipe,
    cal: &DynamicCalendar,
    index: usize,
    coupon: &str,
) -> Result<CdsContract, Box<dyn Error>> {
    let mut terms = recipe.helpers[index].terms.clone();
    terms.trade_id = TradeId::new("Supplied.CDS.RiskTrade")?;
    terms.coupon_decimal = ExactDecimal::parse(coupon)?;
    Ok(terms.build(&binding(&terms, cal))?)
}
pub fn global(recipe: &CdsCurveRecipe) -> CdsCurveRecipe {
    let mut r = recipe.clone();
    r.bootstrap = None;
    r.curve.calibration = CalibrationMode::CreditGlobal(CreditGlobalFitSettings {
        max_iterations: 100,
        objective_tolerance: 1e-9,
        initial_hazards: vec![0.02; r.pillars.len()],
        lower_hazard: 0.0,
        upper_hazard: 1.0,
        quote_scales: r
            .helpers
            .iter()
            .map(|h| match h.quote.unit {
                QuoteUnit::BasisPoints => 1.0,
                QuoteUnit::PercentRate => 0.01,
                _ => 0.0001,
            })
            .collect(),
        weights: vec![1.0; r.helpers.len()],
    });
    r
}
pub fn mixed(recipe: &CdsCurveRecipe) -> Result<CdsCurveRecipe, Box<dyn Error>> {
    #[derive(Deserialize)]
    struct Mixed {
        helpers: Vec<MixedHelper>,
    }
    #[derive(Deserialize)]
    struct MixedHelper {
        pillar_index: usize,
        coupon_decimal: String,
        quote_kind: CdsQuoteKind,
        quote: MarketQuote,
        tolerance: f64,
    }
    let data: Mixed = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/comparisons/credit-isda-1.8.3/mixed-inputs.json"
    ))?;
    let mut r = recipe.clone();
    for m in data.helpers {
        let h = &mut r.helpers[m.pillar_index];
        h.terms.coupon_decimal = ExactDecimal::parse(&m.coupon_decimal)?;
        h.quote_kind = m.quote_kind;
        h.quote = m.quote;
        h.tolerance = m.tolerance;
    }
    Ok(r)
}
