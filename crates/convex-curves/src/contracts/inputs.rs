//! Explicit economics, units and identities used by curve construction.

use convex_core::{
    daycounts::DayCountConvention,
    types::{Compounding, Currency, Date},
};
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};

use super::{CurveContractError as Error, CurveContractResult as Result, MAX_CURVE_TENOR};
use crate::{ExtrapolationMethod, InterpolationMethod};

/// Immutable caller-assigned identity. Version changes are explicit; no hashing
/// of provider identifiers or automatic version selection occurs here.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CurveKey {
    /// Provider-neutral curve identity.
    pub id: String,
    /// Exact definition/market/calendar/model version selected by the caller.
    pub version: String,
}
impl CurveKey {
    /// Validate bounded, nonempty identity and version.
    pub fn validate(&self) -> Result<()> {
        validate_label(&self.id, "curve.id")?;
        validate_label(&self.version, "curve.version")
    }
}

/// Native units are preserved in diagnostics; normalization is explicit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum QuoteUnit {
    /// Annualized decimal rate (0.05 = 5%).
    DecimalRate,
    /// Annualized percentage rate (5 = 5%).
    PercentRate,
    /// Annualized basis points (500 = 5%).
    BasisPoints,
    /// Dimensionless positive discount factor; values above one are allowed.
    DiscountFactor,
    /// Dimensionless survival probability in (0,1].
    SurvivalProbability,
    /// Bond currency price per 100 face, with clean/dirty terms elsewhere.
    PricePer100,
    /// Rate future price in points: 100 minus 100 times decimal rate.
    FuturePrice100MinusRate,
    /// Signed CDS clean/dirty upfront as a fraction of notional.
    UpfrontFraction,
}

/// Finite value in an explicitly named native unit. DTOs are validated on use.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct MarketQuote {
    /// Native quote, without implicit scaling.
    pub value: f64,
    /// Native unit.
    pub unit: QuoteUnit,
}
impl MarketQuote {
    /// Validate finite values and bounded numeric ingress. Rate units additionally
    /// enforce -10..10 in decimal annualized units; negative rates remain valid.
    pub fn validate(&self) -> Result<()> {
        if !self.value.is_finite() || self.value.abs() > 1e12 {
            return Err(Error::invalid(
                "quote",
                "must be finite with magnitude <= 1e12",
            ));
        }
        match self.unit {
            QuoteUnit::DecimalRate | QuoteUnit::PercentRate | QuoteUnit::BasisPoints => {
                let scale = self.rate_scale()?;
                if (self.value / scale).abs() > 10.0 {
                    return Err(Error::invalid("rate", "decimal magnitude exceeds 10"));
                }
            }
            QuoteUnit::DiscountFactor if self.value <= 0.0 => {
                return Err(Error::invalid("discount_factor", "must be positive"))
            }
            QuoteUnit::SurvivalProbability if self.value <= 0.0 || self.value > 1.0 => {
                return Err(Error::invalid("survival_probability", "must be in (0,1]"))
            }
            QuoteUnit::PricePer100 if self.value < 0.0 => {
                return Err(Error::invalid(
                    "price",
                    "must be nonnegative for the declared bond helper",
                ))
            }
            QuoteUnit::UpfrontFraction if self.value.abs() > 10.0 => {
                return Err(Error::invalid("upfront", "fraction magnitude exceeds 10"))
            }
            QuoteUnit::FuturePrice100MinusRate if ((100.0 - self.value) / 100.0).abs() > 10.0 => {
                return Err(Error::invalid(
                    "future_price",
                    "implied decimal rate magnitude exceeds 10",
                ))
            }
            _ => (),
        }
        Ok(())
    }
    fn rate_scale(&self) -> Result<f64> {
        match self.unit {
            QuoteUnit::DecimalRate => Ok(1.0),
            QuoteUnit::PercentRate => Ok(100.0),
            QuoteUnit::BasisPoints => Ok(10_000.0),
            _ => Err(Error::invalid(
                "quote.unit",
                "an annualized rate unit is required",
            )),
        }
    }
    /// Normalize only an explicitly annualized rate unit; prices require a
    /// product convention and cannot be silently interpreted as rates.
    pub fn decimal_rate(&self) -> Result<f64> {
        self.validate()?;
        Ok(self.value / self.rate_scale()?)
    }
    /// Convert a model decimal rate back into this quote's original rate unit.
    pub fn model_rate_quote(&self, decimal_rate: f64) -> Result<Self> {
        let quote = Self {
            value: decimal_rate * self.rate_scale()?,
            unit: self.unit,
        };
        quote.validate()?;
        Ok(quote)
    }
}

