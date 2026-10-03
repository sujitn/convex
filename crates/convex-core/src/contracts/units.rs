//! Exact contract quantities and finite model values with explicit wire units.

use rust_decimal::{prelude::ToPrimitive, Decimal, RoundingStrategy};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::ContractError;
use crate::Currency;

/// Exact decimal, encoded as a plain decimal string even when serde-float is enabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ExactDecimal(Decimal);

#[cfg(feature = "schemars")]
impl schemars::JsonSchema for ExactDecimal {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ExactDecimal".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type":"string", "minLength":1, "maxLength":59,
            "pattern":"^-?[0-9]+(\\.[0-9]{1,28})?$",
            "description":"Exact decimal string; checked ingress additionally enforces 96-bit coefficient bounds without rounding."})
    }
}

impl ExactDecimal {
    /// Preserve an already exact native decimal and its scale.
    pub const fn new(value: Decimal) -> Self {
        Self(value)
    }
    /// Reject exponent notation, separators, whitespace, rounding and overflow.
    pub fn parse(value: &str) -> Result<Self, ContractError> {
        let unsigned = value.strip_prefix('-').unwrap_or(value);
        let mut parts = unsigned.split('.');
        let whole = parts.next().unwrap_or_default();
        let fraction = parts.next();
        if value.len() > 59
            || whole.is_empty()
            || !whole.bytes().all(|b| b.is_ascii_digit())
            || fraction.is_some_and(|p| {
                p.is_empty() || p.len() > 28 || !p.bytes().all(|b| b.is_ascii_digit())
            })
            || parts.next().is_some()
        {
            return Err(ContractError::new(
                "value",
                "expected a bounded plain decimal string",
            ));
        }
        Decimal::from_str_exact(value)
            .map(Self)
            .map_err(|_| ContractError::new("value", "decimal exceeds exact representation"))
    }
    /// Exact value at the supplied scale.
    pub const fn decimal(self) -> Decimal {
        self.0
    }
    /// Explicitly cross into the numerical model; binary representation may round.
    pub fn to_model(self) -> Result<FiniteF64, ContractError> {
        let value = self
            .0
            .to_f64()
            .ok_or_else(|| ContractError::new("value", "model conversion overflow"))?;
        FiniteF64::new(value)
    }
    /// Round a model result for a contract using the caller's scale and rule.
    pub fn from_model(value: FiniteF64, policy: DecimalRounding) -> Result<Self, ContractError> {
        let decimal = Decimal::from_f64_retain(value.get()).ok_or_else(|| {
            ContractError::new("value", "model value exceeds decimal representation")
        })?;
        // A nonzero number that underflows the exact representation is not zero.
        if value.get() != 0.0 && decimal.is_zero() {
            return Err(ContractError::new(
                "value",
                "model value underflows decimal representation",
            ));
        }
        Ok(Self(decimal.round_dp_with_strategy(
            policy.scale,
            policy.mode.strategy(),
        )))
    }
}
impl Serialize for ExactDecimal {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}
impl<'de> Deserialize<'de> for ExactDecimal {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::parse(&String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// Finite numerical-model scalar. NaN/infinity cannot be constructed or decoded.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "f64", into = "f64")]
pub struct FiniteF64(f64);

#[cfg(feature = "schemars")]
impl schemars::JsonSchema for FiniteF64 {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "FiniteF64".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type":"number", "minimum":-f64::MAX,"maximum":f64::MAX})
    }
}

impl FiniteF64 {
    /// Validate a model scalar; negative rates and signed PVs are valid.
    pub fn new(value: f64) -> Result<Self, ContractError> {
        if !value.is_finite() {
            return Err(ContractError::new(
                "value",
                "expected a finite model number",
            ));
        }
        Ok(Self(value))
    }
    /// The unchanged model value (including the sign of zero).
    pub const fn get(self) -> f64 {
        self.0
    }
}
impl TryFrom<f64> for FiniteF64 {
    type Error = ContractError;
    fn try_from(value: f64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}
impl From<FiniteF64> for f64 {
    fn from(value: FiniteF64) -> Self {
        value.0
    }
}

/// Explicit posting rounding rule; never inferred from currency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum RoundingMode {
    /// Halfway values round to the nearest even value.
    HalfEven,
    /// Halfway values round away from zero.
    HalfAwayFromZero,
    /// Discard excess decimal places towards zero.
    TowardZero,
}
impl RoundingMode {
    fn strategy(self) -> RoundingStrategy {
        match self {
            Self::HalfEven => RoundingStrategy::MidpointNearestEven,
            Self::HalfAwayFromZero => RoundingStrategy::MidpointAwayFromZero,
            Self::TowardZero => RoundingStrategy::ToZero,
        }
    }
}

/// Validated scale and rounding mode for an explicit model-to-contract conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RoundingWire", into = "RoundingWire")]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schemars", schemars(with = "RoundingWire"))]
pub struct DecimalRounding {
    scale: u32,
    mode: RoundingMode,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
struct RoundingWire {
    #[cfg_attr(feature = "schemars", schemars(range(max = 28)))]
    scale: u32,
    mode: RoundingMode,
}
impl DecimalRounding {
    /// Choose at most 28 decimal places and a named rounding rule.
    pub fn new(scale: u32, mode: RoundingMode) -> Result<Self, ContractError> {
        if scale > 28 {
            return Err(ContractError::new("scale", "expected 0–28 decimal places"));
        }
        Ok(Self { scale, mode })
    }
    /// Declared contract decimal places.
    pub const fn scale(self) -> u32 {
        self.scale
    }
    /// Declared rounding mode for diagnostics and replay.
    pub const fn mode(self) -> RoundingMode {
        self.mode
    }
}
impl TryFrom<RoundingWire> for DecimalRounding {
    type Error = ContractError;
    fn try_from(value: RoundingWire) -> Result<Self, Self::Error> {
        Self::new(value.scale, value.mode)
    }
}
impl From<DecimalRounding> for RoundingWire {
    fn from(value: DecimalRounding) -> Self {
        Self {
            scale: value.scale,
            mode: value.mode,
        }
    }
}

// One macro for the closed set of units; it supplies no runtime unit conversion.
macro_rules! tagged_value {
    ($name:ident, $unit_type:ident, $scalar:ty, $unit:literal, $description:literal) => {
        #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
        #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
        enum $unit_type {
            #[serde(rename = $unit)]
            Unit,
        }

        #[doc = $description]
        #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
        #[serde(deny_unknown_fields)]
        #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
        pub struct $name {
            value: $scalar,
            unit: $unit_type,
        }
        impl $name {
            /// Construct from a scalar already validated for this representation.
            pub const fn new(value: $scalar) -> Self {
                Self {
                    value,
                    unit: $unit_type::Unit,
                }
            }
            /// Read the scalar in this type's explicitly named unit.
            pub const fn value(self) -> $scalar {
                self.value
            }
        }
    };
}

tagged_value!(
    PricePer100,
    PriceUnit,
    ExactDecimal,
    "price_per_100",
    "Exact price or accrued amount per 100 face; never a decimal rate."
);
tagged_value!(
    DecimalRate,
    RateUnit,
    ExactDecimal,
    "decimal_rate",
    "Exact decimal rate: 0.05 means 5%; negative rates are permitted."
);
tagged_value!(
    BasisPoints,
    BpsUnit,
    ExactDecimal,
    "bp",
    "Exact basis points: 100 means 1 percentage point."
);
tagged_value!(
    ModelPricePer100,
    ModelPriceUnit,
    FiniteF64,
    "price_per_100",
    "Finite model price/accrued per 100; no contract rounding implied."
);
tagged_value!(
    ModelDecimalRate,
    ModelRateUnit,
    FiniteF64,
    "decimal_rate",
    "Finite model decimal rate, preserving binary bits across DTO boundaries."
);
tagged_value!(
    ModelBasisPoints,
    ModelBpsUnit,
    FiniteF64,
    "bp",
    "Finite model basis-point value, preserving negative spread signs."
);

/// Nonnegative exact face quantity. Direction is a separate trade field.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "FaceWire", into = "FaceWire")]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schemars", schemars(with = "FaceWire"))]
pub struct FaceAmount(ExactDecimal);
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
struct FaceWire {
    value: ExactDecimal,
    unit: FaceUnit,
}
#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
enum FaceUnit {
    #[serde(rename = "face_amount")]
    Unit,
}
impl FaceAmount {
    /// Reject negative face; zero is valid for a closed economic position.
    pub fn new(value: ExactDecimal) -> Result<Self, ContractError> {
        if value.decimal().is_sign_negative() && !value.decimal().is_zero() {
            return Err(ContractError::new(
                "face_amount",
                "direction must be supplied separately",
            ));
        }
        Ok(Self(value))
    }
    /// Exact face quantity.
    pub const fn value(self) -> ExactDecimal {
        self.0
    }
}
impl TryFrom<FaceWire> for FaceAmount {
    type Error = ContractError;
    fn try_from(value: FaceWire) -> Result<Self, Self::Error> {
        Self::new(value.value)
    }
}
impl From<FaceAmount> for FaceWire {
    fn from(value: FaceAmount) -> Self {
        Self {
            value: value.0,
            unit: FaceUnit::Unit,
        }
    }
}

/// Signed integer futures position; encoded as a string to preserve 64-bit precision in clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct FuturesContracts {
    #[serde(with = "integer_string")]
    #[cfg_attr(feature = "schemars", schemars(schema_with = "integer_string::schema"))]
    value: i64,
    unit: ContractsUnit,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
enum ContractsUnit {
    #[serde(rename = "futures_contracts")]
    Unit,
}
impl FuturesContracts {
    /// Supply a signed integer count; fractional or face quantities have a different type.
    pub const fn new(value: i64) -> Self {
        Self {
            value,
            unit: ContractsUnit::Unit,
        }
    }
    /// Exact signed contract count.
    pub const fn value(self) -> i64 {
        self.value
    }
}
mod integer_string {
    use super::*;
    pub fn serialize<S: Serializer>(value: &i64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.to_string())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<i64, D::Error> {
        let text = String::deserialize(deserializer)?;
        let unsigned = text.strip_prefix('-').unwrap_or(&text);
        if text.len() > 20
            || unsigned.is_empty()
            || !unsigned.bytes().all(|b| b.is_ascii_digit())
            || (unsigned.len() > 1 && unsigned.starts_with('0'))
        {
            return Err(serde::de::Error::custom(
                "expected a bounded integer string",
            ));
        }
        text.parse()
            .map_err(|_| serde::de::Error::custom("integer outside signed 64-bit bounds"))
    }
    #[cfg(feature = "schemars")]
    pub fn schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type":"string", "maxLength":20, "pattern":"^-?(0|[1-9][0-9]*)$",
            "description":"Signed 64-bit integer; encoded as a string for exact client roundtrips."})
    }
}

