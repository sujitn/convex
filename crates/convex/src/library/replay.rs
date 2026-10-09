//! Complete calibration capture/replay using the existing pure graph owners.
use super::{inputs::ResolvedInputs, *};
use convex_analytics::{
    cash_calibration as c, credit_calibration as d, quote_risk as q, rate_calibration as r,
};

/// Immutable original inputs and actually rebuilt curves. This is setup/batch
/// work; it contains no decoded cache, clock, provider or publication authority.
#[derive(Debug, Clone)]
pub struct CalibrationReplay {
    inputs: LibraryInputs,
    curves: CurveSet,
    diagnostics: Value,
}
impl CalibrationReplay {
    /// Rebuild original full products and every source quote with native guards.
    pub fn build(inputs: &LibraryInputs) -> LibraryResult<Self> {
        let calibration = inputs
            .calibration
            .as_ref()
            .ok_or_else(|| LibraryError::missing("complete calibration", Value::Null))?;
        if calibration.rates.is_empty()
            && calibration.credit.is_empty()
            && calibration.cash.is_empty()
            && inputs.curves.is_empty()
        {
            return Err(LibraryError::invalid(
                "calibration",
                "at least one complete product recipe or independently supplied native curve required",
            ));
        }
        let resources = ResolvedInputs::resources(inputs)?;
        curves::with_graph(calibration, &resources, |graph| {
            let mut diagnostics = curves::graph_value(graph.baseline())?;
            diagnostics["calibrations"] = value(
                graph
                    .baseline()
                    .calibrations
                    .iter()
                    .map(fit_value)
                    .collect::<Vec<_>>(),
            )?;
            Ok(Self {
                inputs: inputs.clone(),
                curves: graph.baseline().curves.clone(),
                diagnostics,
            })
        })
    }
    /// Original complete supplied contracts, calendars, history and controls.
    pub fn inputs(&self) -> &LibraryInputs {
        &self.inputs
    }
    /// Exact native fitted and independently supplied curves; no sampled alias.
    pub fn curves(&self) -> &CurveSet {
        &self.curves
    }
    /// Full original recipes, fitted outputs, valuations and solver/rank/work.
    pub fn diagnostics(&self) -> &Value {
        &self.diagnostics
    }
    /// Rebuild the same original graph under the declared native risk policy.
    /// Failures never expose a partial scenario or modify the captured baseline.
    pub fn evaluate(&self, action: &CalibrationOperation) -> LibraryResult<Value> {
        let resources = ResolvedInputs::resources(&self.inputs)?;
        let input = self
            .inputs
            .calibration
            .as_ref()
            .ok_or_else(|| LibraryError::missing("complete calibration", Value::Null))?;
        curves::calibration(input, action, &resources)
    }
}
/// Use original bounded lossless JSON admission; unknown/discarded terms and
/// numeric Decimal precision loss fail exactly as on the shared client route.
pub fn decode_calibration_inputs(input: &Value) -> LibraryResult<LibraryInputs> {
    let request = serde_json::json!({"schema_version":LIBRARY_SCHEMA_VERSION,"inputs":input,"operation":{"kind":"curves"}});
    let decoded = decode_request(&request.to_string())?;
    if decoded.inputs.calibration.is_none() {
        return Err(LibraryError::missing("complete calibration", Value::Null));
    }
    Ok(decoded.inputs)
}
/// Strict original operation decoding, including nested product Decimal terms.
/// The complete original calibration stays in its operation owner, avoiding
/// installing fitted outputs as independently supplied graph dependencies.
pub fn decode_calibration_action(
    inputs: &LibraryInputs,
    action: &str,
) -> LibraryResult<CalibrationOperation> {
    let action = super::ingress::parse(action).map_err(|e| LibraryError::invalid("action", &e))?;
    let calibration = inputs
        .calibration
        .as_ref()
        .ok_or_else(|| LibraryError::missing("complete calibration", Value::Null))?;
    let mut resources = serde_json::to_value(inputs)
        .map_err(|e| LibraryError::invalid("inputs", &e.to_string()))?;
    resources["calibration"] = Value::Null;
    let request = serde_json::json!({"schema_version":LIBRARY_SCHEMA_VERSION,"inputs":resources,"operation":{"kind":"calibration","input":calibration,"action":action}});
    match decode_request(&request.to_string())?.operation {
        LibraryOperation::Calibration { action, .. } => Ok(action),
        _ => Err(LibraryError::invalid(
            "action",
            "complete calibration action required",
        )),
    }
}
fn roots_value(roots: &[convex_math::solvers::SolverResult]) -> Value {
    serde_json::json!(roots
        .iter()
        .map(|r| serde_json::json!({"root":r.root,"iterations":r.iterations,"residual":r.residual}))
        .collect::<Vec<_>>())
}
fn fit_value(fit: &q::QuoteGraphCalibration) -> Value {
    match fit {
        q::QuoteGraphCalibration::Rate(fit) => {
            let solver = match &fit.solver {
                r::RateSolverDiagnostics::Piecewise { roots, evaluations } => {
                    serde_json::json!({"kind":"piecewise","roots":roots_value(roots),"evaluations":evaluations})
                }
                r::RateSolverDiagnostics::Global(d) => {
                    serde_json::json!({"kind":"global","diagnostics":super::errors::fit_diagnostics(d)})
                }
            };
            serde_json::json!({"kind":"rate","recipe":fit.recipe,"curve":curves::describe_curve(&fit.curve),"repriced_helpers":fit.repriced_helpers,"solver":solver})
        }
        q::QuoteGraphCalibration::Credit(fit) => {
            let solver = match &fit.solver {
                d::CreditSolverDiagnostics::Piecewise { helper_ids, roots } => {
                    serde_json::json!({"kind":"piecewise","helper_ids":helper_ids,"roots":roots_value(roots)})
                }
                d::CreditSolverDiagnostics::Global(d) => {
                    serde_json::json!({"kind":"global","diagnostics":super::errors::fit_diagnostics(d)})
                }
            };
            serde_json::json!({"kind":"credit","recipe":fit.recipe,"curve":curves::describe_curve(&fit.curve),"hazards_per_act365f_year":fit.hazards_per_act365f_year,"repriced_helpers":fit.repriced_helpers,"solver":solver,"price_evaluations":fit.price_evaluations})
        }
        q::QuoteGraphCalibration::Cash(fit) => {
            let solver = match &fit.solver {
                c::CashSolverDiagnostics::Piecewise { helper_ids, roots } => {
                    serde_json::json!({"kind":"piecewise","helper_ids":helper_ids,"roots":roots_value(roots)})
                }
                c::CashSolverDiagnostics::Global(d) => {
                    serde_json::json!({"kind":"global","diagnostics":super::errors::fit_diagnostics(d)})
                }
            };
            serde_json::json!({"kind":"cash","recipe":fit.recipe,"curve":curves::describe_curve(&fit.curve),"annualized_parameters":fit.annualized_parameters,"repriced_helpers":fit.repriced_helpers,"solver":solver,"price_evaluations":fit.price_evaluations})
        }
    }
}