/// Semantic curve role; identities and credit assumptions never come from a vendor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum CurveRole {
    /// Discounting under an explicit collateral currency.
    Discount {
        /// Collateral currency.
        collateral_currency: Currency,
    },
    /// Projection for a specified index.
    Projection {
        /// Index identity.
        index_id: String,
    },
    /// Government benchmark curve, distinct from collateral discounting.
    Government {
        /// Benchmark identity.
        benchmark_id: String,
    },
    /// Survival curve for one entity and seniority under supplied recovery.
    Credit {
        /// Reference entity identity.
        entity_id: String,
        /// Debt seniority contract.
        seniority: String,
        /// Recovery fraction in `[0,1]`.
        recovery: f64,
    },
    /// Explicit issuer/bond residual basis. Raw spread construction lives in
    /// curves; full cash/model composition lives in analytics cash_calibration.
    IssuerSpread {
        /// Issuer identity.
        entity_id: String,
        /// Declared model identity.
        model_id: String,
    },
    /// Repo/funding curve under an explicit collateral definition.
    Repo {
        /// Collateral identity.
        collateral_id: String,
    },
    /// Volatility/model inputs (construction pending).
    Volatility {
        /// Declared model identity.
        model_id: String,
    },
}
impl CurveRole {
    pub(crate) fn validate(&self) -> Result<()> {
        match self {
            Self::Discount { .. } => Ok(()),
            Self::Projection { index_id } => validate_label(index_id, "index_id"),
            Self::Government { benchmark_id } => validate_label(benchmark_id, "benchmark_id"),
            Self::Credit {
                entity_id,
                seniority,
                recovery,
            } => {
                validate_label(entity_id, "entity_id")?;
                validate_label(seniority, "seniority")?;
                if !recovery.is_finite() || !(0.0..=1.0).contains(recovery) {
                    return Err(Error::invalid("recovery", "must be finite in [0,1]"));
                }
                Ok(())
            }
            Self::IssuerSpread {
                entity_id,
                model_id,
            } => {
                validate_label(entity_id, "entity_id")?;
                validate_label(model_id, "model_id")
            }
            Self::Repo { collateral_id } => validate_label(collateral_id, "collateral_id"),
            Self::Volatility { model_id } => validate_label(model_id, "model_id"),
        }
    }
    pub(crate) fn is_rate(&self) -> bool {
        matches!(
            self,
            Self::Discount { .. }
                | Self::Projection { .. }
                | Self::Government { .. }
                | Self::Repo { .. }
        )
    }
}

/// Representation determines interpolation meaning; no spread-to-hazard shortcut.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum CurveRepresentation {
    /// Positive discount factors with explicit P(0)=1.
    DiscountFactors,
    /// Zero rates in explicit compounding and the recipe's time basis.
    ZeroRates {
        /// Rate compounding.
        compounding: Compounding,
    },
    /// Nonincreasing positive survival probabilities with Q(0)=1.
    SurvivalProbabilities,
    /// Instantaneous hazard nodes; integration contract pending CVX-021.
    HazardRates,
    /// Signed annualized residual basis nodes, distinct from discount factors
    /// and hazard rates; valuation requires an explicit analytics pricing model.
    IssuerSpreads,
}

/// Exact calendar date or years in the recipe's declared time basis.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum CurvePoint {
    /// Actual date, with no tenor-to-calendar approximation.
    Date(Date),
    /// Continuous model time; cannot implicitly change day-count basis.
    Tenor(f64),
}

/// A pillar quote; sorted deterministically during construction, duplicates rejected.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CurveNode {
    /// Exact pillar date or time coordinate.
    pub pillar: CurvePoint,
    /// Quote in representation-compatible units.
    pub quote: MarketQuote,
}

/// FRA payment convention retained even though a native fair-rate query is
/// independent of notional and deterministic settlement scaling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum FraSettlement {
    /// Net settlement at accrual start.
    AtStart,
    /// Net settlement at accrual end.
    AtEnd,
}

