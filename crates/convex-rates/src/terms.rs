//! Complete strict native economic inputs, reconstructed through checked build.

use convex_core::{
    calendars::{BusinessDayConvention, Calendar},
    cashflows::{
        CashRounding, DateOffset, DateRules, DatedValue, NegativeCouponPolicy, OvernightConvention,
        ScheduleSpec,
    },
    contracts::{IndexId, ReferenceDataId, TradeId, Version},
    daycounts::DayCountConvention,
    fixings::UnpublishedFixingPolicy,
    Currency, Date,
};
use convex_curves::contracts::CurveKey;
use serde::{Deserialize, Serialize};

use crate::{SwapContract, SwapError};

/// Exact immutable reference resource, independent of a provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ReferencePin {
    /// Internal calendar or agreement identity.
    pub id: ReferenceDataId,
    /// Exact immutable resource version.
    pub version: Version,
}

/// Actual supplied calendar bound to the contract's exact pin by the caller.
#[derive(Clone, Copy)]
pub struct CalendarBinding<'a> {
    /// Exact identity/version; checked against terms at construction.
    pub pin: &'a ReferencePin,
    /// Calendar implementing that resource. No lookup or clock is used.
    pub calendar: &'a dyn Calendar,
}

/// Distinct leg accrual and payment calendar identities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct LegCalendarPins {
    /// Calendar used only to adjust coupon accrual boundaries.
    pub accrual: ReferencePin,
    /// Calendar used for payment lag and adjustment.
    pub payment: ReferencePin,
}

/// Actual immutable coupon calendars for one leg.
#[derive(Clone, Copy)]
pub struct LegCalendars<'a> {
    /// Coupon accrual calendar.
    pub accrual: CalendarBinding<'a>,
    /// Cash payment calendar.
    pub payment: CalendarBinding<'a>,
}

/// Actual calendar resources. Term legs require reset/fixing; overnight legs
/// require observation. Irrelevant resources are rejected rather than ignored.
pub struct SwapCalendars<'a> {
    /// Fixed leg calendars, independent of floating.
    pub fixed: LegCalendars<'a>,
    /// Floating coupon calendars.
    pub floating: LegCalendars<'a>,
    /// Term reset/value-date calendar.
    pub reset: Option<CalendarBinding<'a>>,
    /// Term observation-date calendar.
    pub fixing: Option<CalendarBinding<'a>>,
    /// Overnight observation and day-weight calendar.
    pub observation: Option<CalendarBinding<'a>>,
}

/// Direction from this trade's perspective; notionals remain unsigned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum SwapSide {
    /// Pay fixed and receive floating.
    PayFixed,
    /// Receive fixed and pay floating.
    ReceiveFixed,
}
impl SwapSide {
    pub(crate) fn fixed_sign(self) -> rust_decimal::Decimal {
        match self {
            Self::PayFixed => -rust_decimal::Decimal::ONE,
            Self::ReceiveFixed => rust_decimal::Decimal::ONE,
        }
    }
}

/// Shared coupon terms, without principal exchanges or bond entitlement rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct LegTerms {
    /// Complete independent coupon grid, stubs and irregular reference periods.
    pub schedule: ScheduleSpec,
    /// Independent accrual/payment rules. Fixing/observation/ex fields must be absent.
    pub dates: DateRules,
    /// Exact independent accrual and payment calendar versions.
    pub calendars: LegCalendarPins,
    /// Contractual coupon day count, including ICMA references for term/fixed legs.
    pub day_count: DayCountConvention,
    /// Absolute outstanding notional on actual adjusted accrual dates; no exchange.
    #[serde(with = "exact_profiles")]
    #[cfg_attr(feature = "schemars", schemars(with = "Vec<DatedValue>"))]
    pub notionals: Vec<DatedValue>,
    /// Applied once after all coupon/reset/spread slices have been summed.
    pub rounding: CashRounding,
    /// Explicit handling of negative effective coupons; no floor is inferred.
    pub negative_coupons: NegativeCouponPolicy,
}

/// Deterministic fixed leg with an independently dated rate profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FixedLegTerms {
    /// Complete leg rules.
    pub leg: LegTerms,
    /// Annualized decimal rate (0.05 = 5%); may step independently of principal.
    #[serde(with = "exact_profiles")]
    #[cfg_attr(feature = "schemars", schemars(with = "Vec<DatedValue>"))]
    pub rates: Vec<DatedValue>,
}

