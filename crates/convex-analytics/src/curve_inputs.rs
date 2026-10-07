//! Dated pure product helper descriptions and composition, above curve solvers.
//!
//! Descriptions preserve economic terms and reuse core schedules/fixings. Only
//! supported money-market helpers dispatch to construction here. Full future,
//! bond, swap/basis and CDS calibration is explicitly pending its product task.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use convex_core::{
    calendars::Calendar,
    cashflows::{
        CashRounding, CouponPrincipal, CouponProfiles, CouponSchedule, DateRules, DatedProfile,
        DatedValue, ExCouponAccrual, NegativeCouponPolicy, OvernightConvention, OvernightSchedule,
        PaymentDateEntitlement, ProfileKind, ScheduleSpec,
    },
    contracts::{BusinessDate, IndexId},
    daycounts::DayCountConvention,
    fixings::{FixingContext, FixingRequirement, FixingSeries},
    types::{Compounding, Currency, Date},
};
use convex_curves::contracts::{
    build_curve_set, CalibrationMode, CurveBuildOutput, CurveCapabilityStatus,
    CurveContractError as Error, CurveContractResult as Result, CurveKey, CurveRecipe, CurveSet,
    CurveSource, MarketQuote, MoneyMarketHelper, MoneyMarketTerms, QuoteUnit, MAX_CURVES,
    MAX_CURVE_INPUTS,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Versioned supplied calendar/convention identity. It is independent of the
/// source vendor, and cannot be replaced by a name-only calendar fallback.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ReferencePin {
    /// Internal resource identity.
    pub id: String,
    /// Exact resource version selected for this build.
    pub version: String,
}
impl ReferencePin {
    fn validate(&self) -> Result<()> {
        label(&self.id, "reference.id")?;
        label(&self.version, "reference.version")
    }
}

/// Actual immutable calendar binding supplied by the embedding application.
pub struct CalendarBinding<'a> {
    /// Exact version represented by calendar.
    pub pin: ReferencePin,
    /// Pure calendar implementation; callers ensure bounded/panic-free callbacks.
    pub calendar: &'a dyn Calendar,
}

/// Actual resolved resources, with no repositories, global time or network access.
pub struct HelperResources<'a> {
    /// Supplied versioned calendars (maximum 256, no duplicate identities).
    pub calendars: Vec<CalendarBinding<'a>>,
    /// Already validated immutable fixing revisions and publication/receipt times.
    pub fixings: &'a FixingSeries,
}
impl HelperResources<'_> {
    fn validate(&self) -> Result<()> {
        if self.calendars.len() > MAX_CURVES {
            return Err(Error::BoundExceeded("helper calendars"));
        }
        let mut seen = BTreeSet::new();
        for binding in &self.calendars {
            binding.pin.validate()?;
            if !seen.insert(&binding.pin.id) {
                return Err(Error::InvalidInput {
                    field: "calendars".into(),
                    reason: "duplicate calendar identity".into(),
                });
            }
        }
        Ok(())
    }
    fn calendar(&self, pin: &ReferencePin) -> Result<&dyn Calendar> {
        pin.validate()?;
        self.calendars
            .iter()
            .find(|binding| binding.pin == *pin)
            .map(|binding| binding.calendar)
            .ok_or_else(|| invalid("calendar", "missing exact supplied calendar version"))
    }
}

/// Full reusable schedule contract for one stated calendar. Distinct legs may
/// choose different calendars/rules; multi-calendar rules within a leg require
/// an explicit future extension rather than silently reusing this calendar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct HelperSchedule {
    /// Exact contractual schedule and irregular reference periods.
    pub schedule: ScheduleSpec,
    /// Separate accrual/payment/fixing/observation/ex dates.
    pub dates: DateRules,
    /// Contractual accrual convention, including reference-period ICMA.
    pub day_count: DayCountConvention,
    /// Exact calendar used by this schedule's date rules.
    pub calendar: ReferencePin,
}
impl HelperSchedule {
    fn generate(&self, resources: &HelperResources<'_>) -> Result<CouponSchedule> {
        let generated = CouponSchedule::generate(
            &self.schedule,
            &self.dates,
            resources.calendar(&self.calendar)?,
        )?;
        for period in generated.periods() {
            let (start, end) = period.accrual_dates();
            period.year_fraction(start, end, self.day_count)?;
        }
        Ok(generated)
    }
}

/// Native bond quotation; yield carries its full convention, never a par-yield proxy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub enum BondHelperQuote {
    /// Price excludes accrued according to the declared ex-coupon rule.
    CleanPrice,
    /// Price includes accrued.
    DirtyPrice,
    /// Yield quotation under explicit rate basis/frequency and compounding.
    Yield {
        /// Compounding of the yield quote.
        compounding: Compounding,
        /// Yield quotation basis.
        day_count: DayCountConvention,
    },
}