/// Full dates for supported simple-rate calibration helpers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum MoneyMarketTerms {
    /// Deposit: principal exchanged at start, principal plus simple interest at end.
    Deposit {
        /// Value/start date.
        start_date: Date,
        /// Principal and interest payment date.
        maturity_date: Date,
        /// Interest accrual basis.
        day_count: DayCountConvention,
    },
    /// Unfixed term FRA; seasoned historic fixings require the product route.
    Fra {
        /// Index identity.
        index_id: String,
        /// Contractual fixing date.
        fixing_date: Date,
        /// Accrual start.
        start_date: Date,
        /// Accrual end.
        end_date: Date,
        /// Actual settlement date.
        settlement_date: Date,
        /// Settlement convention.
        settlement: FraSettlement,
        /// Interest accrual basis.
        day_count: DayCountConvention,
    },
}
impl MoneyMarketTerms {
    /// Exact start/end/accrual basis, independent of the curve's time axis.
    pub fn period(&self) -> (Date, Date, DayCountConvention) {
        match self {
            Self::Deposit {
                start_date,
                maturity_date,
                day_count,
            } => (*start_date, *maturity_date, *day_count),
            Self::Fra {
                start_date,
                end_date,
                day_count,
                ..
            } => (*start_date, *end_date, *day_count),
        }
    }
}

/// Native quote and full terms, with an explicit absolute tolerance in that unit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct MoneyMarketHelper {
    /// Unique helper/input identity.
    pub id: String,
    /// Complete supported dated terms.
    pub terms: MoneyMarketTerms,
    /// Original market quote unit/value.
    pub quote: MarketQuote,
    /// Absolute repricing tolerance in quote.unit; never widened by the builder.
    pub tolerance: f64,
}
impl MoneyMarketHelper {
    /// Validate terms and native quote, without consulting a curve or fixing service.
    pub fn validate(&self, reference_date: Date) -> Result<()> {
        validate_label(&self.id, "helper.id")?;
        self.quote.decimal_rate()?;
        validate_tolerance(self.tolerance)?;
        let (start, end, dc) = self.terms.period();
        if start < reference_date {
            return Err(Error::invalid(
                "helper.start",
                "precedes curve reference date",
            ));
        }
        year_fraction(start, end, dc)?;
        if let MoneyMarketTerms::Fra {
            index_id,
            fixing_date,
            settlement_date,
            settlement,
            ..
        } = &self.terms
        {
            validate_label(index_id, "index_id")?;
            validate_date(*fixing_date)?;
            validate_date(*settlement_date)?;
            if *fixing_date > start {
                return Err(Error::invalid("fixing_date", "follows accrual start"));
            }
            if *fixing_date <= reference_date {
                return Err(Error::unsupported(
                    "seasoned FRA",
                    "supply eligible fixings through the product calibration route",
                ));
            }
            let expected = match settlement {
                FraSettlement::AtStart => start,
                FraSettlement::AtEnd => end,
            };
            if *settlement_date != expected {
                return Err(Error::unsupported(
                    "FRA settlement delay",
                    "the simple-rate route requires settlement on the declared boundary",
                ));
            }
        }
        Ok(())
    }
}

/// Explicit global objective configuration, preserved even when that combination
/// is unsupported by this checked route. Existing global fitter remains separate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct GlobalFitSettings {
    /// Maximum solver steps; no hidden unbounded iteration.
    pub max_iterations: usize,
    /// Absolute scaled objective convergence threshold.
    pub objective_tolerance: f64,
    /// Initial decimal zero rates, one per selected pillar.
    pub initial_zeros: Vec<f64>,
    /// Lower decimal zero-rate bound.
    pub lower_zero: f64,
    /// Upper decimal zero-rate bound.
    pub upper_zero: f64,
    /// Native quote scales, one per helper; objective residual = residual / scale.
    pub quote_scales: Vec<f64>,
    /// Nonnegative helper weights, one per helper.
    pub weights: Vec<f64>,
}

/// Credit objective over nonnegative piecewise ACT/365F forward intensities.
/// These are intensities, not zero rates or CDS spreads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CreditGlobalFitSettings {
    /// Bounded LM attempt count.
    pub max_iterations: usize,
    /// Weighted scaled RMS convergence target; native residuals also must pass.
    pub objective_tolerance: f64,
    /// One intensity per increasing parameter interval.
    pub initial_hazards: Vec<f64>,
    /// Closed lower intensity bound, at least zero.
    pub lower_hazard: f64,
    /// Closed upper intensity bound, at most five per ACT/365F year.
    pub upper_hazard: f64,
    /// Positive native scales, in original helper order.
    pub quote_scales: Vec<f64>,
    /// Nonnegative weights, in original helper order; zero never waives repricing.
    pub weights: Vec<f64>,
}

