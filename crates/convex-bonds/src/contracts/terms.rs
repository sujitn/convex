//! Lossless supplied economic terms; checked objects are constructed separately.

use convex_core::{
    calendars::{BusinessDayConvention, Calendar},
    cashflows::{
        CashRounding, CashflowError, CouponPrincipal, CouponProfiles, DateOffset, DateRules,
        DatedProfile, DatedValue, ExCouponAccrual, NegativeCouponPolicy, OvernightConvention,
        ProfileKind, ScheduleSpec,
    },
    contracts::IndexId,
    daycounts::DayCountConvention,
    Currency, Date,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::{BondContract, BondContractError, ExerciseTerms};
use crate::types::CalendarId;

/// Independently dated absolute principal and annualized decimal components.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BondProfiles {
    /// Deterministic coupon component, not an average over future coupons.
    pub coupon: Vec<DatedValue>,
    /// Additive simple spread, outside overnight compounding.
    pub spread: Vec<DatedValue>,
    /// Multiplier applied to the term rate or compounded overnight interest.
    pub gearing: Vec<DatedValue>,
    /// Outstanding principal in absolute contract cash units.
    pub notional: Vec<DatedValue>,
}

impl BondProfiles {
    /// Explicit constant profiles over a supplied accrual interval.
    pub fn constant(
        start: Date,
        end: Date,
        coupon: Decimal,
        spread: Decimal,
        gearing: Decimal,
        notional: Decimal,
    ) -> Self {
        let entry = |value| vec![DatedValue { start, end, value }];
        Self {
            coupon: entry(coupon),
            spread: entry(spread),
            gearing: entry(gearing),
            notional: entry(notional),
        }
    }

    pub(super) fn checked(&self) -> Result<CouponProfiles, CashflowError> {
        CouponProfiles::new(
            DatedProfile::new(ProfileKind::Coupon, self.coupon.clone())?,
            DatedProfile::new(ProfileKind::Spread, self.spread.clone())?,
            DatedProfile::new(ProfileKind::Gearing, self.gearing.clone())?,
            DatedProfile::new(ProfileKind::Notional, self.notional.clone())?,
        )
    }
}

/// Independent term-rate reset intervals. Each reset carries one fixing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TermRateTerms {
    /// Provider-neutral supplied index identity; does not select a curve.
    pub index_id: IndexId,
    /// Reset grid independent of coupon/payment frequency.
    pub resets: ScheduleSpec,
    /// Adjustment of reset boundaries, independently from coupon accrual.
    pub reset_adjustment: BusinessDayConvention,
    /// Signed offset from the actual reset start to observation date.
    pub fixing_offset: DateOffset,
    /// Annualized forward quotation for the supplied projection policy.
    pub projection_day_count: DayCountConvention,
}

/// Plain contracted overnight coupon: compound the index, apply gearing once,
/// then add simple deterministic coupon/spread accrual. Intracoupon principal
/// or gearing changes require a different explicit compounding contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct OvernightRateTerms {
    /// Exact supplied index, independent of vendor identity.
    pub index_id: IndexId,
    /// Actual lookback/shift/lockout, not inferred from the index name.
    pub convention: OvernightConvention,
}

/// Closed initial set of coupon mechanics; no payoff language or inference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BondCoupon {
    /// Deterministic coupon and spread; gearing must be zero.
    Fixed,
    /// Independently reset term-rate coupons.
    Term(TermRateTerms),
    /// Daily compounded index with simple spread outside the compound product.
    Overnight(OvernightRateTerms),
}

// Serde's internally tagged unit variant can ignore extra fields even with
// deny_unknown_fields. Keep the public unit variant and its existing JSON bytes,
// but decode Fixed as an empty struct so cap/floor/payoff fields cannot vanish.
impl<'de> Deserialize<'de> for BondCoupon {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
        enum CheckedCoupon {
            Fixed {},
            Term(TermRateTerms),
            Overnight(OvernightRateTerms),
        }
        Ok(match CheckedCoupon::deserialize(deserializer)? {
            CheckedCoupon::Fixed {} => Self::Fixed,
            CheckedCoupon::Term(terms) => Self::Term(terms),
            CheckedCoupon::Overnight(terms) => Self::Overnight(terms),
        })
    }
}

/// Calendar identities retained in JSON. Actual pure calendars are supplied
/// at construction. `build` resolves only checked compiled calendar names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BondCalendarIds {
    /// Coupon accrual, payment, principal and settlement calendar.
    pub payment: CalendarId,
    /// Term reset and fixing calendar.
    pub reset: CalendarId,
    /// Overnight observation calendar.
    pub observation: CalendarId,
    /// Notice/exercise settlement calendar.
    pub exercise: CalendarId,
}

impl BondCalendarIds {
    /// Explicitly select the same calendar for all four roles.
    pub fn single(calendar: CalendarId) -> Self {
        Self {
            payment: calendar.clone(),
            reset: calendar.clone(),
            observation: calendar.clone(),
            exercise: calendar,
        }
    }
}