/// Complete deterministic fixed/stepped/amortizing bond helper terms. Optional
/// and floating bonds require their own product/model helper extensions; they
/// cannot be accepted and priced as this fixed contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FixedCouponBondHelper {
    /// Internal security identity, independent of a provider ticker.
    pub instrument_id: String,
    /// Exact security definition version.
    pub instrument_version: String,
    /// Issue date distinct from coupon accrual start.
    pub issue_date: Date,
    /// Settlement for the original price/yield.
    pub settlement_date: Date,
    /// Full coupon schedule and date rules.
    pub coupon_schedule: HelperSchedule,
    /// Independently dated coupon rates in annualized decimals.
    #[serde(with = "exact_profiles")]
    #[cfg_attr(feature = "schemars", schemars(with = "Vec<DatedValue>"))]
    pub coupons: Vec<DatedValue>,
    /// Independently dated absolute outstanding principal (not percentages).
    #[serde(with = "exact_profiles")]
    #[cfg_attr(feature = "schemars", schemars(with = "Vec<DatedValue>"))]
    pub notionals: Vec<DatedValue>,
    /// Face amount to which a per-100 input quote refers, independent of residual principal.
    #[serde(with = "exact_decimal")]
    #[cfg_attr(feature = "schemars", schemars(with = "String"))]
    pub quoted_face: Decimal,
    /// Final principal redemption as a fraction, not per 100.
    #[serde(with = "exact_decimal")]
    #[cfg_attr(feature = "schemars", schemars(with = "String"))]
    pub redemption_fraction: Decimal,
    /// Explicit pre/post coupon-boundary notional selection.
    pub coupon_principal: CouponPrincipal,
    /// Contractual final cash rounding.
    pub cash_rounding: CashRounding,
    /// Explicit negative coupon treatment.
    pub negative_coupons: NegativeCouponPolicy,
    /// Accrued rule in the ex-coupon interval.
    pub ex_coupon_accrual: ExCouponAccrual,
    /// Settlement-on-payment entitlement.
    pub on_payment_date: PaymentDateEntitlement,
    /// Original native clean/dirty/yield quotation convention.
    pub quote_convention: BondHelperQuote,
}

/// Declared convexity policy. Zero is an explicit assumption with a versioned
/// rationale, not the result of missing volatility/model inputs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub enum FutureConvexity {
    /// Supplied annualized adjustment; forward rate = future rate - adjustment.
    Supplied {
        /// Decimal/percent/bp adjustment.
        adjustment: MarketQuote,
        /// Assumption/model version.
        model_version: String,
    },
    /// Model calculation is representable but pending product model implementation.
    Model {
        /// Model identity.
        model_id: String,
        /// Exact parameter/volatility version.
        model_version: String,
    },
}

/// Rate-future settlement-rate definition; simple and compounded overnight
/// products are distinct, and retain their exact observation window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub enum FutureAccrual {
    /// Single term fixing for the contractual accrual period.
    Term {
        /// Index identity.
        index_id: IndexId,
        /// Contractual fixing date.
        fixing_date: Date,
    },
    /// Arithmetic daily average of overnight observations.
    OvernightAverage {
        /// Index identity.
        index_id: IndexId,
        /// Lookback/shift/lockout terms.
        convention: OvernightConvention,
    },
    /// Geometric compound product of overnight observations.
    OvernightCompounded {
        /// Index identity.
        index_id: IndexId,
        /// Lookback/shift/lockout terms.
        convention: OvernightConvention,
    },
}

/// Full rate-future description; no assumed IMM/3M window or unqualified convexity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RateFutureHelper {
    /// Exact contract identity.
    pub contract_id: String,
    /// Exact contract terms version.
    pub contract_version: String,
    /// Actual accrual/settlement-rate window start.
    pub accrual_start: Date,
    /// Actual accrual/settlement-rate window end.
    pub accrual_end: Date,
    /// Last trading date, distinct from accrual or final cash date.
    pub last_trading_date: Date,
    /// Final cash settlement date.
    pub final_settlement_date: Date,
    /// Observation calendar and version.
    pub observation_calendar: ReferencePin,
    /// Final settlement calendar and version.
    pub settlement_calendar: ReferencePin,
    /// Day-count weights for the settlement rate.
    pub day_count: DayCountConvention,
    /// Term/arithmetic/compounded definition.
    pub accrual: FutureAccrual,
    /// Contract quote tick in price points.
    #[serde(with = "exact_decimal")]
    #[cfg_attr(feature = "schemars", schemars(with = "String"))]
    pub tick_size: Decimal,
    /// Money per one price point per contract, in helper currency.
    #[serde(with = "exact_decimal")]
    #[cfg_attr(feature = "schemars", schemars(with = "String"))]
    pub price_multiplier: Decimal,
    /// Money per tick per contract; must equal tick_size * price_multiplier.
    #[serde(with = "exact_decimal")]
    #[cfg_attr(feature = "schemars", schemars(with = "String"))]
    pub tick_value: Decimal,
    /// Explicit convexity policy.
    pub convexity: FutureConvexity,
    /// Every required observation with supplied publication instants.
    pub fixing_requirements: Vec<FixingRequirement>,
    /// Valuation date, information cutoff and observation eligibility policy.
    pub fixing_context: FixingContext,
}

