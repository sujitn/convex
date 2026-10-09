//! Full original calibration resources and native graph dispatch.
use super::inputs::ResolvedInputs;
use super::*;
use convex_analytics::{
    cash_calibration as c, credit_calibration as d, quote_risk as q, rate_calibration as r,
};
use convex_core::contracts::*;
use serde::{Deserialize, Serialize};

/// Exact native date/model-time conversion, independent of business-day rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CurveTimeOperation {
    /// Convert an actual date with the original curve's time basis.
    DateToTenor {
        /// Actual calendar date, on or after the curve reference.
        date: Date,
    },
    /// Bounded inverse to a canonical date; rounding is never inferred.
    TenorToDate {
        /// Actual model tenor in years of the declared curve time basis.
        tenor: f64,
        /// Original native inverse rounding rule.
        rounding: TenorRounding,
    },
}
/// Annualization period is explicit, especially for Simple compounding.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RateConversionPeriod {
    /// Actual accrual dates and a supported context-free native day count.
    Dated {
        /// Accrual start, without an inferred settlement rule.
        start: Date,
        /// Accrual end, strictly after start.
        end: Date,
        /// Actual annualization basis; missing ICMA references are unsupported.
        day_count: convex_core::daycounts::DayCountConvention,
    },
    /// Supplied model year fraction with the original native span/domain guard.
    YearFraction {
        /// Nonnegative actual model fraction; inverse rates require positive time.
        value: f64,
    },
}
/// Original stable log/exponential conversions; negative rates retain domains.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RateConversionOperation {
    /// Convert an annualized decimal rate to the actual period discount.
    DiscountFromRate {
        /// Original finite decimal rate.
        rate: f64,
        /// Actual input compounding.
        compounding: convex_core::types::Compounding,
    },
    /// Convert a positive finite period discount to an annualized decimal rate.
    RateFromDiscount {
        /// Actual period discount; negative rates may give values above one.
        discount: f64,
        /// Explicit output compounding.
        compounding: convex_core::types::Compounding,
    },
    /// Equivalent annualized decimal rate over the same actual period.
    ConvertRate {
        /// Original finite decimal rate.
        rate: f64,
        /// Actual input compounding.
        from: convex_core::types::Compounding,
        /// Explicit output compounding.
        to: convex_core::types::Compounding,
    },
}
pub(super) fn curve_time(curve: &BuiltCurve, action: &CurveTimeOperation) -> LibraryResult<Value> {
    let (result, unit) = match *action {
        CurveTimeOperation::DateToTenor { date } => {
            (value(curve.date_to_tenor(date)?)?, "curve_time_years")
        }
        CurveTimeOperation::TenorToDate { tenor, rounding } => (
            value(curve.tenor_to_date(tenor, rounding)?)?,
            "calendar_date",
        ),
    };
    Ok(
        serde_json::json!({"curve":curve.recipe().key,"reference_date":curve.recipe().reference_date,
        "time_basis":curve.recipe().time_basis,"action":action,"value":result,"unit":unit}),
    )
}
pub(super) fn rate_conversion(
    period: &RateConversionPeriod,
    action: &RateConversionOperation,
) -> LibraryResult<Value> {
    let fraction = match *period {
        RateConversionPeriod::Dated {
            start,
            end,
            day_count,
        } => year_fraction(start, end, day_count)?,
        RateConversionPeriod::YearFraction { value } => value,
    };
    let (result, unit) = match *action {
        RateConversionOperation::DiscountFromRate { rate, compounding } => (
            discount_from_rate(rate, fraction, compounding)?,
            "discount_factor",
        ),
        RateConversionOperation::RateFromDiscount {
            discount,
            compounding,
        } => (
            rate_from_discount(discount, fraction, compounding)?,
            "decimal_rate",
        ),
        RateConversionOperation::ConvertRate { rate, from, to } => {
            (convert_rate(rate, fraction, from, to)?, "decimal_rate")
        }
    };
    Ok(
        serde_json::json!({"period":period,"year_fraction":fraction,"action":action,"value":result,"unit":unit}),
    )
}

