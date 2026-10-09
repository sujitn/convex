//! Native complete rate recipes and actual caller-resolved resources.

use convex_core::{
    cashflows::CashflowError,
    contracts::UtcTimestamp,
    fixings::RateOrigin,
    futures::{FuturesContract, ReferenceStamp},
};
use convex_curves::contracts::*;
use convex_math::{
    optimization::{FitDiagnostics, FitFailure},
    solvers::SolverResult,
};
use convex_rates::{
    basis::{BasisQuotedLeg, BasisSwapCalendars, BasisSwapTerms, BasisSwapValuation},
    CalendarBinding, CashflowContext, SwapCalendars, SwapError, SwapTerms,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::*;
use crate::futures::{
    FuturesCalendar, FuturesClearingRules, FuturesError, FuturesFixings, RateFuturesValuation,
};

/// Exact dated money market quotation, including an independently pinned calendar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DatedRateHelper {
    /// Actual deposit or FRA dates, index, quotation and settlement convention.
    pub terms: MoneyMarketTerms,
    /// Exact quoted discount or index curve; no automatic mapping.
    pub curve: CurveKey,
    /// Actual value/fixing/settlement calendar version.
    pub calendar: convex_rates::ReferencePin,
}

/// Full exchange rate futures and explicit native curve/model/source bindings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RateFutureCalibrationTerms {
    /// Full dated strict overnight or single-term exchange contract.
    pub contract: FuturesContract,
    /// Actual revision selection and separate final rounding policy.
    pub clearing: FuturesClearingRules,
    /// Exact index projection mapping.
    pub projection_curve: CurveKey,
    /// Actual numeric curve revision/source facts; caller supplies these.
    pub projection_stamp: ReferenceStamp,
    /// Actual history-manifest source/version/quality.
    pub fixing_stamp: ReferenceStamp,
    /// Exact model source for Supplied convexity, absent for explicit zero.
    pub convexity_model: Option<ReferenceStamp>,
}

/// Closed product composition; every branch uses original dated economics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RateCalibrationTerms {
    /// Deposit or publication-aware term FRA.
    MoneyMarket(DatedRateHelper),
    /// IRS or OIS, with separate exact discount/index mappings.
    Swap(Box<SwapTerms>),
    /// Exact single-term/arithmetic/compound exchange window and convexity policy.
    RateFuture(Box<RateFutureCalibrationTerms>),
    /// Independently dated floating legs; native absolute spread on named leg.
    BasisSwap {
        /// Full basis economics.
        terms: Box<BasisSwapTerms>,
        /// Leg whose uniform spread equals the original quote.
        quoted_leg: BasisQuotedLeg,
    },
}

/// Original market instrument and diagnostic pillar; no quote is dropped.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RateCalibrationHelper {
    /// Unique original identity.
    pub id: String,
    /// Explicit last relevant output-curve date, checked against actual queries.
    pub pillar: Date,
    /// Actual full economics and immutable pins.
    pub terms: RateCalibrationTerms,
    /// Original annualized rate/basis or futures price-point quotation.
    pub quote: MarketQuote,
    /// Absolute tolerance in exactly the original native unit.
    pub tolerance: f64,
}
impl RateCalibrationHelper {
    /// Replace a native quote and its uniform quoted economic profile together.
    /// Full calendars, schedules, known fixings and other economic steps survive.
    /// Call the same graph builder afterwards; callers assign new immutable keys
    /// and update exact dependent pins/source stamps when publishing a revision.
    pub fn with_quote(&self, quote: MarketQuote) -> Result<Self, RateCalibrationError> {
        quote.validate()?;
        let mut result = self.clone();
        match &mut result.terms {
            RateCalibrationTerms::RateFuture(_) => {
                if quote.unit != QuoteUnit::FuturePrice100MinusRate {
                    return Err(invalid(
                        "quote.unit",
                        "rate futures require native price points",
                    ));
                }
            }
            _ => {
                let value = quote.decimal_rate()?;
                let amount = Decimal::from_f64_retain(value)
                    .ok_or_else(|| invalid("quote", "Decimal conversion failed"))?;
                if value != 0.0 && amount.is_zero() {
                    return Err(invalid("quote", "nonzero input underflows Decimal"));
                }
                match &mut result.terms {
                    RateCalibrationTerms::Swap(t) => {
                        for p in &mut t.fixed.rates {
                            p.value = amount;
                        }
                    }
                    RateCalibrationTerms::BasisSwap { terms, quoted_leg } => {
                        let leg = match quoted_leg {
                            BasisQuotedLeg::Pay => &mut terms.pay,
                            BasisQuotedLeg::Receive => &mut terms.receive,
                        };
                        match leg {
                            convex_rates::FloatingLegTerms::Term(t) => {
                                for p in &mut t.profiles.spreads {
                                    p.value = amount;
                                }
                            }
                            convex_rates::FloatingLegTerms::Overnight(t) => {
                                for p in &mut t.profiles.spreads {
                                    p.value = amount;
                                }
                            }
                        }
                    }
                    _ => (),
                }
            }
        }
        result.quote = quote;
        Ok(result)
    }
}

