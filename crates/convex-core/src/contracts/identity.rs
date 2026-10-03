//! Opaque internal identities and immutable record versions.

use serde::{Deserialize, Serialize};
use std::fmt;

use super::ContractError;

macro_rules! identifier {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        #[cfg(feature = "schemars")]
        impl schemars::JsonSchema for $name {
            fn schema_name() -> std::borrow::Cow<'static, str> { stringify!($name).into() }
            fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
                schemars::json_schema!({"type":"string", "minLength":1, "maxLength":128,
                    "pattern":"^[A-Za-z0-9][A-Za-z0-9_.:/-]*$"})
            }
        }

        impl $name {
            /// Validate an opaque token of 1–128 ASCII characters without normalization.
            pub fn new(value: impl Into<String>) -> Result<Self, ContractError> {
                let value = value.into();
                validate_token(&value, stringify!($name))?;
                Ok(Self(value))
            }

            /// Return the exact identifier supplied by the caller.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = ContractError;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

pub(crate) fn validate_token(value: &str, field: &'static str) -> Result<(), ContractError> {
    if value.is_empty()
        || value.len() > 128
        || !value.as_bytes()[0].is_ascii_alphanumeric()
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.:/-".contains(&b))
    {
        return Err(ContractError::new(
            field,
            "expected a bounded ASCII identifier",
        ));
    }
    Ok(())
}

identifier!(
    InstrumentId,
    "Internal instrument identity, independent of supplier symbol mappings."
);
identifier!(TradeId, "Internal trade identity.");
identifier!(CurveId, "Internal curve identity.");
identifier!(IndexId, "Internal fixing or projection index identity.");
identifier!(
    EntityId,
    "Internal issuer, reference entity or legal entity identity."
);
identifier!(
    SnapshotId,
    "Identity of an immutable supplied information set."
);
identifier!(RequestId, "Correlation identity of a calculation request.");
identifier!(ObservationId, "Identity of a consumed market observation.");
identifier!(
    SourceId,
    "Provider-neutral source identity; not a vendor SDK object."
);
identifier!(ModelId, "Explicit model or build identity/version label.");
identifier!(MeasureId, "Stable name of a requested or returned measure.");

/// Positive immutable record version. Zero is never an implicit current version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "u64", into = "u64")]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schemars", schemars(with = "std::num::NonZeroU64"))]
pub struct Version(u64);

impl Version {
    /// Reject an unspecified version (zero).
    pub fn new(value: u64) -> Result<Self, ContractError> {
        if value == 0 {
            return Err(ContractError::new("version", "expected a positive version"));
        }
        Ok(Self(value))
    }
    /// The exact pinned version.
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl TryFrom<u64> for Version {
    type Error = ContractError;
    fn try_from(value: u64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}
impl From<Version> for u64 {
    fn from(value: Version) -> Self {
        value.0
    }
}

/// Version of the canonical wire contract. Unknown versions fail ingress.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum SchemaVersion {
    /// Canonical units and signed-measure contract.
    #[serde(rename = "2.0")]
    V2,
}

/// A pinned economic subject; resolving the record is an application concern.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum ValuationSubject {
    /// Valuation of one immutable instrument definition.
    InstrumentVersion {
        /// Internal instrument identity.
        instrument_id: InstrumentId,
        /// Definition version.
        version: Version,
    },
    /// Valuation of one immutable trade including its economics.
    TradeVersion {
        /// Internal trade identity.
        trade_id: TradeId,
        /// Trade version.
        version: Version,
    },
}

impl TryFrom<crate::ids::InstrumentId> for InstrumentId {
    type Error = ContractError;
    fn try_from(value: crate::ids::InstrumentId) -> Result<Self, Self::Error> {
        Self::new(value.0)
    }
}
impl From<InstrumentId> for crate::ids::InstrumentId {
    fn from(value: InstrumentId) -> Self {
        Self(value.0)
    }
}
impl TryFrom<crate::ids::CurveId> for CurveId {
    type Error = ContractError;
    fn try_from(value: crate::ids::CurveId) -> Result<Self, Self::Error> {
        Self::new(value.0)
    }
}
impl From<CurveId> for crate::ids::CurveId {
    fn from(value: CurveId) -> Self {
        Self(value.0)
    }
}
