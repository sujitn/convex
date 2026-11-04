//! Structured factor identities and explicit signed one-basis-point measures.

use serde::{Deserialize, Serialize};

use super::{
    BasisPoints, ContractError, CurveId, EntityId, FiniteF64, IndexId, InstrumentId, ModelId,
    SnapshotId, Version,
};
use crate::Currency;

/// Risk-factor family; quote risk and model-parameter risk remain distinct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum FactorFamily {
    /// Rate quote or zero curve.
    InterestRate,
    /// Credit quote or bond spread.
    CreditSpread,
    /// Credit hazard parameter.
    Hazard,
    /// Recovery assumption.
    Recovery,
    /// Futures price quote.
    FuturesPrice,
    /// Cash security clean/dirty price quotation.
    CashPrice,
    /// Explicit pricing-model parameter, distinct from market volatility quotes.
    Model,
}

/// Provider-neutral identity of the object being bumped.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum FactorTarget {
    /// Discount, projection, credit or spread curve.
    Curve {
        /// Internal curve identity.
        curve_id: CurveId,
    },
    /// Index quote.
    Index {
        /// Internal index identity.
        index_id: IndexId,
    },
    /// Reference entity or issuer parameter.
    Entity {
        /// Internal entity identity.
        entity_id: EntityId,
    },
    /// Instrument's own quote.
    Instrument {
        /// Internal instrument identity.
        instrument_id: InstrumentId,
    },
    /// Supplied model identity; its immutable revision belongs to the coordinate.
    Model {
        /// Original model identity.
        model_id: ModelId,
        /// Exact supplied string revision, including nonnumeric curve/model keys.
        model_version: String,
    },
    /// Two exact inputs moved together by one explicitly coupled model factor.
    CurvePair {
        /// Original currency discount identity.
        discount_curve_id: CurveId,
        /// Original exact discount revision.
        discount_curve_version: String,
        /// Original index projection identity.
        projection_curve_id: CurveId,
        /// Original exact projection revision.
        projection_curve_version: String,
    },
}

/// Native quote definition; these are not interchangeable risk buckets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum QuoteType {
    /// Instrument's own yield.
    Yield,
    /// Zero rate.
    ZeroRate,
    /// OIS market par rate.
    OisParRate,
    /// Term-rate swap market par rate.
    SwapParRate,
    /// Quoted CDS spread.
    CdsSpread,
    /// CDS upfront fraction.
    CdsUpfront,
    /// Dirty CDS cash upfront, distinct from clean upfront.
    CdsDirtyUpfront,
    /// Bond Z spread.
    ZSpread,
    /// Hazard parameter.
    HazardRate,
    /// Recovery fraction.
    RecoveryRate,
    /// Quoted futures price; not a one-bp rate measure.
    FuturesPrice,
    /// Dated deposit quotation.
    DepositRate,
    /// Publication-aware dated FRA quotation.
    FraRate,
    /// Uniform spread on a declared basis-swap leg.
    BasisSwapSpread,
    /// Coupon of a full original par cash bond.
    BondParCoupon,
    /// Actual dated simple repo quotation.
    RepoRate,
    /// Bill bank-discount yield, distinct from cashflow yield.
    BankDiscountYield,
    /// Rate-future implied annualized rate; native points move in the opposite direction.
    RateFutureRate,
    /// Residual promised-cash issuer basis, distinct from credit and recovery.
    ResidualBasis,
    /// Explicit optional-bond model OAS, distinct from deterministic Z spread.
    OasSpread,
    /// Clean price per100 of original face.
    CashCleanPrice,
    /// Dirty price per100 of original face.
    CashDirtyPrice,
    /// Constant HW1F mean reversion per year.
    Hw1fMeanReversion,
    /// Constant HW1F short-rate diffusion per square-root year.
    Hw1fVolatility,
    /// Explicit parallel discount-and-projection move, distinct from either axis.
    JointZeroRate,
    /// Supplied Bachelier annual decimal-rate volatility per square-root year.
    CouponNormalVolatility,
    /// Supplied shifted-Black annual relative volatility per square-root year.
    CouponLognormalVolatility,
    /// Supplied shifted-Black decimal-rate displacement.
    CouponLognormalDisplacement,
    /// Supplied terminal decimal-rate loading on one common unit normal factor.
    CouponNormalFactorLoading,
}

