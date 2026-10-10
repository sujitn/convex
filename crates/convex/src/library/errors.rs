//! Lossless native failure classification. Nested wrappers cannot hide a missing
//! input, unsupported model or numerical failure behind a generic successful value.
use super::{LibraryError, LibraryErrorCode as Code};
use convex_analytics::{
    cash_calibration as cash, credit_calibration as credit, futures, optional_bond as optional,
    quote_risk as risk, rate_calibration as rates, workflow,
};
use convex_core::cashflows::CashflowError;
use convex_curves::contracts::CurveContractError;
use convex_math::{
    optimization::{FitDiagnostics, FitFailure, FitFailureReason},
    MathError,
};
use serde_json::{json, Value};

impl From<convex_analytics::risk::hedging::constrained::HedgeError> for LibraryError {
    fn from(e: convex_analytics::risk::hedging::constrained::HedgeError) -> Self {
        use convex_analytics::risk::hedging::constrained::HedgeError as E;
        let code = match e {
            E::Missing(_) => Code::MissingInput,
            E::Bound(_) => Code::ResourceLimit,
            E::Invalid { .. } => Code::InvalidInput,
        };
        native(code, &e)
    }
}

impl From<cash::CashBasisHedgeError> for LibraryError {
    fn from(e: cash::CashBasisHedgeError) -> Self {
        match e {
            cash::CashBasisHedgeError::Cash(e) => e.into(),
            cash::CashBasisHedgeError::Workflow(e) => e.into(),
            cash::CashBasisHedgeError::Risk(e) => e.into(),
            cash::CashBasisHedgeError::Curve(e) => e.into(),
        }
    }
}

