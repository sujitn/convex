//! Caller-supplied dates and UTC timestamps without wall-clock access.

use chrono::{DateTime, Datelike, SecondsFormat, Timelike, Utc};
use serde::{Deserialize, Serialize};

use super::ContractError;
use crate::Date;

/// A valid business date in the canonical four-digit ISO calendar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct BusinessDate(Date);

#[cfg(feature = "schemars")]
impl schemars::JsonSchema for BusinessDate {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "BusinessDate".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type":"string", "format":"date", "minLength":10,"maxLength":10,
            "pattern":"^[0-9]{4}-[0-9]{2}-[0-9]{2}$", "description":"Valid YYYY-MM-DD in years 0001–9999."})
    }
}

impl BusinessDate {
    /// Parse exactly YYYY-MM-DD, years 0001–9999; no timestamp or shorthand.
    pub fn parse(value: &str) -> Result<Self, ContractError> {
        let bytes = value.as_bytes();
        if bytes.len() != 10
            || bytes[4] != b'-'
            || bytes[7] != b'-'
            || !bytes
                .iter()
                .enumerate()
                .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit())
        {
            return Err(ContractError::new("date", "expected YYYY-MM-DD"));
        }
        let date =
            Date::parse(value).map_err(|_| ContractError::new("date", "invalid calendar date"))?;
        Self::try_from(date)
    }
    /// Return the existing pure-library date type.
    pub const fn date(self) -> Date {
        self.0
    }
}
impl TryFrom<Date> for BusinessDate {
    type Error = ContractError;
    fn try_from(value: Date) -> Result<Self, Self::Error> {
        if !(1..=9999).contains(&value.year()) {
            return Err(ContractError::new("date", "year outside 0001–9999"));
        }
        Ok(Self(value))
    }
}
impl TryFrom<String> for BusinessDate {
    type Error = ContractError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}
impl From<BusinessDate> for String {
    fn from(value: BusinessDate) -> Self {
        value.0.to_string()
    }
}

/// A UTC instant with at most nanosecond precision. Leap seconds are unsupported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct UtcTimestamp(DateTime<Utc>);

#[cfg(feature = "schemars")]
impl schemars::JsonSchema for UtcTimestamp {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "UtcTimestamp".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type":"string", "format":"date-time", "minLength":20,"maxLength":30,
            "pattern":"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(\\.[0-9]{1,9})?Z$",
            "description":"UTC instant, at most nanosecond precision, years 0001–9999; leap seconds unsupported."})
    }
}

impl UtcTimestamp {
    /// Parse YYYY-MM-DDTHH:MM:SS[.fraction]Z; precision is never truncated.
    pub fn parse(value: &str) -> Result<Self, ContractError> {
        let b = value.as_bytes();
        let valid_fraction = b.len() == 20
            || ((22..=30).contains(&b.len())
                && b[19] == b'.'
                && b[20..b.len() - 1].iter().all(u8::is_ascii_digit));
        if !valid_fraction || b.get(10) != Some(&b'T') || b.last() != Some(&b'Z') {
            return Err(ContractError::new(
                "timestamp",
                "expected UTC ISO timestamp with at most nine fractional digits",
            ));
        }
        BusinessDate::parse(&value[..10])?;
        let parsed = DateTime::parse_from_rfc3339(value)
            .map_err(|_| ContractError::new("timestamp", "invalid UTC timestamp"))?;
        Self::from_datetime(parsed.with_timezone(&Utc))
    }
    /// Validate an explicitly supplied native UTC instant; never call now().
    pub fn from_datetime(value: DateTime<Utc>) -> Result<Self, ContractError> {
        if !(1..=9999).contains(&value.year()) || value.nanosecond() >= 1_000_000_000 {
            return Err(ContractError::new(
                "timestamp",
                "unsupported year or leap second",
            ));
        }
        Ok(Self(value))
    }
    /// Supplied native instant, preserving precision.
    pub const fn datetime(self) -> DateTime<Utc> {
        self.0
    }
}
impl TryFrom<String> for UtcTimestamp {
    type Error = ContractError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}
impl From<UtcTimestamp> for String {
    fn from(value: UtcTimestamp) -> Self {
        value.0.to_rfc3339_opts(SecondsFormat::AutoSi, true)
    }
}