/// Original rate recipe plus actual immutable source quote revisions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RateCalibrationInput {
    /// Full dated helpers, mappings, numerical method and native tolerances.
    pub recipe: r::RateCurveRecipe,
    /// One actual immutable revision per helper in original source order.
    pub quote_versions: Vec<Version>,
    /// Actual original calendar stamp for each futures helper; None otherwise.
    pub futures_calendar_stamps: Vec<Option<convex_core::futures::ReferenceStamp>>,
}
/// Actual resources corresponding to one original cash helper.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CashHelperCalendars {
    /// Original independent bond calendar revisions.
    Bond {
        /// Four actual pins, no supplied terms are reduced.
        pins: BondCalendarPins,
    },
    /// Actual original loan calendar.
    Repo {
        /// Native identity/version checked against the loan.
        pin: convex_rates::ReferencePin,
    },
}
/// Original cash recipe with its actual matched helper calendars.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CashCalibrationInput {
    /// Full original securities/loans, role/model/quote settings and dependencies.
    pub recipe: c::CashCurveRecipe,
    /// One actual resource shape per helper, preserving original order.
    pub calendars: Vec<CashHelperCalendars>,
}
/// Original calibration economic/history instant, independent of system time.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CalibrationContext {
    /// Full original valuation/payment/fixing eligibility policy.
    pub cashflows: convex_rates::CashflowContext,
    /// Actual supplied economic instant.
    pub as_of: UtcTimestamp,
}
/// Complete rate/CDS/government/issuer-basis/repo graph over supplied dependencies.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CalibrationInput {
    /// Full original rate curve recipes and quote revisions.
    pub rates: Vec<RateCalibrationInput>,
    /// Full original single-name CDS recipes, calendars inside terms.
    pub credit: Vec<d::CdsCurveRecipe>,
    /// Full original securities/loans and actual calendar revisions.
    pub cash: Vec<CashCalibrationInput>,
    /// Actual required time policy for rate recipes.
    pub rate_context: Option<CalibrationContext>,
    /// Original identification/Jacobian/SVD controls.
    pub controls: FitNumericalControls,
}
/// Full native graph build, scenario or product risk; quote rebuild is never a
/// calibrated-node shortcut and all native helper residual guards still apply.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CalibrationOperation {
    /// Full actual build plus original native repricing/work/publication evidence.
    Build {},
    /// Complete native valuation on the original actually rebuilt quote graph.
    Price {
        /// Full original product and quotation state.
        product: Box<super::risk::RiskProductInput>,
    },
    /// Original exact quote/node/recovery scenario and dependency propagation.
    Scenario {
        /// Full native factor/unit/model/response key.
        key: q::QuoteRiskKey,
        /// Actual move in the key's original native unit.
        delta: f64,
    },
    /// Actual product signed graph risk with original five-PV/half-step guards.
    ProductRisk {
        /// Full original product and quotation state.
        product: Box<super::risk::RiskProductInput>,
        /// Full actual curve/quote/convention/model/unit key.
        key: q::QuoteRiskKey,
        /// Signed native quantity/reference units, scaled once.
        position: q::RiskPosition,
        /// Actual caller snapshot identity.
        snapshot_id: SnapshotId,
        /// Original bump/half-step stability/work admission.
        settings: q::QuoteRiskSettings,
    },
    /// Full native product repricing on one complete graph scenario.
    ProductScenario {
        /// Original full product and quotation.
        product: Box<super::risk::RiskProductInput>,
        /// Full explicit scenario key.
        key: q::QuoteRiskKey,
        /// Original native-unit move.
        delta: f64,
        /// Exact signed native quantity.
        position: q::RiskPosition,
        /// Actual immutable snapshot identity.
        snapshot_id: SnapshotId,
    },
    /// Reprice each supplied bucket and the simultaneous move using the same
    /// original native product/graph, then enforce exact native membership.
    ReconcileBuckets {
        /// Complete original product, factor manifest and numerical policy.
        input: Box<super::risk::BucketReconciliationInput>,
    },
}