fn native<E: std::fmt::Debug + std::fmt::Display>(code: Code, e: &E) -> LibraryError {
    LibraryError {
        code,
        message: e.to_string(),
        details: json!({"native_kind":format!("{e:?}")}),
    }
}
impl From<convex_analytics::cash_quotation::CashQuotationError> for LibraryError {
    fn from(e: convex_analytics::cash_quotation::CashQuotationError) -> Self {
        use convex_analytics::cash_quotation::CashQuotationError as E;
        match e {
            E::Cash(e) => e.into(),
            E::Workflow(e) => e.into(),
            E::SpreadResidual {
                decimal_residual,
                decimal_tolerance,
                iterations,
                price_evaluations,
            } => Self {
                code: Code::NoConvergence,
                message: format!(
                    "G-spread roundtrip residual {decimal_residual} exceeds {decimal_tolerance}"
                ),
                details: json!({"decimal_residual":decimal_residual,"decimal_tolerance":decimal_tolerance,
                    "residual_unit":"annual_decimal_rate","iterations":iterations,"price_evaluations":price_evaluations}),
            },
        }
    }
}
pub(super) fn fit_diagnostics(d: &FitDiagnostics) -> Value {
    json!({"iterations":d.iterations,"evaluations":d.evaluations,"scaled_rms":d.scaled_rms,
        "native_residuals":d.native_residuals,"rank":d.rank,
        "condition_estimate":match d.condition_estimate {Some(v) if !v.is_finite()=>json!({"non_finite":v.to_string()}),v=>json!(v)},
        "active_bounds":d.active_bounds})
}
pub(super) fn fit_error<E: Into<LibraryError> + std::fmt::Display>(
    e: FitFailure<E>,
) -> LibraryError {
    let diagnostics = fit_diagnostics(&e.diagnostics);
    let mut error = match e.reason {
        FitFailureReason::Callback(e) => e.into(),
        FitFailureReason::InvalidInput(reason) => LibraryError::invalid("solver", reason),
        FitFailureReason::IllConditioned => LibraryError {
            code: Code::NonUnique,
            message: "native weighted Jacobian is not identified".into(),
            details: Value::Null,
        },
        reason => LibraryError {
            code: Code::NoConvergence,
            message: reason.to_string(),
            details: Value::Null,
        },
    };
    error.details = json!({"cause":error.details,"solver":diagnostics});
    error
}
impl From<workflow::WorkflowError> for LibraryError {
    fn from(e: workflow::WorkflowError) -> Self {
        use workflow::WorkflowError as E;
        match e {
            E::Bond(e) => e.into(),
            E::Swap(e) => e.into(),
            E::Credit(e) => e.into(),
            E::Optional(e) => e.into(),
            E::Futures(e) => e.into(),
            E::Curve(e) => e.into(),
            E::Cashflow(e) => e.into(),
            E::Math(e) => e.into(),
            E::Risk(e) => e.into(),
            E::MissingInput(ref v) => Self {
                code: Code::MissingInput,
                message: e.to_string(),
                details: json!({"missing":v}),
            },
            E::NoRoot {
                lower,
                upper,
                low_residual,
                high_residual,
            } => Self {
                code: Code::NoRoot,
                message: e.to_string(),
                details: json!({"lower":lower,"upper":upper,"low_residual":low_residual,"high_residual":high_residual}),
            },
            E::NoConvergence {
                iterations,
                residual,
            } => Self {
                code: Code::NoConvergence,
                message: e.to_string(),
                details: json!({"iterations":iterations,"residual":residual}),
            },
            E::NotApplicable(_) => native(Code::NotApplicable, &e),
            E::Unsupported(_) => native(Code::Unsupported, &e),
            E::NonUnique(_) => native(Code::NonUnique, &e),
            E::Invalid { .. } => native(Code::InvalidInput, &e),
        }
    }
}
impl From<CashflowError> for LibraryError {
    fn from(e: CashflowError) -> Self {
        match e {
            CashflowError::MissingFixings(ref v) => Self {
                code: Code::MissingInput,
                message: e.to_string(),
                details: json!({"missing_fixings":v}),
            },
            CashflowError::BoundExceeded(_) => native(Code::ResourceLimit, &e),
            e => native(Code::InvalidInput, &e),
        }
    }
}
impl From<CurveContractError> for LibraryError {
    fn from(e: CurveContractError) -> Self {
        match e {
            CurveContractError::Primitives(e) => e.into(),
            CurveContractError::MissingDependency(ref key) => Self {
                code: Code::MissingInput,
                message: e.to_string(),
                details: json!({"curve":key}),
            },
            CurveContractError::Unsupported { .. } => native(Code::Unsupported, &e),
            CurveContractError::BoundExceeded(_) => native(Code::ResourceLimit, &e),
            CurveContractError::RepricingFailure { .. } | CurveContractError::Numerical(_) => {
                native(Code::NoConvergence, &e)
            }
            e => native(Code::InvalidInput, &e),
        }
    }
}
impl From<convex_bonds::contracts::BondContractError> for LibraryError {
    fn from(e: convex_bonds::contracts::BondContractError) -> Self {
        use convex_bonds::contracts::BondContractError as E;
        match e {
            E::Cashflow(e) => e.into(),
            E::Unsupported { .. } => native(Code::Unsupported, &e),
            E::Bond(_) => native(Code::InvalidInput, &e),
        }
    }
}
impl From<convex_rates::SwapError> for LibraryError {
    fn from(e: convex_rates::SwapError) -> Self {
        use convex_rates::SwapError as E;
        match e {
            E::Cashflow(e) => e.into(),
            E::Curve(e) => e.into(),
            E::Unsupported { .. } => native(Code::Unsupported, &e),
        }
    }
}
impl From<convex_credit::CdsError> for LibraryError {
    fn from(e: convex_credit::CdsError) -> Self {
        use convex_credit::CdsError as E;
        match e {
            E::Cashflow(e) => e.into(),
            E::Curve(e) => e.into(),
            E::Solver(e) => e.into(),
            E::Unsupported(_) => native(Code::Unsupported, &e),
            E::Numerical(_) => native(Code::NoConvergence, &e),
            E::Contract(_) => native(Code::InvalidInput, &e),
        }
    }
}
impl From<MathError> for LibraryError {
    fn from(e: MathError) -> Self {
        match e {
            MathError::InvalidBracket { .. } => native(Code::NoRoot, &e),
            MathError::SingularMatrix => native(Code::NonUnique, &e),
            MathError::ConvergenceFailed { .. } | MathError::NonFinite { .. } => {
                native(Code::NoConvergence, &e)
            }
            e => native(Code::InvalidInput, &e),
        }
    }
}
impl From<futures::FuturesError> for LibraryError {
    fn from(e: futures::FuturesError) -> Self {
        use futures::FuturesError as E;
        match e {
            E::Cashflow(e) => e.into(),
            E::Curve(e) => e.into(),
            E::Bond(e) => e.into(),
            e => native(Code::InvalidInput, &e),
        }
    }
}
impl From<optional::OptionalBondError> for LibraryError {
    fn from(e: optional::OptionalBondError) -> Self {
        use optional::OptionalBondError as E;
        match e {
            E::Bond(e) => e.into(),
            E::Cashflow(e) => e.into(),
            E::Curve(e) => e.into(),
            E::Calibration(e) => fit_error(*e),
            E::Unsupported { .. } => native(Code::Unsupported, &e),
            E::Invalid { .. } => native(Code::InvalidInput, &e),
            E::Model(e) => {
                use convex_bonds::options::hw1f::Hw1fError as H;
                match e {
                    H::Math(e) => e.into(),
                    H::Bound(_) => native(Code::ResourceLimit, &e),
                    H::Invalid { .. } => native(Code::InvalidInput, &e),
                    H::Numerical(_) => native(Code::NoConvergence, &e),
                }
            }
            E::OasBracket { .. } => native(Code::NoRoot, &e),
            E::GridConvergence { ref diagnostics } => Self {
                code: Code::NoConvergence,
                message: e.to_string(),
                details: json!({"grids":diagnostics}),
            },
            E::RiskConvergence { ref diagnostics } => Self {
                code: Code::NoConvergence,
                message: e.to_string(),
                details: json!({"risk_grids":diagnostics}),
            },
            e => native(Code::NoConvergence, &e),
        }
    }
}
impl From<credit::CreditCalibrationError> for LibraryError {
    fn from(e: credit::CreditCalibrationError) -> Self {
        use credit::CreditCalibrationError as E;
        match e {
            E::Curve(e) => e.into(),
            E::Cds(e) => e.into(),
            E::Global(e) => fit_error(*e),
            E::Root { helper_id, source } => {
                let mut e = Self::from(source);
                e.details = json!({"helper_id":helper_id,"cause":e.details});
                e
            }
        }
    }
}
impl From<cash::CashCalibrationError> for LibraryError {
    fn from(e: cash::CashCalibrationError) -> Self {
        use cash::CashCalibrationError as E;
        match e {
            E::Curve(e) => e.into(),
            E::Bond(e) => e.into(),
            E::Cashflow(e) => e.into(),
            E::Credit(e) => e.into(),
            E::Root(e) => e.into(),
            E::Icma(e) => (*e).into(),
            E::Global(e) => fit_error(*e),
        }
    }
}
impl From<rates::RateCalibrationError> for LibraryError {
    fn from(e: rates::RateCalibrationError) -> Self {
        use rates::RateCalibrationError as E;
        match e {
            E::Curve(e) => e.into(),
            E::Cashflow(e) => e.into(),
            E::Swap(e) => e.into(),
            E::Futures(e) => e.into(),
            E::Root(e) => e.into(),
            E::Global(e) => fit_error(*e),
        }
    }
}
impl From<risk::QuoteRiskError> for LibraryError {
    fn from(e: risk::QuoteRiskError) -> Self {
        use risk::QuoteRiskError as E;
        match e {
            E::Curve(e) => e.into(),
            E::Rate(e) => e.into(),
            E::Credit(e) => e.into(),
            E::Cash(e) => e.into(),
            E::Optional(e) => e.into(),
            E::Invalid { .. } => native(Code::InvalidInput, &e),
        }
    }
}