/// Duplicate-pillar handling is declared, rather than inferred from input order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum RateOverlapPolicy {
    /// Reject two original helpers with the same diagnostic pillar.
    Reject,
    /// Global only: retain all originals, weights, residuals and native thresholds.
    RetainAllGlobal,
}

/// Closed finite continuous-zero bracket for each linear piecewise root.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RateBootstrapSettings {
    /// Lower decimal continuous zero endpoint.
    pub lower_zero: f64,
    /// Upper decimal continuous zero endpoint.
    pub upper_zero: f64,
    /// Common bounded Brent budget per parameter.
    pub max_iterations: u32,
}

/// Explicit zero-basis overnight projection view of this collateral discount curve.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct OvernightProjectionBinding {
    /// Separate immutable projection identity, included in build order.
    pub key: CurveKey,
    /// Exact overnight index; not inferred from a currency/name.
    pub index_id: String,
}

/// Complete mixed-product curve definition; continuous zeros on dated parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RateCurveRecipe {
    /// Exact curve/role/time/interpolation/extrapolation/dependency/fit contract.
    pub curve: CurveRecipe,
    /// Grid parameter dates, or Parametric coverage dates whose last is the
    /// exact native model horizon. Dates never stand in for model parameters.
    pub pillars: Vec<Date>,
    /// Every original full instrument, native quote and threshold.
    pub helpers: Vec<RateCalibrationHelper>,
    /// Required for fitted discount OIS/futures; forbidden on a projection fit.
    pub overnight_projection: Option<OvernightProjectionBinding>,
    /// Required only in Piecewise; global preserves curve.calibration settings.
    pub bootstrap: Option<RateBootstrapSettings>,
    /// No silent dropping or preferred source inference at an overlap.
    pub overlap_policy: RateOverlapPolicy,
}

/// Actual resource shape must match the helper branch, with exact version pins.
pub enum RateHelperResources<'a> {
    /// One complete supplied value/fixing/settlement calendar.
    MoneyMarket(CalendarBinding<'a>),
    /// Full independent fixed/float/reset/observation resources.
    Swap(SwapCalendars<'a>),
    /// Actual exchange observation calendar and checked clearing history.
    RateFuture {
        /// Calendar plus source quality/version.
        calendar: FuturesCalendar<'a>,
        /// Explicit first-published/latest-eligible immutable history.
        fixings: &'a FuturesFixings,
    },
    /// Complete independent resources for both floating legs.
    BasisSwap(BasisSwapCalendars<'a>),
}

/// Recipe/resources pair; helpers and resources align in original source order.
pub struct RateCurveBuildInput<'a> {
    /// Complete economic/numerical recipe.
    pub recipe: &'a RateCurveRecipe,
    /// Exact supplied calendar/history resources, one per original helper.
    pub resources: &'a [RateHelperResources<'a>],
}

/// Explicit economic instant and information policy; no clock enters fitting.
#[derive(Debug, Clone, Copy)]
pub struct RateCalibrationContext {
    /// Exact swap/FRA history and payment-date state.
    pub cashflows: CashflowContext,
    /// Economic timestamp, whose UTC date must equal the valuation date.
    pub as_of: UtcTimestamp,
}