impl QuoteType {
    /// Exact supplied coupon parameters, distinct from market quotes and HW1F.
    pub const fn is_coupon_parameter(self) -> bool {
        matches!(
            self,
            Self::CouponNormalVolatility
                | Self::CouponLognormalVolatility
                | Self::CouponLognormalDisplacement
                | Self::CouponNormalFactorLoading
        )
    }
}

/// Definition of the bump/rebuild operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum RiskBasis {
    /// Bump a raw product quote without calibrated curve risk claims.
    RawQuote,
    /// Bump a curve node or zero rate.
    ZeroNode,
    /// Bump an input market quote and recalibrate dependent curves.
    MarketQuoteRecalibrated,
    /// Bump a model parameter.
    ModelParameter,
}

/// Positive dated tenor (D/W/M/Y), or the explicitly declared parallel/spot bucket.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct FactorTenor(String);
#[cfg(feature = "schemars")]
impl schemars::JsonSchema for FactorTenor {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "FactorTenor".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type":"string", "pattern":"^([1-9][0-9]{0,4}[DWMY]|parallel|spot)$"})
    }
}
impl FactorTenor {
    /// Validate without approximation, date generation or case normalization.
    pub fn new(value: impl Into<String>) -> Result<Self, ContractError> {
        let value = value.into();
        if value == "parallel" || value == "spot" {
            return Ok(Self(value));
        }
        let b = value.as_bytes();
        if !(2..=6).contains(&b.len())
            || !b"DWMY".contains(&b[b.len() - 1])
            || b[0] == b'0'
            || !b[..b.len() - 1].iter().all(u8::is_ascii_digit)
        {
            return Err(ContractError::new(
                "tenor",
                "expected a positive D/W/M/Y tenor, parallel or spot",
            ));
        }
        Ok(Self(value))
    }
    /// Exact tenor label; not an approximate number of years.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for FactorTenor {
    type Error = ContractError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}
impl From<FactorTenor> for String {
    fn from(value: FactorTenor) -> Self {
        value.0
    }
}

/// Validated factor key; every identity dimension participates in equality/hash.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "FactorDefinition", into = "FactorDefinition")]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schemars", schemars(with = "FactorDefinition"))]
pub struct RiskFactorKey(FactorDefinition);

