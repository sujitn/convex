//! Pure checked optional-bond model inputs, calibration, pricing and risk.
//!
//! Supported combinations are explicit. No provider lookup, global evaluation
//! date, assumed volatility or deterministic optional-coupon shortcut occurs.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod calibration;
mod pricing;

pub use calibration::*;
pub use convex_bonds::options::hw1f::{Hw1fParameters, OptionSide};
pub use pricing::*;

use convex_bonds::{contracts::BondContractError, options::hw1f::Hw1fError};
use convex_core::{cashflows::CashflowError, Date};
use convex_curves::contracts::{
    BuiltCurve, CurveContractError, CurveKey, CurvePoint, CurveQuery, CurveRole,
};
use rust_decimal::{prelude::ToPrimitive, Decimal};

/// Explicit input/unsupported/numerical outcomes; unsuccessful prices are absent.
#[derive(Debug, thiserror::Error)]
pub enum OptionalBondError {
    /// Invalid finite/dated/identity/role input.
    #[error("invalid {field}: {reason}")]
    Invalid {
        /// Input field.
        field: &'static str,
        /// Failed condition.
        reason: String,
    },
    /// Contract/model combination lacks the needed state/economics.
    #[error("unsupported {capability}: {reason}")]
    Unsupported {
        /// Unsupported operation or structure.
        capability: &'static str,
        /// Missing support.
        reason: String,
    },
    /// Original supplied bond/fixing failure.
    #[error(transparent)]
    Bond(#[from] BondContractError),
    /// Original cashflow/date failure.
    #[error(transparent)]
    Cashflow(#[from] CashflowError),
    /// Original curve failure.
    #[error(transparent)]
    Curve(#[from] CurveContractError),
    /// Original checked model numerical/resource failure.
    #[error(transparent)]
    Model(#[from] Hw1fError),
    /// Original calibration failure with native-unit residuals and actual limits.
    #[error("model calibration failed: {0}")]
    Calibration(Box<convex_math::optimization::FitFailure<OptionalBondError>>),
    /// Finest declared refinements did not satisfy both price postconditions.
    #[error("optional price grid refinement failed")]
    GridConvergence {
        /// All measured grids, without a successful price.
        diagnostics: Vec<GridPrice>,
    },
    /// Option finite differences are too small for the modeled state resolution.
    #[error("risk bump {bump} is below the grid state-resolution floor {minimum_bump}")]
    RiskResolution {
        /// Supplied decimal rate bump.
        bump: f64,
        /// Two finest-grid state cells at the widest live decision layer.
        minimum_bump: f64,
    },
    /// Price convergence alone did not establish sensitivity convergence.
    #[error("optional risk grid refinement failed")]
    RiskConvergence {
        /// Actual sensitivities at all resolutions; no successful risk result.
        diagnostics: Vec<GridRisk>,
    },
    /// Model parameter first/second derivatives failed separate grid refinement.
    #[error("optional model-parameter risk grid refinement failed")]
    ModelRiskConvergence {
        /// Actual derivatives; no accepted parameter-risk result.
        diagnostics: Vec<GridModelRisk>,
    },
    /// Supplied OAS bracket has no crossing or is a price plateau.
    #[error("OAS bracket cannot identify a crossing: low residual {low_residual}, high residual {high_residual}")]
    OasBracket {
        /// Low-spread residual, per 100 original face.
        low_residual: f64,
        /// High-spread residual, same units.
        high_residual: f64,
    },
    /// Bounded inverse exhausted its attempts without a price residual pass.
    #[error("OAS iteration limit {iterations}; price residual {residual}")]
    OasConvergence {
        /// Actual attempts.
        iterations: usize,
        /// Last price residual, per 100 original face.
        residual: f64,
    },
}
pub(super) fn invalid(field: &'static str, reason: impl Into<String>) -> OptionalBondError {
    OptionalBondError::Invalid {
        field,
        reason: reason.into(),
    }
}
pub(super) fn unsupported(
    capability: &'static str,
    reason: impl Into<String>,
) -> OptionalBondError {
    OptionalBondError::Unsupported {
        capability,
        reason: reason.into(),
    }
}
pub(super) fn finite(value: f64, field: &'static str) -> Result<f64, OptionalBondError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(invalid(field, "nonfinite arithmetic"))
    }
}
pub(super) fn number(value: Decimal) -> Result<f64, OptionalBondError> {
    value
        .to_f64()
        .filter(|v| v.is_finite())
        .ok_or_else(|| invalid("cash", "unrepresentable Decimal"))
}
pub(super) fn discount_role(
    curve: &BuiltCurve,
    currency: convex_core::Currency,
) -> Result<(), OptionalBondError> {
    if curve.recipe().currency != currency
        || !matches!(&curve.recipe().role, CurveRole::Discount { collateral_currency } if *collateral_currency == currency)
    {
        return Err(invalid(
            "discount_curve",
            "currency/collateral/discount role mismatch",
        ));
    }
    Ok(())
}
pub(super) fn df(curve: &BuiltCurve, date: Date) -> Result<f64, OptionalBondError> {
    Ok(curve
        .query(&CurveQuery::DiscountFactor {
            point: CurvePoint::Date(date),
        })?
        .value)
}
pub(super) fn time(root: Date, date: Date) -> Result<f64, OptionalBondError> {
    if !(1..=9999).contains(&date.year()) || date < root {
        return Err(invalid(
            "date",
            "date precedes model root or exceeds 0001..9999",
        ));
    }
    let value = root.days_between(&date) as f64 / 365.0;
    if value > 150.0 {
        return Err(invalid("model horizon", "exceeds 150 ACT/365F years"));
    }
    Ok(value)
}

/// Caller-assigned immutable model identity, kept distinct from curve versions.
pub type ModelKey = CurveKey;