/// Construction method; no mode is silently replaced by another algorithm.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub enum CalibrationMode {
    /// Direct nodes, without instrument calibration.
    Direct,
    /// Exact supported dated deposit/FRA chain, final native repricing.
    Piecewise,
    /// Global objective, requiring a supported product composition.
    Global(GlobalFitSettings),
    /// Credit fit with explicitly different parameter semantics from rate zeros.
    CreditGlobal(CreditGlobalFitSettings),
}

/// Source values are supplied, never fetched. Product helpers reside in analytics
/// to avoid making curves depend on products that already use curves.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub enum CurveSource {
    /// Original analytic exponential forward-convergence tail joined to an
    /// exact immutable continuous-zero model. Parameters are supplied in the
    /// required UfrConvergence extrapolation policy, not inferred by currency.
    UfrConvergence {
        /// Full FlatForward or original parametric source at its exact version.
        base: CurveKey,
        /// Actual supplied date/model time inside the source's native domain.
        /// The original left forward owns the join; no source tail is consumed.
        last_liquid_point: CurvePoint,
    },
    /// Exact supplied parametric model, not a sampled node-grid alias. Dated
    /// observations must all reprice; fitting uses `fit_parametric_curve`.
    Parametric {
        /// Complete annualized continuous-zero parameters and model family.
        parameters: super::ParametricModel,
        /// Explicit finite supported model horizon, in this recipe's time basis.
        horizon: CurvePoint,
        /// All supplied native-unit zero observations and exact quote revisions.
        observations: Vec<super::ParametricObservation>,
    },
    /// Original delegated raw-value semantics over an exact immutable version.
    Delegated {
        /// Same role/currency/reference/time basis/representation dependency.
        base: CurveKey,
        /// Explicit override of the native delegated interval.
        range: super::CurveRange,
        /// Original checked strict/trust/clamp/flat behavior, never inferred.
        fallback: crate::curves::DelegationFallback,
    },
    /// Original segmented raw values; each segment resolves a complete exact
    /// source recipe, so its actual interpolation and tail policy survive.
    Segmented {
        /// Contiguous ordered intervals; the right segment owns each join.
        segments: Vec<super::CurveSegmentInput>,
        /// Absolute raw-value continuity bound at every join, never relaxed.
        join_tolerance: f64,
    },
    /// Right-owned step continuous-zero rate grid; no linear interpolation proxy.
    PiecewiseConstant {
        /// Complete actual dates/model times and native rate-unit quotes.
        nodes: Vec<CurveNode>,
    },
    /// Continuous-zero rate grid integrating the original native flat forwards.
    FlatForward {
        /// Complete actual dates/model times and native rate-unit quotes.
        nodes: Vec<CurveNode>,
    },
    /// Full node grid in a declared representation.
    Nodes(Vec<CurveNode>),
    /// Supported exact simple-rate helper chain.
    MoneyMarket(Vec<MoneyMarketHelper>),
    /// Product helper identities resolved by the pure analytics composition layer.
    Helpers(Vec<String>),
    /// Add an annualized continuous zero spread to an exact base curve version.
    ContinuousZeroSpread {
        /// Exact base version.
        base: CurveKey,
        /// Spread in a rate unit.
        spread: MarketQuote,
    },
    /// Exact view of the base discount function in the recipe's DF/zero
    /// representation; the base interpolation is not resampled/reinterpreted.
    DiscountEquivalent {
        /// Exact base version; currency, role, reference/time basis and methods
        /// must match. Only the output representation may change.
        base: CurveKey,
    },
    /// Explicit zero-basis overnight-index model: the index projection discount
    /// function equals a same-currency collateral discount function. This is a
    /// declared economic mapping, never inferred from an index name or currency.
    OvernightProjection {
        /// Exact collateral discount curve version.
        discount: CurveKey,
        /// Actual overnight index, matching the output Projection role.
        index_id: String,
    },
    /// Explicit forward reference-date movement of an immutable rate curve.
    /// Output is DiscountFactors; no implicit sticky policy or date approximation.
    Rolled {
        /// Actual immutable source curve version.
        base: CurveKey,
        /// Mathematical meaning of reference-date movement.
        policy: super::RollPolicy,
    },
}