/// CDS original quotation, including sign and clean/dirty upfront semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum CdsHelperQuote {
    /// Par running spread in an annualized rate unit.
    ParSpread,
    /// Upfront excluding premium accrued, positive paid by protection buyer.
    CleanUpfront,
    /// Upfront including premium accrued, same protection-buyer sign.
    DirtyUpfront,
}

/// Protection settlement convention; no accrual/cash date is inferred.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub enum ProtectionPayment {
    /// Payment at default plus a nonnegative calendar lag.
    AtDefault {
        /// Contractual calendar-day settlement lag.
        calendar_days: u32,
    },
    /// Protection paid at this fixed date.
    FixedDate(Date),
}

/// Complete declared single-name premium/protection helper description. Standard
/// date/model conversion and pricing are CVX-020/058, not a spread/LGD shortcut.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CdsHelper {
    /// Reference entity identity and exact definition version.
    pub reference_entity: ReferencePin,
    /// Debt seniority contract.
    pub seniority: String,
    /// Documentation/convention version (e.g. supplied ISDA pack).
    pub documentation: ReferencePin,
    /// Original trade date.
    pub trade_date: Date,
    /// Step-in date for the priced quote.
    pub step_in_date: Date,
    /// Upfront cash settlement date.
    pub cash_settlement_date: Date,
    /// Protection start, distinct from premium accrued start.
    pub protection_start: Date,
    /// Protection end.
    pub protection_end: Date,
    /// Inclusion of protection end.
    pub protection_end_inclusive: bool,
    /// Explicit premium accrual start, schedule, dates and calendar.
    pub premium_schedule: HelperSchedule,
    /// Independently dated absolute protection/premium notional.
    #[serde(with = "exact_profiles")]
    #[cfg_attr(feature = "schemars", schemars(with = "Vec<DatedValue>"))]
    pub notionals: Vec<DatedValue>,
    /// Contractual fixed running premium, in an annualized rate unit.
    pub running_coupon: MarketQuote,
    /// Recovery fraction in `[0,1]`.
    pub recovery: f64,
    /// Whether accrued premium is paid on default.
    pub settles_accrual_on_default: bool,
    /// Protection cash timing.
    pub protection_payment: ProtectionPayment,
    /// Original quote meaning and sign convention.
    pub quote_convention: CdsHelperQuote,
    /// Exact risk-free discount dependency, not a sovereign fallback.
    pub discount_curve: CurveKey,
}

/// Fixed swap leg with independently dated notional and coupon steps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FixedHelperLeg {
    /// Full fixed schedule.
    pub schedule: HelperSchedule,
    /// Annualized decimal coupons.
    #[serde(with = "exact_profiles")]
    #[cfg_attr(feature = "schemars", schemars(with = "Vec<DatedValue>"))]
    pub coupons: Vec<DatedValue>,
    /// Absolute principal profile.
    #[serde(with = "exact_profiles")]
    #[cfg_attr(feature = "schemars", schemars(with = "Vec<DatedValue>"))]
    pub notionals: Vec<DatedValue>,
}

/// Full floating-index definition for a leg. Historical publication/cutoff terms
/// are retained in the leg instead of a date-only map.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub enum FloatingHelperIndex {
    /// Term index with explicit tenor; no default 3M assumption.
    Term {
        /// Provider-neutral index.
        index_id: IndexId,
        /// Contractual tenor in months.
        tenor_months: u32,
    },
    /// Overnight compounded index.
    Overnight {
        /// Provider-neutral index.
        index_id: IndexId,
        /// Observation terms.
        convention: OvernightConvention,
    },
}

/// Floating swap leg; projection and discount dependencies are separate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FloatingHelperLeg {
    /// Full independent floating schedule.
    pub schedule: HelperSchedule,
    /// Term/overnight index.
    pub index: FloatingHelperIndex,
    /// Absolute principal profile.
    #[serde(with = "exact_profiles")]
    #[cfg_attr(feature = "schemars", schemars(with = "Vec<DatedValue>"))]
    pub notionals: Vec<DatedValue>,
    /// Independent annualized decimal spread steps.
    #[serde(with = "exact_profiles")]
    #[cfg_attr(feature = "schemars", schemars(with = "Vec<DatedValue>"))]
    pub spreads: Vec<DatedValue>,
    /// Independent dimensionless gearing steps.
    #[serde(with = "exact_profiles")]
    #[cfg_attr(feature = "schemars", schemars(with = "Vec<DatedValue>"))]
    pub gearings: Vec<DatedValue>,
    /// Exact projection dependency.
    pub projection_curve: CurveKey,
    /// Complete expected publications.
    pub fixing_requirements: Vec<FixingRequirement>,
    /// Supplied fixing cutoff/policy.
    pub fixing_context: FixingContext,
}

/// Which floating leg carries the market basis-spread quote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum BasisQuoteLeg {
    /// First/pay leg.
    Pay,
    /// Second/receive leg.
    Receive,
}

