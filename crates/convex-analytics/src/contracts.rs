//! Compatibility boundaries; existing JSON/FFI DTOs retain their economic units.
//!
//! These adapters validate supplied data without repricing or inventing provenance.
//! They do not certify full contractual/model support. The existing FFI remains v1;
//! new service/application callers can opt into these stricter contracts.

#![forbid(unsafe_code)]

use convex_core::contracts::{
    BasisPoints, BusinessDate, ContractError, DecimalRate, ExactDecimal, FaceAmount, FiniteF64,
    ModelBasisPoints, ModelDecimalRate, ModelPricePer100,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::dto::{BondSpec, CouponSpec, PricingResponse, RiskResponse};

/// BondSpec accepted only after date/quantity/model-number validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "BondSpec", into = "BondSpec")]
pub struct ValidatedBondSpec {
    spec: BondSpec,
    face: FaceAmount,
}
impl ValidatedBondSpec {
    /// Inspect complete supplied terms, preserved without normalization or defaults added here.
    pub fn spec(&self) -> &BondSpec {
        &self.spec
    }
    /// Face is an exact quantity, never inferred from a rate or futures count.
    pub fn face_amount(&self) -> FaceAmount {
        // Construction validates face; do not re-enter a fallible conversion here.
        self.face
    }
    /// Existing decimal coupon; no percentage/100 conversion.
    pub fn coupon_rate(&self) -> Option<DecimalRate> {
        let coupon = match &self.spec {
            BondSpec::FixedRate(v) => &v.coupon,
            BondSpec::Callable(v) => &v.coupon,
            BondSpec::SinkingFund(v) => &v.coupon,
            _ => return None,
        };
        Some(DecimalRate::new(ExactDecimal::new(coupon.coupon_rate)))
    }
    /// Existing FRN spread in bp; no implicit rate conversion.
    pub fn spread_bps(&self) -> Option<BasisPoints> {
        match &self.spec {
            BondSpec::FloatingRate(v) => Some(BasisPoints::new(ExactDecimal::new(v.spread_bps))),
            _ => None,
        }
    }
}

// Keep validated face alongside the DTO so accessors cannot panic or revalidate.
// The wire remains the BondSpec; this state is reconstructed at ingress.

fn validate_terms(
    issue: convex_core::Date,
    maturity: convex_core::Date,
    face: Decimal,
) -> Result<FaceAmount, ContractError> {
    BusinessDate::try_from(issue)?;
    BusinessDate::try_from(maturity)?;
    if issue >= maturity || face <= Decimal::ZERO {
        return Err(ContractError {
            field: "bond",
            reason: "maturity must follow issue and face must be positive",
        });
    }
    FaceAmount::new(ExactDecimal::new(face))
}
fn validate_coupon(coupon: &CouponSpec) -> Result<FaceAmount, ContractError> {
    validate_terms(coupon.issue, coupon.maturity, coupon.face_value)
}
fn finite(value: f64, field: &'static str) -> Result<FiniteF64, ContractError> {
    FiniteF64::new(value).map_err(|_| ContractError {
        field,
        reason: "expected a finite model number",
    })
}
impl TryFrom<BondSpec> for ValidatedBondSpec {
    type Error = ContractError;
    fn try_from(spec: BondSpec) -> Result<Self, Self::Error> {
        let face = match &spec {
            BondSpec::FixedRate(v) => validate_coupon(&v.coupon)?,
            BondSpec::Callable(v) => {
                let face = validate_coupon(&v.coupon)?;
                for entry in v.call_schedule.iter().chain(&v.put_schedule) {
                    BusinessDate::try_from(entry.date)?;
                    finite(entry.price, "call_schedule.price")?;
                    if let Some(end) = entry.end_date {
                        BusinessDate::try_from(end)?;
                        if end < entry.date {
                            return Err(ContractError {
                                field: "call_schedule.end_date",
                                reason: "end precedes start",
                            });
                        }
                    }
                }
                if let Some(spread) = v.make_whole_spread_bps {
                    finite(spread, "make_whole_spread_bps")?;
                }
                face
            }
            BondSpec::FloatingRate(v) => validate_terms(v.issue, v.maturity, v.face_value)?,
            BondSpec::ZeroCoupon(v) => validate_terms(v.issue, v.maturity, v.face_value)?,
            BondSpec::SinkingFund(v) => {
                let face = validate_coupon(&v.coupon)?;
                for entry in &v.schedule {
                    BusinessDate::try_from(entry.date)?;
                }
                face
            }
        };
        Ok(Self { spec, face })
    }
}
impl From<ValidatedBondSpec> for BondSpec {
    fn from(value: ValidatedBondSpec) -> Self {
        value.spec
    }
}

/// Pricing values with canonical units, preserving each supplied model bit and sign.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct CanonicalPricingValues {
    /// Clean model price per 100.
    pub clean_price: ModelPricePer100,
    /// Dirty model price per 100.
    pub dirty_price: ModelPricePer100,
    /// Model accrued per 100, including negative accrued where supported.
    pub accrued: ModelPricePer100,
    /// Decimal yield; 0.05 remains 5%.
    pub ytm_decimal: ModelDecimalRate,
    /// Signed spread in bp; omitted stays omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub z_spread_bps: Option<ModelBasisPoints>,
}
impl TryFrom<PricingResponse> for CanonicalPricingValues {
    type Error = ContractError;
    fn try_from(value: PricingResponse) -> Result<Self, Self::Error> {
        Ok(Self {
            clean_price: ModelPricePer100::new(finite(value.clean_price, "clean_price")?),
            dirty_price: ModelPricePer100::new(finite(value.dirty_price, "dirty_price")?),
            accrued: ModelPricePer100::new(finite(value.accrued, "accrued")?),
            ytm_decimal: ModelDecimalRate::new(finite(value.ytm_decimal, "ytm_decimal")?),
            z_spread_bps: value
                .z_spread_bps
                .map(|v| finite(v, "z_spread_bps").map(ModelBasisPoints::new))
                .transpose()?,
        })
    }
}
impl From<CanonicalPricingValues> for PricingResponse {
    fn from(value: CanonicalPricingValues) -> Self {
        Self {
            clean_price: value.clean_price.value().get(),
            dirty_price: value.dirty_price.value().get(),
            accrued: value.accrued.value().get(),
            ytm_decimal: value.ytm_decimal.value().get(),
            z_spread_bps: value.z_spread_bps.map(|v| v.value().get()),
        }
    }
}