/// Complete supplied recipe. Native construction supports the capability subset
/// returned by curve_capabilities; all other methods return Unsupported.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CurveRecipe {
    /// Exact result identity/version.
    pub key: CurveKey,
    /// Semantic purpose and explicit entity/index/collateral assumptions.
    pub role: CurveRole,
    /// Currency of the economic inputs.
    pub currency: Currency,
    /// Supplied valuation/reference date.
    pub reference_date: Date,
    /// Date-to-model-time basis (ACT/360, ACT/365F or ACT/ACT ISDA).
    pub time_basis: DayCountConvention,
    /// Interpolated value meaning and compounding.
    pub representation: CurveRepresentation,
    /// Explicit method; unsupported methods fail.
    pub interpolation: InterpolationMethod,
    /// Explicit extension policy; supported raw Flat/Linear semantics are disclosed.
    pub extrapolation: ExtrapolationMethod,
    /// Exact dependency versions. Must match the source's consumed dependency set.
    pub dependencies: Vec<CurveKey>,
    /// Construction/calibration mode and numerical configuration.
    pub calibration: CalibrationMode,
    /// Actual quotes or pure helper inputs.
    pub source: CurveSource,
}

/// Stable diagnostic for every original helper quote.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct QuoteRepricing {
    /// Original helper identity.
    pub helper_id: String,
    /// Actual pillar date.
    pub pillar: Date,
    /// Original native quote.
    pub input: MarketQuote,
    /// Model quote in the original native unit.
    pub model: MarketQuote,
    /// Model minus input in the original unit.
    pub residual: f64,
    /// Supplied absolute native-unit tolerance.
    pub tolerance: f64,
}

/// Construction diagnostics distinguish supplied nodes from calibrated quotes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct CurveDiagnostics {
    /// Result identity/version.
    pub curve: CurveKey,
    /// Exact algorithm used; direct nodes carry no repricing claim.
    pub method: String,
    /// Solver iterations; zero for direct/analytic construction.
    pub iterations: usize,
    /// Sorted model-time pillars, including explicit mathematical origin if needed.
    pub pillars: Vec<f64>,
    /// All resolved dependency versions.
    pub dependencies: Vec<CurveKey>,
    /// All native helper repricing outcomes; empty for supplied direct nodes.
    pub quotes: Vec<QuoteRepricing>,
}

pub(crate) fn validate_label(value: &str, field: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(Error::invalid(
            field,
            "must be nonempty, <=128 bytes, trimmed and without control characters",
        ));
    }
    Ok(())
}
pub(crate) fn validate_date(date: Date) -> Result<()> {
    if !(1..=9999).contains(&date.year()) {
        return Err(Error::invalid("date", "year outside 0001..9999"));
    }
    Ok(())
}
pub(crate) fn validate_tolerance(value: f64) -> Result<()> {
    if !value.is_finite() || value <= 0.0 {
        return Err(Error::invalid(
            "tolerance",
            "must be finite and positive in the native quote unit",
        ));
    }
    Ok(())
}
pub(crate) fn validate_time_basis(dc: DayCountConvention) -> Result<()> {
    if !matches!(
        dc,
        DayCountConvention::Act360
            | DayCountConvention::Act365Fixed
            | DayCountConvention::ActActIsda
    ) {
        return Err(Error::unsupported(
            "curve time basis",
            "supports ACT/360, ACT/365 Fixed and ACT/ACT ISDA",
        ));
    }
    Ok(())
}
pub(crate) fn validate_tenor(t: f64) -> Result<()> {
    if !t.is_finite() || !(0.0..=MAX_CURVE_TENOR).contains(&t) {
        return Err(Error::invalid("tenor", "must be finite in [0,150]"));
    }
    Ok(())
}
/// Actual year fraction for supported context-free query/helper conventions.
/// ICMA and termination-sensitive bases require product/reference context.
pub fn year_fraction(start: Date, end: Date, dc: DayCountConvention) -> Result<f64> {
    validate_date(start)?;
    validate_date(end)?;
    if end <= start || start.days_between(&end) > convex_core::cashflows::MAX_CONTRACT_DAYS {
        return Err(Error::invalid(
            "period",
            "end must follow start within 150x366 calendar days",
        ));
    }
    if !matches!(
        dc,
        DayCountConvention::Act360
            | DayCountConvention::Act365Fixed
            | DayCountConvention::ActActIsda
            | DayCountConvention::Thirty360US
            | DayCountConvention::Thirty360E
    ) {
        return Err(Error::unsupported("year fraction", "this query needs a context-free ACT/360, ACT/365F, ACT/ACT ISDA, 30/360 US or 30E/360 convention"));
    }
    let result = dc
        .to_day_count()
        .year_fraction(start, end)
        .to_f64()
        .ok_or_else(|| Error::Numerical("year fraction conversion".into()))?;
    if !result.is_finite() || result <= 0.0 {
        return Err(Error::invalid("period", "year fraction must be positive"));
    }
    Ok(result)
}