/// Independently dated floating spread and gearing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FloatingProfiles {
    /// Simple annualized decimal spread, outside overnight compounding.
    #[serde(with = "exact_profiles")]
    #[cfg_attr(feature = "schemars", schemars(with = "Vec<DatedValue>"))]
    pub spreads: Vec<DatedValue>,
    /// Dimensionless multiplier of the resolved index or compounded interest.
    #[serde(with = "exact_profiles")]
    #[cfg_attr(feature = "schemars", schemars(with = "Vec<DatedValue>"))]
    pub gearings: Vec<DatedValue>,
}

/// Contractual eligibility policy; a calculation context cannot weaken it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FixingPolicy {
    /// Treatment of a past observation not yet due for publication.
    pub unpublished: UnpublishedFixingPolicy,
    /// Whether preliminary published revisions are ineligible.
    pub final_only: bool,
}

/// Explicit quotation period for each term observation, including a stub.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum TermRatePeriod {
    /// Rate over the actual adjusted reset interval. This must be a contracted
    /// dated rate; no tenor interpolation is performed or claimed.
    ResetInterval,
    /// Specified full index tenor from each adjusted reset start, even for a stub.
    /// Calendar-month EOM and end adjustment are explicitly supplied below.
    IndexTenor,
}

/// Independent reset and term-rate definition. Multiple resets in a coupon
/// sum simple accrued interest on their actual slices; they are not compounded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TermResetTerms {
    /// Provider-neutral index.
    pub index_id: IndexId,
    /// Contractual index tenor, in 1..120 calendar months.
    pub tenor_months: u32,
    /// Independent reset frequency, boundaries and stub rules.
    pub schedule: ScheduleSpec,
    /// Reset/value-date calendar version.
    pub reset_calendar: ReferencePin,
    /// Reset accrual/value-date adjustment.
    pub reset_adjustment: BusinessDayConvention,
    /// Observation-date calendar version, independently from reset dates.
    pub fixing_calendar: ReferencePin,
    /// Nonpositive signed offset from reset start; in-arrears resets are unsupported.
    pub fixing_offset: DateOffset,
    /// Adjustment after the fixing offset, on the fixing calendar.
    pub fixing_adjustment: BusinessDayConvention,
    /// Explicit rate interval contract, without interpolated stub assumptions.
    pub rate_period: TermRatePeriod,
    /// Index-tenor EOM flag; must be false for ResetInterval.
    pub index_end_of_month: bool,
    /// Index-tenor end adjustment; must be Unadjusted for ResetInterval.
    pub index_end_adjustment: BusinessDayConvention,
    /// Simple-rate quotation day count; ICMA/contextual ISDA 30E are unsupported.
    pub projection_day_count: DayCountConvention,
}

/// Term-rate IRS floating leg.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TermLegTerms {
    /// Coupon/payment/notional rules.
    pub leg: LegTerms,
    /// Actual independent simple-accrual reset mechanics.
    pub resets: TermResetTerms,
    /// Explicit spread/gearing profiles.
    pub profiles: FloatingProfiles,
    /// Exact projection dependency, distinct from discounting.
    pub projection_curve: CurveKey,
    /// Required history policy.
    pub fixing_policy: FixingPolicy,
}

/// Daily compounded OIS floating leg; gearing multiplies compounded interest
/// once, spread accrues simply on unshifted coupon dates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct OvernightLegTerms {
    /// Coupon/payment/notional rules; day count must be ACT/360 or ACT/365 Fixed.
    pub leg: LegTerms,
    /// Provider-neutral index; never a market preset.
    pub index_id: IndexId,
    /// Exact observation calendar version.
    pub observation_calendar: ReferencePin,
    /// Actual lookback/shift/lockout terms and holiday day weighting.
    pub convention: OvernightConvention,
    /// Simple spread and compounded-interest gearing.
    pub profiles: FloatingProfiles,
    /// Exact index projection curve dependency.
    pub projection_curve: CurveKey,
    /// Published fixing eligibility policy.
    pub fixing_policy: FixingPolicy,
}