/// Economic descriptions retained above the curve solver to avoid dependency cycles.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub enum CurveHelperTerms {
    /// Dated deposit/FRA simple-rate terms; supported native construction.
    MoneyMarket(MoneyMarketTerms),
    /// Rate-future full contract, observation and convexity terms.
    RateFuture(RateFutureHelper),
    /// Fixed/stepped/amortizing bond with original settlement/quotation.
    FixedCouponBond(FixedCouponBondHelper),
    /// Standard declared single-name CDS economics.
    Cds(CdsHelper),
    /// IRS or OIS, with independent fixed/float legs and discount mapping.
    Swap {
        /// Fixed leg terms.
        fixed: FixedHelperLeg,
        /// Floating leg terms.
        floating: FloatingHelperLeg,
        /// Exact discount curve.
        discount_curve: CurveKey,
    },
    /// Two independently projected legs and native quoted spread selection.
    BasisSwap {
        /// Pay floating leg.
        pay: FloatingHelperLeg,
        /// Receive floating leg.
        receive: FloatingHelperLeg,
        /// Leg to which quote applies.
        quoted_leg: BasisQuoteLeg,
        /// Exact discount curve.
        discount_curve: CurveKey,
    },
}

/// Pillar date selection never rewrites maturity, settlement or payment dates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum HelperPillar {
    /// Contractual economic end.
    Maturity,
    /// Latest relevant payment/protection date.
    LastRelevantDate,
    /// Explicit date in [maturity,last relevant date].
    Explicit(Date),
}

/// Shared native helper DTO, with all economics and numerical quote semantics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CurveHelperInput {
    /// Unique helper/observation identity.
    pub id: String,
    /// Currency of the contract/quote.
    pub currency: Currency,
    /// Full dated terms.
    pub terms: CurveHelperTerms,
    /// Market quote in its original unit.
    pub quote: MarketQuote,
    /// Pillar selection distinct from maturity.
    pub pillar: HelperPillar,
    /// Absolute native quote tolerance.
    pub tolerance: f64,
    /// Nonnegative objective weight (positive for mandatory fitted input).
    pub weight: f64,
    /// Positive native-unit residual divisor for a scaled global objective.
    pub residual_scale: f64,
}

/// Pure product recipe; helper identities must exactly match recipe.source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct HelperCurveRecipe {
    /// Full curve parameters, versions, methods and dependencies.
    pub recipe: CurveRecipe,
    /// All original instrument terms/market quotes (no tenor-only adapters).
    pub helpers: Vec<CurveHelperInput>,
}

/// Validated description, with actual generated dates and honest dispatch status.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct HelperDescription {
    /// Original identity.
    pub helper_id: String,
    /// Selected actual pillar.
    pub pillar: Date,
    /// Actual last relevant date.
    pub last_relevant_date: Date,
    /// Checked native construction status; does not certify product pricing.
    pub calibration_status: CurveCapabilityStatus,
}

/// Validate a helper with actual calendars/histories. This inspects dates and
/// requirements, not a model price; unsupported calibration remains explicit.
pub fn describe_helper(
    input: &CurveHelperInput,
    reference_date: Date,
    resources: &HelperResources<'_>,
) -> Result<HelperDescription> {
    resources.validate()?;
    date(reference_date)?;
    label(&input.id, "helper.id")?;
    input.quote.validate()?;
    for (field, value) in [
        ("tolerance", input.tolerance),
        ("weight", input.weight),
        ("residual_scale", input.residual_scale),
    ] {
        if !value.is_finite() || value <= 0.0 {
            return Err(invalid(
                field,
                "must be positive and finite; mandatory helpers cannot be dropped with zero weight",
            ));
        }
    }
    let (maturity, relevant, supported) =
        match &input.terms {
            CurveHelperTerms::MoneyMarket(terms) => {
                let helper = MoneyMarketHelper {
                    id: input.id.clone(),
                    terms: terms.clone(),
                    quote: input.quote,
                    tolerance: input.tolerance,
                };
                helper.validate(reference_date)?;
                let (_, end, _) = terms.period();
                (end, end, true)
            }
            CurveHelperTerms::RateFuture(terms) => {
                validate_future(input.quote, terms, reference_date, resources)?;
                (terms.accrual_end, terms.final_settlement_date, false)
            }
            CurveHelperTerms::FixedCouponBond(terms) => {
                let last = validate_bond(input.quote, terms, resources)?;
                (terms.coupon_schedule.schedule.end_date, last, false)
            }
            CurveHelperTerms::Cds(terms) => {
                let last = validate_cds(input.quote, terms, resources)?;
                (terms.protection_end, last, false)
            }
            CurveHelperTerms::Swap {
                fixed,
                floating,
                discount_curve,
            } => {
                input.quote.decimal_rate()?;
                discount_curve.validate()?;
                let fixed_schedule = fixed.schedule.generate(resources)?;
                validate_profiles(&fixed.schedule, &fixed.coupons, &fixed.notionals)?;
                let float_last = validate_floating(floating, reference_date, resources)?;
                let maturity = fixed
                    .schedule
                    .schedule
                    .end_date
                    .max(floating.schedule.schedule.end_date);
                (
                    maturity,
                    last_payment(&fixed_schedule)?.max(float_last),
                    false,
                )
            }
            CurveHelperTerms::BasisSwap {
                pay,
                receive,
                discount_curve,
                ..
            } => {
                input.quote.decimal_rate()?;
                discount_curve.validate()?;
                let last = validate_floating(pay, reference_date, resources)?
                    .max(validate_floating(receive, reference_date, resources)?);
                (
                    pay.schedule
                        .schedule
                        .end_date
                        .max(receive.schedule.schedule.end_date),
                    last,
                    false,
                )
            }
        };
    let last_relevant_date = relevant.max(maturity);
    let pillar = match input.pillar {
        HelperPillar::Maturity => maturity,
        HelperPillar::LastRelevantDate => last_relevant_date,
        HelperPillar::Explicit(pillar) => {
            date(pillar)?;
            if pillar < maturity || pillar > last_relevant_date {
                return Err(invalid(
                    "pillar",
                    "must be within [maturity,last relevant date] for this contract",
                ));
            }
            pillar
        }
    };
    if pillar <= reference_date {
        return Err(invalid(
            "pillar",
            "expired helper; pillar must follow curve reference date",
        ));
    }
    // Enforce model/date bounds even for a representable unsupported product.
    convex_curves::contracts::year_fraction(
        reference_date,
        pillar,
        DayCountConvention::Act365Fixed,
    )?;
    Ok(HelperDescription {
        helper_id: input.id.clone(),
        pillar,
        last_relevant_date,
        calibration_status: if supported {
            CurveCapabilityStatus::Supported
        } else {
            CurveCapabilityStatus::DescribedUnsupported
        },
    })
}