/// Resolved calendar values for native embedding. The caller binds each value
/// to the identity/version in its immutable input set; no data lookup occurs.
pub struct BondCalendars<'a> {
    /// Payment/accrual/principal calendar.
    pub payment: &'a dyn Calendar,
    /// Reset/fixing calendar.
    pub reset: &'a dyn Calendar,
    /// Observation calendar.
    pub observation: &'a dyn Calendar,
    /// Exercise/notice calendar.
    pub exercise: &'a dyn Calendar,
}

/// Non-par price for one internal principal reduction. Unlisted reductions
/// pay at par. Effective notional and payment dates remain separate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct PrincipalPrice {
    /// Exact effective internal reduction date.
    pub effective_date: Date,
    /// Redemption multiplier (1 = par), not a per-100 price.
    #[serde(with = "convex_core::contracts::decimal_serde")]
    #[cfg_attr(
        feature = "schemars",
        schemars(schema_with = "convex_core::contracts::decimal_serde::schema")
    )]
    pub multiplier: Decimal,
    /// Optional absolute part retired at this price. The remaining part keeps
    /// its par/final residual price as a separate cashflow, never an average.
    #[serde(default, with = "convex_core::contracts::decimal_serde::optional")]
    #[cfg_attr(
        feature = "schemars",
        schemars(schema_with = "convex_core::contracts::decimal_serde::optional::schema")
    )]
    pub amount: Option<Decimal>,
}

/// Full initial bond-family economic input schema. There are no silent defaults
/// for dates, floating mechanics, exercise, principal, rounding or entitlement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BondTerms {
    /// Exact supported economic schema version (1).
    pub schema_version: u32,
    /// Contract currency; numerical cash is absolute in this currency.
    pub currency: Currency,
    /// Issue date, distinct from dated/accrual start.
    pub issue_date: Date,
    /// Original face, used for per-original-face quote normalization.
    #[serde(with = "convex_core::contracts::decimal_serde")]
    #[cfg_attr(
        feature = "schemars",
        schemars(schema_with = "convex_core::contracts::decimal_serde::schema")
    )]
    pub face_value: Decimal,
    /// Complete coupon accrual grid with retained irregular references.
    pub schedule: ScheduleSpec,
    /// Independent accrual, payment and ex-coupon transformations.
    pub dates: DateRules,
    /// Calendar identities for reconstruction with supplied resources.
    pub calendars: BondCalendarIds,
    /// Nonnegative settlement lag; no market cycle inferred.
    pub settlement_lag: DateOffset,
    /// Explicit settlement adjustment.
    pub settlement_adjustment: BusinessDayConvention,
    /// Coupon accrual day count, including retained ICMA references.
    pub day_count: DayCountConvention,
    /// Four independent dated profiles.
    pub profiles: BondProfiles,
    /// Coupon/reset/observation mechanics.
    pub coupon: BondCoupon,
    /// Principal choice at coupon boundaries.
    pub coupon_principal: CouponPrincipal,
    /// Final residual principal multiplier, distinct from internal repayments.
    #[serde(with = "convex_core::contracts::decimal_serde")]
    #[cfg_attr(
        feature = "schemars",
        schemars(schema_with = "convex_core::contracts::decimal_serde::schema")
    )]
    pub redemption_multiplier: Decimal,
    /// Explicit non-par internal repayments.
    pub principal_prices: Vec<PrincipalPrice>,
    /// Contract cash rounding, once per coupon/payment/accrued result.
    pub rounding: CashRounding,
    /// Explicit negative effective coupon behavior, without invented floors.
    pub negative_coupons: NegativeCouponPolicy,
    /// Ex-coupon accrued behavior separate from principal entitlement.
    pub ex_coupon_accrual: ExCouponAccrual,
    /// Call and put terms, including notice and settlement, if contracted.
    pub exercise: Vec<ExerciseTerms>,
}

impl BondTerms {
    /// Construct with checked compiled calendar identifiers. Unknown calendars
    /// fail; custom/versioned calendars use `build_with_calendars`.
    pub fn build(self) -> Result<BondContract, BondContractError> {
        let payment = self.calendars.payment.try_to_calendar()?;
        let reset = self.calendars.reset.try_to_calendar()?;
        let observation = self.calendars.observation.try_to_calendar()?;
        let exercise = self.calendars.exercise.try_to_calendar()?;
        self.build_with_calendars(&BondCalendars {
            payment: payment.as_ref(),
            reset: reset.as_ref(),
            observation: observation.as_ref(),
            exercise: exercise.as_ref(),
        })
    }

    /// Construct from actual resolved calendars; IDs/versions belong to the
    /// caller's immutable information set. All economic validation still runs.
    pub fn build_with_calendars(
        self,
        calendars: &BondCalendars<'_>,
    ) -> Result<BondContract, BondContractError> {
        BondContract::new(self, calendars)
    }
}