pub(super) fn describe_curves(curves: &CurveSet) -> LibraryResult<Value> {
    let mut result = Vec::with_capacity(curves.len());
    for key in curves.keys() {
        let curve = curves.get(key)?;
        result.push(describe_curve(curve));
    }
    Ok(serde_json::json!({"curves":result}))
}
pub(super) fn describe_curve(curve: &BuiltCurve) -> Value {
    let grid = match curve.interpolation_grid() {
        Ok((tenors, values)) => serde_json::json!({"tenors":tenors,"values":values}),
        Err(e) => serde_json::json!({"status":"unsupported","reason":e.to_string()}),
    };
    let mut result = serde_json::json!({"recipe":curve.recipe(),"diagnostics":curve.diagnostics(),"domain":curve.domain(),"interpolation_grid":grid});
    if let Some(structure) = curve.structured_diagnostics() {
        result["structure"] = serde_json::json!(structure);
    }
    if let Some(tail) = curve.ufr_diagnostics() {
        result["ufr"] = serde_json::json!(tail);
    }
    if let Some(model) = curve.monotone_diagnostics() {
        result["monotone"] = serde_json::json!(model);
    }
    result
}
pub(super) fn fit_parametric(
    recipe: &CurveRecipe,
    settings: &ParametricFitSettings,
) -> LibraryResult<Value> {
    let out =
        convex_curves::contracts::fit_parametric_curve(recipe, settings).map_err(|e| match e {
            convex_curves::contracts::CurveFitError::Contract(e) => LibraryError::from(e),
            convex_curves::contracts::CurveFitError::Solver(e) => super::errors::fit_error(*e),
        })?;
    Ok(
        serde_json::json!({"input_recipe":out.input_recipe,"settings":out.settings,"curve":describe_curve(&out.curve),"solver":super::errors::fit_diagnostics(&out.solver)}),
    )
}
pub(super) fn fit_curve(
    recipe: &CurveRecipe,
    pillars: &[Date],
    controls: FitNumericalControls,
) -> LibraryResult<Value> {
    let out = convex_curves::contracts::fit_money_market_curve(recipe, pillars, controls).map_err(
        |e| match e {
            CurveFitError::Contract(e) => LibraryError::from(e),
            CurveFitError::Solver(e) => super::errors::fit_error(*e),
        },
    )?;
    let d = out.solver;
    let diagnostics = serde_json::json!({"iterations":d.iterations,"evaluations":d.evaluations,
        "scaled_rms":d.scaled_rms,"native_residuals":d.native_residuals,"rank":d.rank,
        "condition_estimate":d.condition_estimate,"active_bounds":d.active_bounds});
    let curves = CurveSet::new().with_curve(out.curve)?;
    Ok(serde_json::json!({"result":describe_curves(&curves)?,"solver":diagnostics}))
}
pub(super) fn graph_value(graph: &q::QuoteGraphScenario) -> LibraryResult<Value> {
    let mut output = serde_json::json!({"result":describe_curves(&graph.curves)?,"build_order":graph.build_order,
        "repricing":graph.repricing,"price_evaluations":graph.price_evaluations,"publications":graph.publications});
    // Original grid/credit output bytes remain unchanged. Parametric product
    // graphs additionally retain their mixed-unit solved parameters and full
    // measured solver/helper evidence, including on dependent quote rebuilds.
    if graph.calibrations.iter().any(|fit| match fit {
        q::QuoteGraphCalibration::Rate(f) => {
            matches!(
                f.solver,
                r::RateSolverDiagnostics::Parametric { .. } | r::RateSolverDiagnostics::Ufr { .. }
            )
        }
        q::QuoteGraphCalibration::Cash(f) => {
            matches!(
                f.solver,
                c::CashSolverDiagnostics::Parametric { .. } | c::CashSolverDiagnostics::Ufr { .. }
            )
        }
        q::QuoteGraphCalibration::Credit(_) => false,
    }) {
        output["calibrations"] = value(
            graph
                .calibrations
                .iter()
                .map(super::replay::fit_value)
                .collect::<Vec<_>>(),
        )?;
    }
    Ok(output)
}
fn rate_histories(
    rate: &RateCalibrationInput,
    resources: &ResolvedInputs<'_>,
) -> LibraryResult<Vec<Option<convex_analytics::futures::FuturesFixings>>> {
    if rate.recipe.helpers.len() > r::MAX_RATE_FIT_HELPERS {
        return Err(LibraryError::limit("at most128 complete rate helpers"));
    }
    if rate.quote_versions.len() != rate.recipe.helpers.len() {
        return Err(LibraryError::invalid(
            "quote_versions",
            "one exact revision per original helper required",
        ));
    }
    if rate.futures_calendar_stamps.len() != rate.recipe.helpers.len() {
        return Err(LibraryError::invalid(
            "futures_calendar_stamps",
            "one entry per original rate helper required",
        ));
    }
    let mut histories = Vec::new();
    for helper in &rate.recipe.helpers {
        histories.push(match &helper.terms {
            r::RateCalibrationTerms::RateFuture(f) => {
                Some(convex_analytics::futures::FuturesFixings::new(
                    resources.fixing_observations().to_vec(),
                    f.clearing.fixing_selection.ok_or_else(|| {
                        LibraryError::invalid("clearing", "missing original revision selection")
                    })?,
                )?)
            }
            _ => None,
        });
    }
    Ok(histories)
}
fn rate_bindings<'a>(
    rate: &'a RateCalibrationInput,
    histories: &'a [Option<convex_analytics::futures::FuturesFixings>],
    resources: &'a ResolvedInputs<'_>,
) -> LibraryResult<Vec<r::RateHelperResources<'a>>> {
    let mut bindings = Vec::new();
    for (hi, helper) in rate.recipe.helpers.iter().enumerate() {
        let stamp = &rate.futures_calendar_stamps[hi];
        if !matches!(&helper.terms, r::RateCalibrationTerms::RateFuture(_)) && stamp.is_some() {
            return Err(LibraryError::invalid(
                "futures_calendar_stamps",
                "non-future helper cannot consume future calendar stamp",
            ));
        }
        bindings.push(match &helper.terms {
            r::RateCalibrationTerms::MoneyMarket(m) => {
                r::RateHelperResources::MoneyMarket(resources.rate_binding(&m.calendar)?)
            }
            r::RateCalibrationTerms::Swap(t) => {
                r::RateHelperResources::Swap(resources.swap_calendars(t)?)
            }
            r::RateCalibrationTerms::BasisSwap { terms, .. } => {
                r::RateHelperResources::BasisSwap(resources.basis_calendars(terms)?)
            }
            r::RateCalibrationTerms::RateFuture(_) => r::RateHelperResources::RateFuture {
                calendar: super::products::future_calendar(
                    stamp.as_ref().ok_or_else(|| {
                        LibraryError::missing("future calendar stamp", Value::Null)
                    })?,
                    resources,
                )?,
                fixings: histories[hi].as_ref().ok_or_else(|| {
                    LibraryError::missing("original clearing history", Value::Null)
                })?,
            },
        });
    }
    Ok(bindings)
}
pub(super) fn reprice_rate_helpers(
    input: &RateCalibrationInput,
    context: &CalibrationContext,
    resources: &ResolvedInputs<'_>,
) -> LibraryResult<Value> {
    let histories = rate_histories(input, resources)?;
    let bindings = rate_bindings(input, &histories, resources)?;
    let result = r::reprice_rate_helpers(
        &r::RateCurveBuildInput {
            recipe: &input.recipe,
            resources: &bindings,
        },
        &resources.curves,
        &resources.history,
        r::RateCalibrationContext {
            cashflows: context.cashflows,
            as_of: context.as_of,
        },
        resources.publications.callback(),
    )?;
    Ok(serde_json::json!({"repricing": result, "quote_versions": input.quote_versions}))
}
pub(super) fn with_graph<T>(
    input: &CalibrationInput,
    resources: &ResolvedInputs<'_>,
    consume: impl FnOnce(&q::PreparedQuoteRiskGraph<'_>) -> LibraryResult<T>,
) -> LibraryResult<T> {
    if input.rates.len() + input.credit.len() + input.cash.len() > 256 {
        return Err(LibraryError::limit("at most256 curve recipes"));
    }
    // Each existing native builder is borrowed intact into the original graph.
    let future_histories = input
        .rates
        .iter()
        .map(|rate| rate_histories(rate, resources))
        .collect::<LibraryResult<Vec<_>>>()?;
    let rate_resources = input
        .rates
        .iter()
        .zip(&future_histories)
        .map(|(rate, histories)| rate_bindings(rate, histories, resources))
        .collect::<LibraryResult<Vec<_>>>()?;
    let rates = input
        .rates
        .iter()
        .zip(&rate_resources)
        .map(|(i, resources)| q::RateRiskCurveInput {
            build: r::RateCurveBuildInput {
                recipe: &i.recipe,
                resources,
            },
            quote_versions: &i.quote_versions,
        })
        .collect::<Vec<_>>();
    let credit_calendars = input
        .credit
        .iter()
        .map(|i| {
            i.helpers
                .iter()
                .map(|h| resources.credit_calendars(&h.terms))
                .collect::<LibraryResult<Vec<_>>>()
        })
        .collect::<LibraryResult<Vec<_>>>()?;
    let credit = input
        .credit
        .iter()
        .zip(&credit_calendars)
        .map(|(recipe, calendars)| d::CdsCurveBuildInput { recipe, calendars })
        .collect::<Vec<_>>();
    let cash_resources = input
        .cash
        .iter()
        .map(|i| cash_resources(i, resources))
        .collect::<LibraryResult<Vec<_>>>()?;
    let cash = input
        .cash
        .iter()
        .zip(&cash_resources)
        .map(|(i, resources)| c::CashCurveBuildInput {
            recipe: &i.recipe,
            resources,
        })
        .collect::<Vec<_>>();
    let rate_context = input
        .rate_context
        .as_ref()
        .map(|c| r::RateCalibrationContext {
            cashflows: c.cashflows,
            as_of: c.as_of,
        });
    let graph = q::prepare_quote_risk_graph(
        q::QuoteRiskGraphInput {
            rates: &rates,
            credit: &credit,
            cash: &cash,
            supplied: &resources.curves,
            history: &resources.history,
            rate_context,
            controls: input.controls,
        },
        resources.publications.callback(),
    )?;
    consume(&graph)
}
/// Resolve full cash helper calendars for fitting and direct diagnostics alike.
pub(super) fn cash_resources<'a>(
    input: &'a CashCalibrationInput,
    resources: &ResolvedInputs<'a>,
) -> LibraryResult<Vec<c::CashHelperResources<'a>>> {
    if input.recipe.helpers.len() > c::MAX_CASH_HELPERS {
        return Err(LibraryError::limit("at most128 complete cash helpers"));
    }
    if input.calendars.len() != input.recipe.helpers.len() {
        return Err(LibraryError::invalid(
            "cash calendars",
            "one exact resource shape per original helper required",
        ));
    }
    input
        .recipe
        .helpers
        .iter()
        .zip(&input.calendars)
        .map(|(h, calendars)| match (h, calendars) {
            (c::CashCalibrationHelper::Bond { input: h }, CashHelperCalendars::Bond { pins }) => {
                Ok(c::CashHelperResources::Bond {
                    ids: &h.terms.calendars,
                    calendars: resources.bond_calendars(&BondInput {
                        terms: h.terms.clone(),
                        calendars: pins.clone(),
                    })?,
                })
            }
            (c::CashCalibrationHelper::Repo { .. }, CashHelperCalendars::Repo { pin }) => {
                Ok(c::CashHelperResources::Repo(resources.rate_binding(pin)?))
            }
            _ => Err(LibraryError::invalid(
                "cash calendars",
                "full original helper/resource family mismatch",
            )),
        })
        .collect()
}
pub(super) fn calibration_market(
    input: &CalibrationInput,
    resources: &ResolvedInputs<'_>,
) -> LibraryResult<(CurveSet, Value)> {
    with_graph(input, resources, |g| {
        Ok((g.baseline().curves.clone(), graph_value(g.baseline())?))
    })
}
pub(super) fn calibration(
    input: &CalibrationInput,
    action: &CalibrationOperation,
    resources: &ResolvedInputs<'_>,
) -> LibraryResult<Value> {
    with_graph(input, resources, |graph| match action {
        CalibrationOperation::Build {} => graph_value(graph.baseline()),
        CalibrationOperation::Price { product } => {
            let valuation = super::risk::graph_price(product, graph.baseline(), resources)?;
            Ok(serde_json::json!({"valuation":valuation,"graph":graph_value(graph.baseline())?}))
        }
        CalibrationOperation::Scenario { key, delta } => {
            graph_value(&q::evaluate_quote_graph(graph, key, *delta)?)
        }
        CalibrationOperation::ProductRisk {
            product,
            key,
            position,
            snapshot_id,
            settings,
        } => super::risk::graph_risk(
            product,
            graph,
            key.clone(),
            position.clone(),
            snapshot_id.clone(),
            settings.clone(),
            resources,
        ),
        CalibrationOperation::ProductScenario {
            product,
            key,
            delta,
            position,
            snapshot_id,
        } => {
            let shocked = q::evaluate_quote_graph(graph, key, *delta)?;
            let result = super::risk::graph_scenario(
                product,
                graph.baseline(),
                &shocked,
                position.clone(),
                snapshot_id.clone(),
                resources,
            )?;
            Ok(serde_json::json!({"valuation":result,"graph":graph_value(&shocked)?}))
        }
        CalibrationOperation::ReconcileBuckets { input } => {
            super::risk::graph_buckets(input, graph, resources)
        }
    })
}