/// Pure composition of full helper DTOs into supported curve construction.
/// All terms are validated first. No complex helper is dropped or reduced to a
/// maturity/rate proxy. Inputs and supplied immutable curves are never mutated.
pub fn build_helper_curve_set(
    requests: &[HelperCurveRecipe],
    resources: &HelperResources<'_>,
    supplied: &CurveSet,
) -> Result<CurveBuildOutput> {
    resources.validate()?;
    if requests.is_empty() || requests.len() + supplied.len() > MAX_CURVES {
        return Err(Error::BoundExceeded("helper curve set"));
    }
    let mut recipes = Vec::with_capacity(requests.len());
    for request in requests {
        let CurveSource::Helpers(ids) = &request.recipe.source else {
            return Err(invalid(
                "source",
                "helper recipes require the Helpers source",
            ));
        };
        if request.helpers.is_empty()
            || request.helpers.len() >= MAX_CURVE_INPUTS
            || ids.len() != request.helpers.len()
        {
            return Err(invalid(
                "helpers",
                "nonempty bounded helper list and exact source ID dimensions required",
            ));
        }
        let expected: BTreeSet<_> = ids.iter().collect();
        if expected.len() != ids.len() {
            return Err(invalid("source", "duplicate helper identity"));
        }
        let mut seen = BTreeSet::new();
        let mut helpers = Vec::with_capacity(request.helpers.len());
        let mut unsupported = Vec::new();
        for helper in &request.helpers {
            if !expected.contains(&helper.id) || !seen.insert(&helper.id) {
                return Err(invalid(
                    "helper.id",
                    "must exactly match unique source identities",
                ));
            }
            if helper.currency != request.recipe.currency {
                return Err(invalid("helper.currency", "must match recipe currency"));
            }
            let description = describe_helper(helper, request.recipe.reference_date, resources)?;
            if let CurveHelperTerms::MoneyMarket(terms) = &helper.terms {
                if description.pillar != terms.period().1 {
                    return Err(invalid(
                        "pillar",
                        "simple-rate construction uses its actual end date",
                    ));
                }
                helpers.push(MoneyMarketHelper {
                    id: helper.id.clone(),
                    terms: terms.clone(),
                    quote: helper.quote,
                    tolerance: helper.tolerance,
                });
            } else {
                unsupported.push(helper.id.clone());
            }
        }
        if !unsupported.is_empty() {
            return Err(Error::Unsupported { capability: "product-backed helper calibration".into(), reason: format!("full terms retained for {}; CVX-021/057/058/059 require actual product pricers and model/native quote repricing", unsupported.join(", ")) });
        }
        if request.recipe.calibration != CalibrationMode::Piecewise {
            return Err(Error::Unsupported { capability: "helper calibration mode".into(), reason: "checked money-market composition currently supports exact Piecewise chains only".into() });
        }
        let mut recipe = request.recipe.clone();
        recipe.source = CurveSource::MoneyMarket(helpers);
        recipes.push(recipe);
    }
    build_curve_set(&recipes, supplied)
}