/// Closed initial floating mechanics; unknown optional/payoff fields fail JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FloatingLegTerms {
    /// Fixed/term-rate IRS.
    Term(TermLegTerms),
    /// Fixed/daily compounded OIS.
    Overnight(OvernightLegTerms),
}
impl FloatingLegTerms {
    /// Common coupon rules, without changing independent reset/observation terms.
    pub fn leg(&self) -> &LegTerms {
        match self {
            Self::Term(t) => &t.leg,
            Self::Overnight(t) => &t.leg,
        }
    }
    /// Explicit floating economic components.
    pub fn profiles(&self) -> &FloatingProfiles {
        match self {
            Self::Term(t) => &t.profiles,
            Self::Overnight(t) => &t.profiles,
        }
    }
    /// Actual provider-neutral index.
    pub fn index_id(&self) -> &IndexId {
        match self {
            Self::Term(t) => &t.resets.index_id,
            Self::Overnight(t) => &t.index_id,
        }
    }
    /// Exact required projection curve version.
    pub fn projection_curve(&self) -> &CurveKey {
        match self {
            Self::Term(t) => &t.projection_curve,
            Self::Overnight(t) => &t.projection_curve,
        }
    }
    /// Required history policy.
    pub fn fixing_policy(&self) -> FixingPolicy {
        match self {
            Self::Term(t) => t.fixing_policy,
            Self::Overnight(t) => t.fixing_policy,
        }
    }
}

/// Explicit same-currency collateral agreement and discount mapping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CollateralTerms {
    /// Exact caller-resolved collateral agreement version.
    pub agreement: ReferencePin,
    /// Collateral currency; cross-currency discount economics are unsupported.
    pub currency: Currency,
    /// Exact discount dependency; no index/government fallback.
    pub discount_curve: CurveKey,
}

/// Complete supplied vanilla swap trade economics. Profile steps change accrued
/// interest; there is no initial, amortization or final principal exchange.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SwapTerms {
    /// Supported economic schema version, exactly 1.
    pub schema_version: u32,
    /// Trade identity, independent of provider symbols.
    pub trade_id: TradeId,
    /// Exact trade economics version.
    pub version: Version,
    /// Cash currency of both legs.
    pub currency: Currency,
    /// Fixed payer/receiver direction.
    pub side: SwapSide,
    /// Common unadjusted effective date (spot, forward or seasoned).
    pub effective_date: Date,
    /// Common unadjusted maturity, distinct from adjusted/delayed payments.
    pub maturity_date: Date,
    /// Independent fixed leg.
    pub fixed: FixedLegTerms,
    /// Independent floating leg.
    pub floating: FloatingLegTerms,
    /// Explicit agreement/discount mapping.
    pub collateral: CollateralTerms,
}
impl SwapTerms {
    /// Validate all economic/date/profile/calendar fields and cache actual plans.
    /// Deserializing terms alone never constructs an unchecked SwapContract.
    pub fn build(self, calendars: &SwapCalendars<'_>) -> Result<SwapContract, SwapError> {
        SwapContract::new(self, calendars)
    }
}

// Economic Decimal inputs remain exact strings even when workspace serde-float
// is enabled. Bound profile allocation while decoding rather than afterwards.
mod exact_profiles {
    use convex_core::cashflows::{DatedValue, MAX_SCHEDULE_PERIODS};
    use serde::{
        de::{SeqAccess, Visitor},
        ser::SerializeSeq,
        Deserializer, Serializer,
    };
    use std::fmt;

    pub fn serialize<S: Serializer>(
        values: &[DatedValue],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        if values.len() > MAX_SCHEDULE_PERIODS {
            return Err(serde::ser::Error::custom("too many profile entries"));
        }
        let mut seq = serializer.serialize_seq(Some(values.len()))?;
        for v in values {
            seq.serialize_element(v)?;
        }
        seq.end()
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<DatedValue>, D::Error> {
        struct Profiles;
        impl<'de> Visitor<'de> for Profiles {
            type Value = Vec<DatedValue>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("at most 4096 exact string-valued dated intervals")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut values =
                    Vec::with_capacity(seq.size_hint().unwrap_or(0).min(MAX_SCHEDULE_PERIODS));
                while let Some(v) = seq.next_element::<DatedValue>()? {
                    if values.len() == MAX_SCHEDULE_PERIODS {
                        return Err(serde::de::Error::custom("bounded profile/value exceeded"));
                    }
                    values.push(v);
                }
                Ok(values)
            }
        }
        d.deserialize_seq(Profiles)
    }
}