/// Exact signed cash or ledger amount with required currency.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Money {
    value: ExactDecimal,
    currency: Currency,
    unit: MoneyUnit,
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
enum MoneyUnit {
    #[serde(rename = "currency")]
    Unit,
}
impl Money {
    /// Supply signed amount and currency explicitly; no FX or rounding is performed.
    pub const fn new(value: ExactDecimal, currency: Currency) -> Self {
        Self {
            value,
            currency,
            unit: MoneyUnit::Unit,
        }
    }
    /// Exact signed cash amount.
    pub const fn value(self) -> ExactDecimal {
        self.value
    }
    /// Amount currency.
    pub const fn currency(self) -> Currency {
        self.currency
    }
}

/// Finite signed model PV in an explicit currency, before ledger posting/rounding.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ModelMoney {
    value: FiniteF64,
    currency: Currency,
    unit: MoneyUnit,
}
impl ModelMoney {
    /// Supply a finite model amount and currency without converting to ledger precision.
    pub const fn new(value: FiniteF64, currency: Currency) -> Self {
        Self {
            value,
            currency,
            unit: MoneyUnit::Unit,
        }
    }
    /// Unrounded signed model amount.
    pub const fn value(self) -> FiniteF64 {
        self.value
    }
    /// Model amount currency.
    pub const fn currency(self) -> Currency {
        self.currency
    }
}