fn validate_bond(
    quote: MarketQuote,
    terms: &FixedCouponBondHelper,
    resources: &HelperResources<'_>,
) -> Result<Date> {
    label(&terms.instrument_id, "instrument_id")?;
    label(&terms.instrument_version, "instrument_version")?;
    date(terms.issue_date)?;
    date(terms.settlement_date)?;
    if terms.issue_date > terms.settlement_date
        || terms.settlement_date >= terms.coupon_schedule.schedule.end_date
    {
        return Err(invalid(
            "bond.settlement",
            "must follow issue and precede maturity",
        ));
    }
    if terms.quoted_face <= Decimal::ZERO
        || terms.quoted_face > Decimal::from(1_000_000_000_000_000_000_i64)
        || terms.redemption_fraction < Decimal::ZERO
        || terms.redemption_fraction > Decimal::from(10)
    {
        return Err(invalid(
            "bond.principal",
            "positive face <=1e18 and redemption fraction in [0,10] required",
        ));
    }
    let schedule = terms.coupon_schedule.generate(resources)?;
    let profiles = validate_profiles(&terms.coupon_schedule, &terms.coupons, &terms.notionals)?;
    terms.cash_rounding.apply(Decimal::ZERO)?;
    let calculation = convex_core::cashflows::CouponCalculation {
        index_rate: Decimal::ZERO,
        day_count: terms.coupon_schedule.day_count,
        principal: terms.coupon_principal,
        rounding: terms.cash_rounding,
        negative_coupons: terms.negative_coupons,
    };
    // Check actual coupon arithmetic/negative policy, without claiming price/yield.
    for period in schedule.periods() {
        profiles.coupon_amount(period, calculation)?;
    }
    match terms.quote_convention {
        BondHelperQuote::CleanPrice | BondHelperQuote::DirtyPrice
            if quote.unit != QuoteUnit::PricePer100 =>
        {
            return Err(invalid("bond.quote", "price per 100 required"))
        }
        BondHelperQuote::Yield { .. } => {
            quote.decimal_rate()?;
        }
        _ => (),
    }
    last_payment(&schedule)
}

fn validate_future(
    quote: MarketQuote,
    terms: &RateFutureHelper,
    reference_date: Date,
    resources: &HelperResources<'_>,
) -> Result<()> {
    label(&terms.contract_id, "contract_id")?;
    label(&terms.contract_version, "contract_version")?;
    if quote.unit != QuoteUnit::FuturePrice100MinusRate {
        return Err(invalid(
            "future.quote",
            "100-minus-rate price points required",
        ));
    }
    convex_curves::contracts::year_fraction(
        terms.accrual_start,
        terms.accrual_end,
        terms.day_count,
    )?;
    date(terms.last_trading_date)?;
    date(terms.final_settlement_date)?;
    if terms.last_trading_date > terms.final_settlement_date
        || terms.final_settlement_date < terms.accrual_end
    {
        return Err(invalid(
            "future.dates",
            "final settlement must be no earlier than accrual end/last trade",
        ));
    }
    let calendar = resources.calendar(&terms.observation_calendar)?;
    let settlement_calendar = resources.calendar(&terms.settlement_calendar)?;
    if !settlement_calendar.is_business_day(terms.final_settlement_date) {
        return Err(invalid(
            "future.settlement",
            "must be a supplied-calendar business day",
        ));
    }
    for value in [terms.tick_size, terms.price_multiplier, terms.tick_value] {
        if value <= Decimal::ZERO || value > Decimal::from(1_000_000_000_000_000_000_i64) {
            return Err(invalid(
                "future.tick",
                "positive contract amounts <=1e18 required",
            ));
        }
    }
    if terms.tick_size.checked_mul(terms.price_multiplier) != Some(terms.tick_value) {
        return Err(invalid(
            "future.tick_value",
            "must equal tick size times price multiplier exactly",
        ));
    }
    match &terms.convexity {
        FutureConvexity::Supplied {
            adjustment,
            model_version,
        } => {
            adjustment.decimal_rate()?;
            label(model_version, "convexity.model_version")?;
        }
        FutureConvexity::Model {
            model_id,
            model_version,
        } => {
            label(model_id, "convexity.model_id")?;
            label(model_version, "convexity.model_version")?;
        }
    }
    let expected: BTreeSet<_> = match &terms.accrual {
        FutureAccrual::Term {
            index_id,
            fixing_date,
        } => {
            date(*fixing_date)?;
            if *fixing_date > terms.accrual_start {
                return Err(invalid("future.fixing_date", "follows term accrual start"));
            }
            BTreeSet::from([(index_id.clone(), *fixing_date)])
        }
        FutureAccrual::OvernightAverage {
            index_id,
            convention,
        }
        | FutureAccrual::OvernightCompounded {
            index_id,
            convention,
        } => {
            let schedule = OvernightSchedule::generate(
                terms.accrual_start,
                terms.accrual_end,
                terms.day_count,
                calendar,
                *convention,
            )?;
            schedule
                .observations()
                .iter()
                .map(|observation| (index_id.clone(), observation.observation_date))
                .collect()
        }
    };
    validate_fixings(
        &terms.fixing_requirements,
        terms.fixing_context,
        &expected,
        reference_date,
        resources,
    )
}