/// Validated risk response, retaining its original sign and notional/price basis.
///
/// This is deliberately not a conversion to RiskSensitivity: duration-based DV01 alone
/// does not supply a factor key, reporting currency, quantity, bump or snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "RiskResponse", into = "RiskResponse")]
pub struct ValidatedRiskResponse(RiskResponse);
impl ValidatedRiskResponse {
    /// Read the unchanged risk response representation.
    pub fn response(&self) -> &RiskResponse {
        &self.0
    }
}
impl TryFrom<RiskResponse> for ValidatedRiskResponse {
    type Error = ContractError;
    fn try_from(value: RiskResponse) -> Result<Self, Self::Error> {
        for (number, field) in [
            (value.modified_duration, "modified_duration"),
            (value.macaulay_duration, "macaulay_duration"),
            (value.convexity, "convexity"),
            (value.dv01, "dv01"),
        ] {
            finite(number, field)?;
        }
        if let Some(number) = value.spread_duration {
            finite(number, "spread_duration")?;
        }
        for key_rate in &value.key_rates {
            finite(key_rate.tenor, "key_rates.tenor")?;
            finite(key_rate.duration, "key_rates.duration")?;
            if key_rate.tenor <= 0.0 {
                return Err(ContractError {
                    field: "key_rates.tenor",
                    reason: "expected a positive tenor",
                });
            }
        }
        Ok(Self(value))
    }
}
impl From<ValidatedRiskResponse> for RiskResponse {
    fn from(value: ValidatedRiskResponse) -> Self {
        value.0
    }
}