impl BasisPoints {
    /// Convert bp to decimal rate only if division by 10,000 is exactly representable.
    pub fn to_decimal_rate(self) -> Result<DecimalRate, ContractError> {
        let value = self.value.decimal();
        let converted = value
            .checked_div(Decimal::from(10_000))
            .ok_or_else(|| ContractError::new("bp", "rate conversion overflow"))?;
        if converted.checked_mul(Decimal::from(10_000)) != Some(value) {
            return Err(ContractError::new("bp", "rate conversion would round"));
        }
        Ok(DecimalRate::new(ExactDecimal::new(converted)))
    }
}
impl DecimalRate {
    /// Convert decimal rate to basis points with checked bounds.
    pub fn to_basis_points(self) -> Result<BasisPoints, ContractError> {
        let value = self.value.decimal();
        let converted = value
            .checked_mul(Decimal::from(10_000))
            .ok_or_else(|| ContractError::new("decimal_rate", "basis-point conversion overflow"))?;
        if converted.checked_div(Decimal::from(10_000)) != Some(value) {
            return Err(ContractError::new(
                "decimal_rate",
                "basis-point conversion would round",
            ));
        }
        Ok(BasisPoints::new(ExactDecimal::new(converted)))
    }
}

/// Closed quantity set. The required wire tag prevents face/contract ambiguity.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum PositionQuantity {
    /// Exact nonnegative face quantity; direction belongs to the trade.
    Face(FaceAmount),
    /// Signed integer contract position.
    Contracts(FuturesContracts),
}

/// Explicit value representations for canonical result measures.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum MeasureValue {
    /// Exact money; signed and currency-qualified.
    Money(Money),
    /// Model PV before exact contract/posting rounding.
    ModelMoney(ModelMoney),
    /// Exact price at contract precision.
    Price(PricePer100),
    /// Exact decimal rate.
    Rate(DecimalRate),
    /// Exact basis points.
    Bps(BasisPoints),
    /// Finite model price, without premature rounding.
    ModelPrice(ModelPricePer100),
    /// Finite model decimal rate.
    ModelRate(ModelDecimalRate),
    /// Finite model basis points.
    ModelBps(ModelBasisPoints),
    /// Structured signed factor risk with its bump, basis and snapshot.
    Risk(super::RiskSensitivity),
}