fn validate_cds(
    quote: MarketQuote,
    terms: &CdsHelper,
    resources: &HelperResources<'_>,
) -> Result<Date> {
    terms.reference_entity.validate()?;
    terms.documentation.validate()?;
    label(&terms.seniority, "seniority")?;
    for d in [
        terms.trade_date,
        terms.step_in_date,
        terms.cash_settlement_date,
        terms.protection_start,
        terms.protection_end,
    ] {
        date(d)?;
    }
    if terms.trade_date > terms.step_in_date
        || terms.cash_settlement_date < terms.trade_date
        || terms.step_in_date >= terms.protection_end
        || terms.protection_start >= terms.protection_end
        || terms.premium_schedule.schedule.end_date != terms.protection_end
    {
        return Err(invalid(
            "cds.dates",
            "inconsistent trade/step-in/cash/protection/premium dates",
        ));
    }
    let premium = terms.premium_schedule.generate(resources)?;
    let notionals = DatedProfile::new(ProfileKind::Notional, terms.notionals.clone())?;
    let expected_start = terms
        .premium_schedule
        .schedule
        .start_date
        .min(terms.protection_start);
    if notionals.coverage() != (expected_start, terms.protection_end) {
        return Err(invalid(
            "cds.notional",
            "must cover premium and protection periods exactly",
        ));
    }
    if terms.running_coupon.decimal_rate()? < 0.0
        || !terms.recovery.is_finite()
        || !(0.0..=1.0).contains(&terms.recovery)
    {
        return Err(invalid(
            "cds.coupon/recovery",
            "nonnegative running coupon and finite recovery in [0,1] required",
        ));
    }
    match terms.quote_convention {
        CdsHelperQuote::ParSpread if quote.decimal_rate()? < 0.0 => {
            return Err(invalid("cds.spread", "must be nonnegative"))
        }
        CdsHelperQuote::CleanUpfront | CdsHelperQuote::DirtyUpfront
            if quote.unit != QuoteUnit::UpfrontFraction =>
        {
            return Err(invalid("cds.upfront", "signed upfront fraction required"))
        }
        _ => (),
    }
    terms.discount_curve.validate()?;
    let last = match terms.protection_payment {
        ProtectionPayment::AtDefault { calendar_days } => {
            if calendar_days > 366 {
                return Err(Error::BoundExceeded("protection settlement delay"));
            }
            terms
                .protection_end
                .checked_add_days(i64::from(calendar_days))
                .map_err(|e| invalid("protection_payment", &e.to_string()))?
        }
        ProtectionPayment::FixedDate(d) => {
            date(d)?;
            if d < terms.protection_end {
                return Err(invalid(
                    "protection_payment",
                    "fixed cash date precedes protection end",
                ));
            }
            d
        }
    };
    date(last)?;
    Ok(last.max(last_payment(&premium)?))
}

fn validate_profiles(
    schedule: &HelperSchedule,
    coupons: &[DatedValue],
    notionals: &[DatedValue],
) -> Result<CouponProfiles> {
    let coupon = DatedProfile::new(ProfileKind::Coupon, coupons.to_vec())?;
    let notional = DatedProfile::new(ProfileKind::Notional, notionals.to_vec())?;
    let (start, end) = (schedule.schedule.start_date, schedule.schedule.end_date);
    if coupon.coverage() != (start, end) || notional.coverage() != (start, end) {
        return Err(invalid(
            "profiles",
            "must cover the full contractual schedule exactly",
        ));
    }
    let spread = DatedProfile::new(
        ProfileKind::Spread,
        vec![DatedValue {
            start,
            end,
            value: Decimal::ZERO,
        }],
    )?;
    let gearing = DatedProfile::new(
        ProfileKind::Gearing,
        vec![DatedValue {
            start,
            end,
            value: Decimal::ZERO,
        }],
    )?;
    Ok(CouponProfiles::new(coupon, spread, gearing, notional)?)
}

