//! Closed-domain inverse requests over the same full native product pricers.
use super::*;
use convex_curves::contracts::BuiltCurve;
use convex_math::solvers::{brent, SolverConfig};
use serde::{Deserialize, Serialize};
use std::cell::{Cell, RefCell};

/// Explicit target quotation; clean/dirty conversion uses original accrued.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum BondPriceTarget {
    /// Per100 original face, excluding original accrued/rebate.
    Clean(f64),
    /// Per100 original face, including original accrued/rebate.
    Dirty(f64),
}
/// Caller-admitted closed inverse domain and native price residual tolerance.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BondInverseSettings {
    /// Original lower annual decimal yield/spread.
    pub lower: f64,
    /// Original upper annual decimal yield/spread.
    pub upper: f64,
    /// Positive absolute per100 price bound; no width-only acceptance.
    pub price_tolerance: f64,
    /// Positive Brent parameter/solver tolerance, at most1e-6. This is distinct
    /// from final price acceptance; neither parameter width nor a grid is proof.
    pub parameter_tolerance: f64,
    /// Work ceiling,1..256; no automatic retries with relaxed controls.
    pub max_iterations: u32,
}
impl BondInverseSettings {
    pub(crate) fn validate(self, max_parameter: f64) -> WorkflowResult<()> {
        let settings = self;
        if !settings.lower.is_finite()
            || !settings.upper.is_finite()
            || settings.lower >= settings.upper
            || settings.lower < -max_parameter
            || settings.upper > max_parameter
            || !settings.price_tolerance.is_finite()
            || settings.price_tolerance <= 0.0
            || settings.price_tolerance > 1.0
            || !settings.parameter_tolerance.is_finite()
            || settings.parameter_tolerance <= 0.0
            || settings.parameter_tolerance > 1e-6
            || settings.max_iterations == 0
            || settings.max_iterations > 256
        {
            return Err(invalid("inverse settings","finite closed admitted domain, native price tolerance in (0,1], iterations1..256 required"));
        }
        Ok(())
    }
}
/// Accepted same-pricer bond inverse and actual work/postcondition evidence.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BondInverse {
    /// Annual decimal yield/Z/DM, retaining fractional basis points.
    pub decimal_parameter: f64,
    /// Original requested clean/dirty target.
    pub target: BondPriceTarget,
    /// Original unchanged closed domain and tolerance.
    pub settings: BondInverseSettings,
    /// Actual Brent iterations,0 for an admitted endpoint.
    pub iterations: u32,
    /// Actual complete cash valuations, including endpoint and final repricing.
    pub price_evaluations: usize,
    /// Final dirty-price minus converted target in per100 units.
    pub native_residual: f64,
    /// Full final cash/model quote.
    pub valuation: BondValue,
}
pub(super) fn solve_bond(
    cash: &PreparedBondCash,
    target: BondPriceTarget,
    settings: BondInverseSettings,
    max_parameter: f64,
    price: impl Fn(f64) -> WorkflowResult<BondValue>,
) -> WorkflowResult<BondInverse> {
    settings.validate(max_parameter)?;
    let evaluations = Cell::new(0usize);
    let evaluate = |p| {
        evaluations.set(evaluations.get() + 1);
        price(p)
    };
    let low = evaluate(settings.lower)?;
    let high = evaluate(settings.upper)?;
    let dirty = match target {
        BondPriceTarget::Clean(c) => c + low.accrued_per100,
        BondPriceTarget::Dirty(d) => d,
    };
    if !dirty.is_finite() || dirty < 0.0 {
        return Err(invalid(
            "inverse target",
            "nonnegative finite original dirty price required",
        ));
    }
    let f_low = finite(low.dirty_per100 - dirty, "inverse residual")?;
    let f_high = finite(high.dirty_per100 - dirty, "inverse residual")?;
    if !cash.monotone()? {
        if f_low.abs() <= settings.price_tolerance {
            return Err(WorkflowError::NonUnique(
                "no remaining future cash rate exposure",
            ));
        }
        return Err(WorkflowError::NoRoot {
            lower: settings.lower,
            upper: settings.upper,
            low_residual: f_low,
            high_residual: f_high,
        });
    }
    if (low.dirty_per100 - high.dirty_per100).abs() <= settings.price_tolerance
        && f_low.abs() <= settings.price_tolerance
        && f_high.abs() <= settings.price_tolerance
    {
        return Err(WorkflowError::NonUnique(
            "parameter is not identified within the original native price tolerance",
        ));
    }
    let (parameter, iterations) = if f_low.abs() <= settings.price_tolerance {
        (settings.lower, 0)
    } else if f_high.abs() <= settings.price_tolerance {
        (settings.upper, 0)
    } else {
        let error = RefCell::new(None);
        let root = brent(
            |p| match evaluate(p) {
                Ok(v) => v.dirty_per100 - dirty,
                Err(e) => {
                    if error.borrow().is_none() {
                        *error.borrow_mut() = Some(e);
                    }
                    f64::NAN
                }
            },
            settings.lower,
            settings.upper,
            &SolverConfig {
                tolerance: settings.parameter_tolerance,
                max_iterations: settings.max_iterations,
            },
        );
        if let Some(e) = error.into_inner() {
            return Err(e);
        }
        let root = root?;
        (root.root, root.iterations)
    };
    let value = evaluate(parameter)?;
    let residual = finite(value.dirty_per100 - dirty, "inverse residual")?;
    if residual.abs() > settings.price_tolerance {
        return Err(WorkflowError::NoConvergence {
            iterations: iterations as usize,
            residual,
        });
    }
    Ok(BondInverse {
        decimal_parameter: parameter,
        target,
        settings,
        iterations,
        price_evaluations: evaluations.get(),
        native_residual: residual,
        valuation: value,
    })
}
/// Yield of actual entitled fixed cash; mixed-sign and uninformative cash do
/// not produce an arbitrary root. Domain errors remain distinct from no-root.
pub fn solve_bond_yield(
    cash: &PreparedBondCash,
    convention: DatedYieldConvention,
    target: BondPriceTarget,
    settings: BondInverseSettings,
) -> WorkflowResult<BondInverse> {
    cash.fixed_yield()?;
    solve_bond(cash, target, settings, 10.0, |p| {
        cash.price_yield(p, convention)
    })
}
/// Exact Z-spread or frozen-projection DM on the supplied reference curve.
pub fn solve_bond_spread(
    cash: &PreparedBondCash,
    curve: &BuiltCurve,
    role: BondDiscountRole,
    target: BondPriceTarget,
    settings: BondInverseSettings,
) -> WorkflowResult<BondInverse> {
    solve_bond(cash, target, settings, 1.0, |p| {
        cash.price_spread(curve, role.clone(), p)
    })
}
/// Consistent disjoint inverse requests; no measure-name string dispatch.
pub enum InverseRequest<'a> {
    /// Full dated fixed-cash yield.
    BondYield {
        /// Prepared exact cash.
        cash: &'a PreparedBondCash,
        /// Explicit dated convention.
        convention: DatedYieldConvention,
        /// Clean/dirty original-face target.
        target: BondPriceTarget,
        /// Closed admitted controls.
        settings: BondInverseSettings,
    },
    /// Full Z/frozen-projection DM.
    BondSpread {
        /// Prepared exact cash.
        cash: &'a PreparedBondCash,
        /// Actual reference curve.
        curve: &'a BuiltCurve,
        /// Exact collateral/government role.
        role: BondDiscountRole,
        /// Clean/dirty target.
        target: BondPriceTarget,
        /// Closed admitted controls.
        settings: BondInverseSettings,
    },
    /// Native uniform fair fixed rate, preserving original linearity restrictions.
    SwapFairRate(&'a ValuationInputs<'a>),
    /// Original standard conversion through complete CDS integration.
    Cds {
        /// Original checked contract.
        contract: &'a convex_credit::CdsContract,
        /// Actual payment/default state.
        context: &'a convex_credit::CdsPricingContext,
        /// Actual discount values.
        curves: &'a convex_curves::contracts::CurveSet,
        /// Explicit running/clean/dirty unit.
        quote: convex_credit::CdsQuote,
        /// Original intensity/residual/work controls.
        settings: convex_credit::ConversionSettings,
    },
    /// Native HW1F OAS solve retaining every price-grid guard.
    OptionalOas {
        /// Original model, full cash and fixed histories.
        prepared: &'a crate::optional_bond::PreparedOptionalBond,
        /// Dirty per100 face.
        target_dirty: f64,
        /// Native closed grid solve settings.
        settings: crate::optional_bond::OasSolveSettings,
    },
    /// Actual remaining-rate inversion after validating the full native snapshot.
    RateFuture {
        /// Exact original contract.
        contract: &'a convex_core::futures::FuturesContract,
        /// Complete native window/history.
        valuation: &'a crate::futures::RateFuturesValuation,
        /// Original target points.
        price_points: convex_core::contracts::ExactDecimal,
    },
}
/// Inverse output retains full native economic diagnostics and distinctions.
#[derive(Debug, Clone)]
pub enum InverseValue {
    /// Complete bond price/inverse.
    Bond(Box<BondInverse>),
    /// Original swap PV/cash and all uniform/shift par quotes.
    Swap(Box<convex_rates::pricing::SwapValuation>),
    /// Full original CDS conversion/initial cash/native quote residual.
    Cds(Box<convex_credit::ConversionResult>),
    /// Native fixed-model OAS price/refinement/actual work.
    Optional(Box<crate::optional_bond::OasResult>),
    /// Annual decimal equivalent rate for remaining future weights.
    RemainingFutureRate(convex_core::contracts::ExactDecimal),
}
/// Execute one admitted inverse, preserving original pricers and postconditions.
pub fn inverse(request: InverseRequest<'_>) -> WorkflowResult<InverseValue> {
    Ok(match request {
        InverseRequest::BondYield {
            cash,
            convention,
            target,
            settings,
        } => InverseValue::Bond(Box::new(solve_bond_yield(
            cash, convention, target, settings,
        )?)),
        InverseRequest::BondSpread {
            cash,
            curve,
            role,
            target,
            settings,
        } => InverseValue::Bond(Box::new(solve_bond_spread(
            cash, curve, role, target, settings,
        )?)),
        InverseRequest::SwapFairRate(input) => {
            if !matches!(input, ValuationInputs::Swap { .. }) {
                return Err(WorkflowError::NotApplicable(
                    "fair swap fixed rate requires IRS/OIS economics",
                ));
            }
            let Valuation::Swap(value) = price(input)? else {
                return Err(invalid("inverse", "swap dispatch mismatch"));
            };
            match &value.par {
                convex_rates::pricing::SwapParQuotes::Available { .. } => {}
                convex_rates::pricing::SwapParQuotes::Unavailable { .. } => {
                    if value.fixed.is_empty() && value.floating.is_empty() {
                        return Err(WorkflowError::NonUnique(
                            "swap has no remaining rate exposure",
                        ));
                    }
                    return Err(WorkflowError::Unsupported("analytic par requires unrounded linear cash and positive remaining exposure"));
                }
            }
            InverseValue::Swap(value)
        }
        InverseRequest::Cds {
            contract,
            context,
            curves,
            quote,
            settings,
        } => InverseValue::Cds(Box::new(convex_credit::convert_cds(
            contract, context, curves, quote, settings,
        )?)),
        InverseRequest::OptionalOas {
            prepared,
            target_dirty,
            settings,
        } => {
            let result = prepared
                .solve_oas(target_dirty, settings)
                .map_err(|e| match e {
                    crate::optional_bond::OptionalBondError::OasBracket {
                        low_residual,
                        high_residual,
                    } if low_residual.abs() <= settings.price_tolerance
                        && high_residual.abs() <= settings.price_tolerance =>
                    {
                        WorkflowError::NonUnique(
                            "OAS is not identified at the original model resolution",
                        )
                    }
                    crate::optional_bond::OptionalBondError::OasBracket {
                        low_residual,
                        high_residual,
                    } => WorkflowError::NoRoot {
                        lower: settings.lower,
                        upper: settings.upper,
                        low_residual,
                        high_residual,
                    },
                    crate::optional_bond::OptionalBondError::OasConvergence {
                        iterations,
                        residual,
                    } => WorkflowError::NoConvergence {
                        iterations,
                        residual,
                    },
                    e => e.into(),
                })?;
            if result.residual_per_100.abs() > settings.price_tolerance {
                return Err(WorkflowError::NoConvergence {
                    iterations: result.iterations,
                    residual: result.residual_per_100,
                });
            }
            InverseValue::Optional(Box::new(result))
        }
        InverseRequest::RateFuture {
            contract,
            valuation,
            price_points,
        } => {
            // Always invoke original full snapshot validation before classifying
            // lack of remaining exposure. Do not trust caller-created aggregates.
            let rate =
                crate::futures::remaining_rate_from_price(contract, valuation, price_points)?;
            InverseValue::RemainingFutureRate(rate.ok_or(WorkflowError::NonUnique(
                "fully fixed future has no remaining-rate parameter",
            ))?)
        }
    })
}
