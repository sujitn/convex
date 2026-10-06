//! Bounded standard quote conversion through the same complete leg valuation.
use crate::{
    finite, invalid, pricing::Prepared, CdsContract, CdsError, CdsPricingContext, CdsValuation,
    CreditInput, MAX_CONVERSION_ITERATIONS,
};
use convex_curves::contracts::CurveSet;
use convex_math::solvers::{brent, SolverConfig};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;

/// Buyer-to-seller quote units. A standard coupon is retained in contract terms.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CdsQuote {
    /// Nonnegative conventional annualized spread; 0.01 means 100 bp.
    RunningSpread {
        /// Annualized decimal rate, not a coupon cash amount.
        decimal_rate: f64,
    },
    /// CLEAN upfront fraction; 0.01 means one percent of notional.
    CleanUpfront {
        /// Buyer-to-seller clean fraction, excluding accrued refund.
        fraction: f64,
    },
    /// Actual initial CASH fraction; clean less accrued-premium refund.
    DirtyUpfront {
        /// Buyer-to-seller currency cash divided by notional.
        fraction: f64,
    },
}
/// Explicit closed intensity bracket and original native-quote convergence bound.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversionSettings {
    /// Nonnegative constant intensity lower bound per ACT/365F year.
    pub lower_hazard: f64,
    /// Strictly higher upper bound, at most 5; never expanded silently.
    pub upper_hazard: f64,
    /// Positive absolute residual in input decimal rate or upfront-fraction units.
    pub native_quote_tolerance: f64,
    /// Iteration ceiling in 1..256; no width-only root success.
    pub max_iterations: u32,
}
impl Default for ConversionSettings {
    fn default() -> Self {
        Self {
            lower_hazard: 0.0,
            upper_hazard: 1.0,
            native_quote_tolerance: 1e-12,
            max_iterations: 100,
        }
    }
}
/// Successful constant-hazard fit with final repricing in the original quote unit.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ConversionResult {
    /// Original native quote and unit preserved verbatim.
    pub input_quote: CdsQuote,
    /// Actual converged intensity per ACT/365F year.
    pub hazard_per_act365f_year: f64,
    /// CLEAN conventional spread under the fitted flat intensity, decimal/year.
    pub conventional_spread_decimal: f64,
    /// Final model minus original quote, in the input quote's declared unit.
    pub native_quote_residual: f64,
    /// Actual solver iterations, including zero for an exact endpoint.
    pub iterations: u32,
    /// Actual settings, unchanged during the solve.
    pub settings: ConversionSettings,
    /// Full same-pricer fair quote, accrued, signed leg and original payment PV.
    pub valuation: CdsValuation,
}
/// Convert a valid conventional running spread or clean/dirty upfront quote.
/// The single nonnegative constant hazard is fitted through actual premium and
/// protection integration. A flat spread/(1-recovery) proxy is never substituted.
pub fn convert_cds(
    contract: &CdsContract,
    context: &CdsPricingContext,
    curves: &CurveSet,
    quote: CdsQuote,
    settings: ConversionSettings,
) -> Result<ConversionResult, CdsError> {
    if !settings.lower_hazard.is_finite()
        || !settings.upper_hazard.is_finite()
        || settings.lower_hazard < 0.0
        || settings.upper_hazard > 5.0
        || settings.lower_hazard >= settings.upper_hazard
        || !settings.native_quote_tolerance.is_finite()
        || settings.native_quote_tolerance <= 0.0
        || settings.native_quote_tolerance > 1e-6
        || settings.max_iterations == 0
        || settings.max_iterations > MAX_CONVERSION_ITERATIONS as u32
    {
        return Err(invalid("conversion_settings","requires a closed nonnegative bracket <= 5, native tolerance in (0,1e-6], and bounded iterations"));
    }
    let target=match quote {CdsQuote::RunningSpread{decimal_rate} if decimal_rate.is_finite() && (0.0..=1.0).contains(&decimal_rate)=>decimal_rate,CdsQuote::CleanUpfront{fraction}|CdsQuote::DirtyUpfront{fraction} if fraction.is_finite() && fraction.abs()<=10.0=>fraction,_=>return Err(invalid("quote","requires explicit finite decimal spread in [0,1] or upfront fraction of magnitude <= 10"))};
    if contract.terms().reference.recovery_fraction.get() == 1.0 {
        return Err(CdsError::Unsupported(
            "conventional conversion with full recovery has no identified hazard",
        ));
    }
    let prepared = Prepared::new(
        contract,
        context,
        curves.get(&contract.terms().discount_curve)?,
        None,
    )?;
    if prepared.matured {
        return Err(CdsError::Unsupported(
            "conventional conversion after protection maturity",
        ));
    }
    let evaluate = |hazard: f64| -> Result<f64, CdsError> {
        let qs = prepared.flat_survival(hazard)?;
        let components = prepared.components(&qs)?;
        let model = match quote {
            CdsQuote::RunningSpread { .. } => prepared.par_spread(&components)?,
            CdsQuote::CleanUpfront { .. } => prepared.clean_fraction(&components)?,
            CdsQuote::DirtyUpfront { .. } => {
                prepared.clean_fraction(&components)? - prepared.coupon * prepared.accrued_fraction
            }
        };
        finite(model - target)
    };
    // The shared Brent API has a numeric callback. Preserve its first product
    // failure, then stop via NaN rejection; no failed product becomes a residual.
    let error = RefCell::new(None);
    let solved = brent(
        |h| match evaluate(h) {
            Ok(value) => value,
            Err(e) => {
                if error.borrow().is_none() {
                    *error.borrow_mut() = Some(e);
                }
                f64::NAN
            }
        },
        settings.lower_hazard,
        settings.upper_hazard,
        &SolverConfig {
            tolerance: settings.native_quote_tolerance,
            max_iterations: settings.max_iterations,
        },
    );
    if let Some(e) = error.into_inner() {
        return Err(e);
    }
    let solved = solved?;
    let residual = evaluate(solved.root)?;
    if residual.abs() > settings.native_quote_tolerance {
        return Err(CdsError::Numerical(
            "final native quote residual exceeds original tolerance",
        ));
    }
    let qs = prepared.flat_survival(solved.root)?;
    let valuation = prepared.result(
        &qs,
        CreditInput::FlatHazard {
            hazard_per_act365f_year: solved.root,
        },
    )?;
    let conventional_spread_decimal = valuation.par_spread_decimal.ok_or(CdsError::Unsupported(
        "no positive clean annuity for conventional spread",
    ))?;
    Ok(ConversionResult {
        input_quote: quote,
        hazard_per_act365f_year: solved.root,
        conventional_spread_decimal,
        native_quote_residual: residual,
        iterations: solved.iterations,
        settings,
        valuation,
    })
}