/// Original domain or measured solver failure, never a partial accepted curve.
#[derive(Debug, thiserror::Error)]
pub enum RateCalibrationError {
    /// Original immutable dependency/query/financial postcondition failure.
    #[error(transparent)]
    Curve(#[from] CurveContractError),
    /// Original complete missing-history/schedule/arithmetic failure.
    #[error(transparent)]
    Cashflow(#[from] CashflowError),
    /// Original swap/basis failure.
    #[error(transparent)]
    Swap(#[from] SwapError),
    /// Original exact futures input/valuation failure.
    #[error(transparent)]
    Futures(#[from] FuturesError),
    /// Actual global objective/rank/iteration diagnostics.
    #[error(transparent)]
    Global(Box<FitFailure<RateCalibrationError>>),
    /// Actual closed bracket/root/iteration failure.
    #[error(transparent)]
    Root(#[from] convex_math::MathError),
}

/// Native money-market implied quote, preserving due history or actual query.
#[derive(Debug, Clone, Serialize)]
pub struct DatedRateValuation {
    /// Original full dates/settlement/calendar/curve pin.
    pub terms: DatedRateHelper,
    /// Native original-unit model quote.
    pub model: MarketQuote,
    /// Exact published fixing or explicit permitted forecast; absent for deposit.
    pub origin: Option<RateOrigin>,
    /// Consumed full actual simple-forward query; absent for a known FRA.
    pub query: Option<CurveQueryValue>,
}

/// Full final product evidence, not only a residual against a tenor proxy.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", content = "valuation", rename_all = "snake_case")]
pub enum RateHelperValuation {
    /// Actual dated simple rate or eligible known FRA.
    MoneyMarket(Box<DatedRateValuation>),
    /// Full signed cash/accrued/PV/par/fixing/curve evidence.
    Swap(Box<convex_rates::pricing::SwapValuation>),
    /// Full weighted known/unfixed window, convexity and source blocks.
    RateFuture(Box<RateFuturesValuation>),
    /// Full independent basis cash/PV/par/known rates and mappings.
    BasisSwap(Box<BasisSwapValuation>),
}

/// Actual bounded numerical diagnostics, separate from native instrument results.
#[derive(Debug, Clone)]
pub enum RateSolverDiagnostics {
    /// Sequential local linear-zero roots; no global interpolation substitution.
    Piecewise {
        /// Measured roots/iterations/native residuals in parameter date order.
        roots: Vec<SolverResult>,
        /// Actual helper valuation count including final repricing.
        evaluations: usize,
    },
    /// Full scaled objective, rank, bounds, steps and measured work.
    Global(FitDiagnostics),
    /// Actual original model parameters and identified full-product objective.
    Parametric {
        /// Solved loadings and decay years; no annualized node interpretation.
        parameters: ParametricModel,
        /// Complete measured work, rank, objective and original-unit residuals.
        diagnostics: FitDiagnostics,
    },
    /// Actual full-product exponential-tail fit over its exact fixed base.
    Ufr {
        /// Solved continuous UFR and positive speed; no annualized node interpretation.
        parameters: UfrParameters,
        /// Complete measured work, rank, objective and original-unit residuals.
        diagnostics: FitDiagnostics,
    },
}

/// Successful complete fitted curve and every original helper's final valuation.
#[derive(Debug, Clone)]
pub struct CalibratedRateCurve {
    /// Exact full original recipe.
    pub recipe: RateCurveRecipe,
    /// Immutable fitted curve with native-unit residual diagnostics.
    pub curve: BuiltCurve,
    /// Full original-order valuations, matching diagnostics.quotes.
    pub repriced_helpers: Vec<RateHelperValuation>,
    /// Actual selected algorithm's outcomes.
    pub solver: RateSolverDiagnostics,
}

/// All-or-error immutable graph output.
#[derive(Debug, Clone)]
pub struct RateCurveBuildOutput {
    /// Supplied exact dependencies, fitted curves and explicit OIS views.
    pub curves: CurveSet,
    /// Deterministic dependency-first identity order, including projection views.
    pub build_order: Vec<CurveKey>,
    /// Full final product and solver evidence in fitted curve order.
    pub calibrations: Vec<CalibratedRateCurve>,
    /// Frozen administrator times in deterministic index/date order.
    pub publications: Vec<FixingRequirement>,
    /// Actual helper valuations, enforcing the separate graph work ceiling.
    pub price_evaluations: usize,
}