/// Explicit factor definition used by the checked constructor.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct FactorDefinition {
    /// Risk-factor family.
    pub family: FactorFamily,
    /// Curve/index/entity/instrument whose quote or parameter moves.
    pub target: FactorTarget,
    /// Quote currency, distinct from risk reporting currency.
    pub currency: Currency,
    /// Explicit quote bucket.
    pub tenor: FactorTenor,
    /// Native quotation.
    pub quote_type: QuoteType,
    /// Bump and recalibration basis.
    pub basis: RiskBasis,
    /// Pinned convention pack.
    pub convention_version: Version,
    /// Pinned model/bump semantics.
    pub model_version: ModelId,
}
impl RiskFactorKey {
    /// Reject incoherent family/quote/basis combinations.
    pub fn new(value: FactorDefinition) -> Result<Self, ContractError> {
        let valid = match value.family {
            FactorFamily::InterestRate => matches!(
                value.quote_type,
                QuoteType::Yield
                    | QuoteType::ZeroRate
                    | QuoteType::OisParRate
                    | QuoteType::SwapParRate
                    | QuoteType::DepositRate
                    | QuoteType::FraRate
                    | QuoteType::BasisSwapSpread
                    | QuoteType::BondParCoupon
                    | QuoteType::RepoRate
                    | QuoteType::BankDiscountYield
                    | QuoteType::RateFutureRate
                    | QuoteType::JointZeroRate
            ),
            FactorFamily::CreditSpread => matches!(
                value.quote_type,
                QuoteType::CdsSpread
                    | QuoteType::CdsUpfront
                    | QuoteType::CdsDirtyUpfront
                    | QuoteType::ZSpread
                    | QuoteType::ResidualBasis
                    | QuoteType::OasSpread
            ),
            FactorFamily::Hazard => value.quote_type == QuoteType::HazardRate,
            FactorFamily::Recovery => value.quote_type == QuoteType::RecoveryRate,
            FactorFamily::FuturesPrice => value.quote_type == QuoteType::FuturesPrice,
            FactorFamily::CashPrice => matches!(
                value.quote_type,
                QuoteType::CashCleanPrice | QuoteType::CashDirtyPrice
            ),
            FactorFamily::Model => {
                matches!(
                    value.quote_type,
                    QuoteType::Hw1fMeanReversion | QuoteType::Hw1fVolatility
                ) || value.quote_type.is_coupon_parameter()
            }
        };
        let basis_valid = match value.quote_type {
            q if q.is_coupon_parameter() => value.basis == RiskBasis::ModelParameter,
            QuoteType::ZeroRate | QuoteType::JointZeroRate => value.basis == RiskBasis::ZeroNode,
            QuoteType::HazardRate
            | QuoteType::RecoveryRate
            | QuoteType::Hw1fMeanReversion
            | QuoteType::Hw1fVolatility => value.basis == RiskBasis::ModelParameter,
            QuoteType::FuturesPrice | QuoteType::OasSpread => value.basis == RiskBasis::RawQuote,
            QuoteType::ResidualBasis => value.basis == RiskBasis::ZeroNode,
            _ => matches!(
                value.basis,
                RiskBasis::RawQuote | RiskBasis::MarketQuoteRecalibrated
            ),
        };
        let target_valid = match value.family {
            FactorFamily::InterestRate => !matches!(value.target, FactorTarget::Entity { .. }),
            FactorFamily::CreditSpread | FactorFamily::Hazard => {
                !matches!(value.target, FactorTarget::Index { .. })
            }
            FactorFamily::Recovery => matches!(value.target, FactorTarget::Entity { .. }),
            FactorFamily::FuturesPrice => matches!(value.target, FactorTarget::Instrument { .. }),
            FactorFamily::CashPrice => matches!(
                value.target,
                FactorTarget::Curve { .. } | FactorTarget::Instrument { .. }
            ),
            FactorFamily::Model => matches!(value.target, FactorTarget::Model { .. }),
        };
        let label = |s: &str| {
            !s.is_empty() && s.len() <= 128 && s.trim() == s && !s.chars().any(char::is_control)
        };
        let target_valid = target_valid
            && match &value.target {
                FactorTarget::Model { model_version, .. } => {
                    value.family == FactorFamily::Model && label(model_version)
                }
                FactorTarget::CurvePair {
                    discount_curve_id,
                    discount_curve_version,
                    projection_curve_id,
                    projection_curve_version,
                } => {
                    value.quote_type == QuoteType::JointZeroRate
                        && discount_curve_id != projection_curve_id
                        && label(discount_curve_version)
                        && label(projection_curve_version)
                }
                _ => value.quote_type != QuoteType::JointZeroRate,
            };
        if !valid || !basis_valid || !target_valid {
            return Err(ContractError::new(
                "factor",
                "inconsistent family, quote, target or bump basis",
            ));
        }
        Ok(Self(value))
    }
    /// Read the immutable complete factor definition.
    pub fn definition(&self) -> &FactorDefinition {
        &self.0
    }
}
impl TryFrom<FactorDefinition> for RiskFactorKey {
    type Error = ContractError;
    fn try_from(value: FactorDefinition) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}