fn validate_floating(
    terms: &FloatingHelperLeg,
    reference_date: Date,
    resources: &HelperResources<'_>,
) -> Result<Date> {
    terms.projection_curve.validate()?;
    let schedule = terms.schedule.generate(resources)?;
    let notional = DatedProfile::new(ProfileKind::Notional, terms.notionals.clone())?;
    let spread = DatedProfile::new(ProfileKind::Spread, terms.spreads.clone())?;
    let gearing = DatedProfile::new(ProfileKind::Gearing, terms.gearings.clone())?;
    let (start, end) = (
        terms.schedule.schedule.start_date,
        terms.schedule.schedule.end_date,
    );
    for profile in [&notional, &spread, &gearing] {
        if profile.coverage() != (start, end) {
            return Err(invalid(
                "floating profiles",
                "must cover the full contract exactly",
            ));
        }
    }
    let expected = match &terms.index {
        FloatingHelperIndex::Term {
            index_id,
            tenor_months,
        } => {
            if *tenor_months == 0 || *tenor_months > 120 {
                return Err(invalid("index.tenor_months", "must be in 1..120"));
            }
            let mut expected = BTreeSet::new();
            for period in schedule.periods() {
                let fixing = period.fixing_date().ok_or_else(|| {
                    invalid(
                        "fixing_offset",
                        "term floating legs need explicit fixing dates",
                    )
                })?;
                expected.insert((index_id.clone(), fixing));
            }
            expected
        }
        FloatingHelperIndex::Overnight {
            index_id,
            convention,
        } => {
            let calendar = resources.calendar(&terms.schedule.calendar)?;
            let mut expected = BTreeSet::new();
            for period in schedule.periods() {
                let (start, end) = period.accrual_dates();
                let observations = OvernightSchedule::generate(
                    start,
                    end,
                    terms.schedule.day_count,
                    calendar,
                    *convention,
                )?;
                expected.extend(
                    observations
                        .observations()
                        .iter()
                        .map(|o| (index_id.clone(), o.observation_date)),
                );
                if expected.len() > convex_core::fixings::MAX_FIXING_REQUIREMENTS {
                    return Err(Error::BoundExceeded("floating fixing requirements"));
                }
            }
            expected
        }
    };
    validate_fixings(
        &terms.fixing_requirements,
        terms.fixing_context,
        &expected,
        reference_date,
        resources,
    )?;
    last_payment(&schedule)
}

fn validate_fixings(
    requirements: &[FixingRequirement],
    context: FixingContext,
    expected: &BTreeSet<(IndexId, Date)>,
    reference_date: Date,
    resources: &HelperResources<'_>,
) -> Result<()> {
    if context.valuation_date != reference_date {
        return Err(invalid(
            "fixing_context",
            "valuation date must match curve reference date",
        ));
    }
    if requirements.len() > convex_core::fixings::MAX_FIXING_REQUIREMENTS {
        return Err(Error::BoundExceeded("fixing requirements"));
    }
    let supplied: BTreeSet<_> = requirements
        .iter()
        .map(|r| (r.index_id.clone(), r.observation_date))
        .collect();
    if supplied != *expected || supplied.len() != requirements.len() {
        return Err(invalid(
            "fixing_requirements",
            "must exactly match unique generated observation dates/indices",
        ));
    }
    let resolution = resources.fixings.resolve(requirements, context)?;
    if !resolution.missing().is_empty() {
        return Err(convex_core::cashflows::CashflowError::MissingFixings(
            resolution.missing().to_vec(),
        )
        .into());
    }
    Ok(())
}

fn last_payment(schedule: &CouponSchedule) -> Result<Date> {
    schedule
        .periods()
        .last()
        .map(convex_core::cashflows::CouponPeriod::payment_date)
        .ok_or_else(|| invalid("schedule", "empty generated schedule"))
}
fn date(value: Date) -> Result<()> {
    BusinessDate::try_from(value).map_err(|e| invalid("date", &e.to_string()))?;
    Ok(())
}
fn label(value: &str, field: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(invalid(
            field,
            "nonempty trimmed label <=128 bytes without control characters required",
        ));
    }
    Ok(())
}
fn invalid(field: &str, reason: &str) -> Error {
    Error::InvalidInput {
        field: field.into(),
        reason: reason.into(),
    }
}

// Only the new economic DTOs use exact strings. Existing core/profile and host
// serializations remain separate compatibility contracts.
mod exact_decimal {
    use rust_decimal::Decimal;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(
        value: &Decimal,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.to_string())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Decimal, D::Error> {
        let text = String::deserialize(deserializer)?;
        if text.len() > 64 {
            return Err(serde::de::Error::custom(
                "contractual decimal string exceeds64bytes",
            ));
        }
        Decimal::from_str_exact(&text).map_err(serde::de::Error::custom)
    }
}

mod exact_profiles {
    use convex_core::{
        cashflows::{DatedValue, MAX_SCHEDULE_PERIODS},
        Date,
    };
    use rust_decimal::Decimal;
    use serde::{ser::SerializeSeq, Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct WireValue {
        start: Date,
        end: Date,
        #[serde(with = "super::exact_decimal")]
        value: Decimal,
    }
    pub fn serialize<S: Serializer>(
        values: &[DatedValue],
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        if values.len() > MAX_SCHEDULE_PERIODS {
            return Err(serde::ser::Error::custom(
                "too many contractual profile entries",
            ));
        }
        let mut sequence = serializer.serialize_seq(Some(values.len()))?;
        for value in values {
            sequence.serialize_element(&WireValue {
                start: value.start,
                end: value.end,
                value: value.value,
            })?;
        }
        sequence.end()
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Vec<DatedValue>, D::Error> {
        let values = Vec::<WireValue>::deserialize(deserializer)?;
        if values.len() > MAX_SCHEDULE_PERIODS {
            return Err(serde::de::Error::custom(
                "too many contractual profile entries",
            ));
        }
        Ok(values
            .into_iter()
            .map(|value| DatedValue {
                start: value.start,
                end: value.end,
                value: value.value,
            })
            .collect())
    }
}
