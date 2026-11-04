//! Execute current native calibration/risk from supplied inputs, never references.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[path = "../tests/credit_support/mod.rs"]
mod common;
use common::*;
use convex_analytics::{credit_calibration::*, credit_risk::*};
use convex_core::contracts::{ExactDecimal, FiniteF64, ModelId};
use convex_credit::{price_cds, InitialPaymentState};
use convex_curves::contracts::*;
use serde_json::json;
use std::error::Error;

fn curve_values(
    curve: &BuiltCurve,
    pillars: &[convex_core::Date],
) -> Result<(Vec<f64>, Vec<f64>), Box<dyn Error>> {
    let mut q = Vec::new();
    let mut hazards = Vec::new();
    let mut previous = curve.recipe().reference_date;
    let mut old = 1.0;
    for &date in pillars {
        let value = curve
            .query(&CurveQuery::SurvivalProbability {
                point: CurvePoint::Date(date),
            })?
            .value;
        q.push(value);
        hazards.push((old / value).ln() / (previous.days_between(&date) as f64 / 365.0));
        old = value;
        previous = date;
    }
    Ok((q, hazards))
}
fn fit_report(calibrated: &CalibratedCdsCurve) -> Result<serde_json::Value, Box<dyn Error>> {
    let (q, h) = curve_values(&calibrated.curve, &calibrated.recipe.pillars)?;
    let global = match &calibrated.solver {
        CreditSolverDiagnostics::Global(d) => {
            json!({"rank":d.rank,"scaled_rms":d.scaled_rms,"iterations":d.iterations,"evaluations":d.evaluations})
        }
        _ => serde_json::Value::Null,
    };
    Ok(
        json!({"survival_probabilities":q,"hazards_per_act365f_year":h,"repricing":calibrated.curve.diagnostics().quotes,"recipe":calibrated.recipe,"price_evaluations":calibrated.price_evaluations,"global":global}),
    )
}
fn main() -> Result<(), Box<dyn Error>> {
    let data = inputs()?;
    let recipe = recipe(&data)?;
    let cal = calendar(&data);
    let supplied = supplied(&data)?;
    let helper_calendars = calendars(&recipe, &cal);
    let trade = trade(&recipe, &cal, 5, "0.01")?;
    let context = trade
        .terms()
        .trade_context(InitialPaymentState::NotContracted);
    let prepared = prepare_credit_risk(CreditRiskInput {
        recipe: &recipe,
        helper_calendars: &helper_calendars,
        supplied: &supplied,
        trade: &trade,
        trade_calendars: binding(trade.terms(), &cal),
        context: &context,
        controls: FitNumericalControls::default(),
        risk_model: ModelId::new("supplied.credit.central.v1")?,
    })?;
    let base_curves = supplied.with_curve(prepared.baseline().curve.clone())?;
    let mut experiments = Vec::new();
    for e in &data.experiments {
        let scenario = if e.id == "base" {
            None
        } else {
            let shock = if e.id.starts_with("rates_") {
                CreditShock::DiscountZero {
                    output_discount: CurveKey {
                        id: trade.terms().discount_curve.id.clone(),
                        version: e.id.clone(),
                    },
                    shift_decimal: e.discount_shift_decimal,
                    credit_response: if e.hold_survival {
                        CreditResponse::HoldSurvival
                    } else {
                        CreditResponse::RecalibrateQuotes
                    },
                }
            } else if e.id.starts_with("recovery_") {
                CreditShock::Recovery {
                    fraction: FiniteF64::new(e.recovery_fraction)?,
                    credit_response: if e.hold_survival {
                        CreditResponse::HoldSurvival
                    } else {
                        CreditResponse::RecalibrateQuotes
                    },
                }
            } else {
                let mut moves = Vec::new();
                for (i, &quote) in e.running_spreads_decimal.iter().enumerate() {
                    let delta = quote - data.running_spreads_decimal[i];
                    if delta != 0.0 {
                        let h = &recipe.helpers[i];
                        moves.push(CdsQuoteMove {
                            helper_id: h.id.clone(),
                            expected_quote_version: h.quote_version,
                            delta: h.quote.model_rate_quote(delta)?,
                        });
                    }
                }
                CreditShock::Quotes { moves }
            };
            Some(evaluate_credit_scenario(
                &prepared,
                &CreditScenarioRequest {
                    id: e.id.clone(),
                    output_credit: CurveKey {
                        id: recipe.curve.key.id.clone(),
                        version: e.id.clone(),
                    },
                    shock,
                },
            )?)
        };
        let curve = scenario
            .as_ref()
            .map_or(&prepared.baseline().curve, |s| &s.credit_curve);
        let curves = scenario.as_ref().map_or(&base_curves, |s| &s.curves);
        let discount = scenario
            .as_ref()
            .map_or(&trade.terms().discount_curve, |s| &s.shocked.discount_curve);
        let (q, hazards) = curve_values(curve, &data.pillars)?;
        let mut valuations = Vec::new();
        for i in 0..6 {
            for coupon in ["0.01", "0.05"] {
                let mut terms = recipe.helpers[i].terms.clone();
                terms.survival_curve = curve.recipe().key.clone();
                terms.discount_curve = discount.clone();
                terms.reference.recovery_fraction = FiniteF64::new(e.recovery_fraction)?;
                terms.coupon_decimal = ExactDecimal::parse(coupon)?;
                let contract = terms.build(&binding(&terms, &cal))?;
                let v = price_cds(
                    &contract,
                    &terms.trade_context(InitialPaymentState::NotContracted),
                    curves,
                )?;
                valuations.push(json!({"pillar_index":i,"coupon_decimal":coupon.parse::<f64>()?,"clean_upfront_fraction":v.clean_upfront_fraction,"dirty_upfront_fraction":v.dirty_upfront_fraction,"net_pv":v.net_pv}));
            }
        }
        let calibration = if let Some(s) = &scenario {
            s.calibration.as_ref()
        } else {
            Some(prepared.baseline())
        };
        experiments.push(json!({"id":e.id,"survival_probabilities":q,"hazards_per_act365f_year":hazards,"valuations":valuations,"calibration":calibration.map(fit_report).transpose()?,"recovery_fraction":e.recovery_fraction,"discount_shift_decimal":e.discount_shift_decimal,"hold_survival":e.hold_survival,"running_spreads_decimal":e.running_spreads_decimal}));
    }
    let mut fits = Vec::new();
    for (id, recipe) in [
        ("running_global", global(&recipe)),
        ("mixed_piecewise", mixed(&recipe)?),
        ("mixed_global", global(&mixed(&recipe)?)),
    ] {
        let bindings = calendars(&recipe, &cal);
        let output = build_credit_curve_set(
            &[CdsCurveBuildInput {
                recipe: &recipe,
                calendars: &bindings,
            }],
            &supplied,
            FitNumericalControls::default(),
        )?;
        let c = output.calibrations.first().ok_or("missing actual fit")?;
        fits.push(json!({"id":id,"fit":fit_report(c)?}));
    }
    let mut sensitivities = Vec::new();
    for response in [
        CreditResponse::HoldSurvival,
        CreditResponse::RecalibrateQuotes,
    ] {
        let suffix = if response == CreditResponse::HoldSurvival {
            "hold"
        } else {
            "recalibrate"
        };
        for (id, bump) in [
            (
                format!("rates_{suffix}"),
                CreditRiskBump::DiscountZero {
                    step_decimal: 0.0001,
                    credit_response: response,
                },
            ),
            (
                format!("recovery_{suffix}"),
                CreditRiskBump::Recovery {
                    step_fraction: 0.01,
                    credit_response: response,
                },
            ),
        ] {
            let factor = factor(&recipe, &bump)?;
            let keys = revisions(
                &recipe,
                &id,
                matches!(bump, CreditRiskBump::DiscountZero { .. }),
            );
            let value = credit_sensitivity(&prepared, &factor, bump, &keys)?;
            sensitivities.push(json!({"id":id,"factor":value.factor,"measure":value.measure,"value":value.value,"bump":value.bump}));
        }
    }
    for i in 0..6 {
        let id = format!("quote_{i}");
        let bump = CreditRiskBump::RunningQuote {
            helper_id: recipe.helpers[i].id.clone(),
            step_bps: 1.0,
        };
        let value = credit_sensitivity(
            &prepared,
            &factor(&recipe, &bump)?,
            bump,
            &revisions(&recipe, &id, false),
        )?;
        sensitivities.push(json!({"id":id,"factor":value.factor,"measure":value.measure,"value":value.value,"bump":value.bump}));
    }
    println!(
        "{}",
        json!({"schema_version":1,"engine":"Convex","model_version":"1.8.3","notice":convex_credit::STANDARD_MODEL_NOTICE,"experiments":experiments,"additional_fits":fits,"sensitivities":sensitivities})
    );
    Ok(())
}