impl From<RiskFactorKey> for FactorDefinition {
    fn from(value: RiskFactorKey) -> Self {
        value.0
    }
}

/// Signed monetary change normalized to a +1 bp quote move, not a derivative.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum SignedRiskMeasure {
    /// (PV(q+0.0001) - PV(q-0.0001))/2. A long fixed bond is typically negative.
    Pv01Signed,
    /// Analogous signed change for a +1 bp credit spread quote move.
    Cs01Signed,
    /// Conventional loss sign: -pv01_signed.
    Dv01Loss,
}

/// Computation method, explicitly reported with the finite-difference bump.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum RiskMethod {
    /// Central finite difference, normalized to one bp.
    Central,
    /// Analytic sensitivity checked against the reference bump convention.
    Analytic,
}

/// Complete signed measure; futures price delta/hazard/upfront cannot enter as PV01.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "SensitivityWire", into = "SensitivityWire")]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schemars", schemars(with = "SensitivityWire"))]
pub struct RiskSensitivity(SensitivityWire);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
struct SensitivityWire {
    factor: RiskFactorKey,
    measure: SignedRiskMeasure,
    value: FiniteF64,
    unit: RiskUnit,
    currency: Currency,
    bump: BasisPoints,
    method: RiskMethod,
    snapshot_id: SnapshotId,
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
enum RiskUnit {
    #[serde(rename = "currency_per_bp")]
    Unit,
}

impl RiskSensitivity {
    /// Supply a normalized result and actual positive bump; no scaling is inferred.
    pub fn new(
        factor: RiskFactorKey,
        measure: SignedRiskMeasure,
        value: FiniteF64,
        currency: Currency,
        bump: BasisPoints,
        method: RiskMethod,
        snapshot_id: SnapshotId,
    ) -> Result<Self, ContractError> {
        Self::try_from(SensitivityWire {
            factor,
            measure,
            value,
            unit: RiskUnit::Unit,
            currency,
            bump,
            method,
            snapshot_id,
        })
    }
    /// Complete factor key for safe aggregation.
    pub fn factor(&self) -> &RiskFactorKey {
        &self.0.factor
    }
    /// Monetary signed value normalized to one bp; multiply directly by a bp move.
    pub const fn value(&self) -> FiniteF64 {
        self.0.value
    }
    /// Explicit sign convention.
    pub const fn measure(&self) -> SignedRiskMeasure {
        self.0.measure
    }
    /// Information set consumed by this sensitivity.
    pub fn snapshot_id(&self) -> &SnapshotId {
        &self.0.snapshot_id
    }
    /// Currency of the monetary sensitivity, distinct from factor quote currency.
    pub const fn currency(&self) -> Currency {
        self.0.currency
    }
    /// Actual finite-difference bump in bp, before normalization to one bp.
    pub const fn bump(&self) -> BasisPoints {
        self.0.bump
    }
    /// Declared quantitative method.
    pub const fn method(&self) -> RiskMethod {
        self.0.method
    }
}
impl TryFrom<SensitivityWire> for RiskSensitivity {
    type Error = ContractError;
    fn try_from(value: SensitivityWire) -> Result<Self, Self::Error> {
        let factor = value.factor.definition();
        let valid = match value.measure {
            SignedRiskMeasure::Pv01Signed | SignedRiskMeasure::Dv01Loss => {
                factor.family == FactorFamily::InterestRate
            }
            SignedRiskMeasure::Cs01Signed => {
                factor.family == FactorFamily::CreditSpread
                    && matches!(factor.quote_type, QuoteType::CdsSpread | QuoteType::ZSpread)
            }
        };
        if !valid || value.bump.value().decimal() <= rust_decimal::Decimal::ZERO {
            return Err(ContractError::new(
                "risk",
                "measure incompatible with factor, or bump is nonpositive",
            ));
        }
        Ok(Self(value))
    }
}
impl From<RiskSensitivity> for SensitivityWire {
    fn from(value: RiskSensitivity) -> Self {
        value.0
    }
}
