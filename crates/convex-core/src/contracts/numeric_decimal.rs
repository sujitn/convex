//! Checked number-or-string decoding for existing native Decimal inputs.
//!
//! Numeric values retain their established binary64/shortest-decimal wire
//! interpretation. Strings preserve exact decimal digits. Both forms reject
//! rounding, overflow and nonzero erasure. Raw JSON callers must first use
//! [`super::json::parse`] to retain lexical nonzero intent before binary64 parsing.
//! Exact-string contracts continue to use [`super::decimal_serde`].

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{de::Visitor, Deserializer, Serializer};

fn parse(text: &str, context: &str) -> Result<Decimal, String> {
    if text.len() > 8192 {
        return Err(format!(
            "{context} decimal text exceeds the admitted byte bound"
        ));
    }
    if let Some((base, _)) = text.split_once(['e', 'E']) {
        // Check the mantissa before the dependency's scientific parser can round it.
        Decimal::from_str_exact(base).map_err(|_| {
            format!("{context} decimal mantissa requires rounding or exceeds representation")
        })?;
        Decimal::from_scientific(text)
            .map_err(|_| format!("{context} decimal exponent exceeds exact representation"))
    } else {
        Decimal::from_str_exact(text).map_err(|_| {
            format!("{context} decimal requires rounding or exceeds exact representation")
        })
    }
}

/// Parse exact bounded text without rounding or erasing a nonzero value.
///
/// # Errors
/// Rejects malformed, oversized or unrepresentable decimal values.
pub fn from_text(text: &str) -> Result<Decimal, String> {
    parse(text, "financial")
}

/// Serialize exact digits, retaining numeric output when it round-trips exactly.
///
/// # Errors
/// Propagates an output serializer failure.
pub fn serialize<S: Serializer>(value: &Decimal, serializer: S) -> Result<S::Ok, S::Error> {
    if let Some(number) = value.to_f64().filter(|number| {
        number.is_finite() && from_text(&number.to_string()).is_ok_and(|decoded| decoded == *value)
    }) {
        serializer.serialize_f64(number)
    } else {
        serializer.serialize_str(&value.to_string())
    }
}

/// Decode an exactly representable financial Decimal using the shared admission policy.
///
/// # Errors
/// Rejects nonfinite, oversized, rounded or out-of-range values.
pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Decimal, D::Error> {
    deserialize_named(deserializer, "financial")
}

/// Decode with a bounded static diagnostic context, preserving existing diagnostics.
///
/// # Errors
/// Applies the same admission as [`deserialize`]; labels longer than 64 UTF-8
/// bytes are rejected before formatting diagnostics.
pub fn deserialize_named<'de, D: Deserializer<'de>>(
    deserializer: D,
    context: &'static str,
) -> Result<Decimal, D::Error> {
    if context.len() > 64 {
        return Err(serde::de::Error::custom(
            "decimal diagnostic context exceeds byte bound",
        ));
    }
    struct NumberVisitor(&'static str);
    impl<'de> Visitor<'de> for NumberVisitor {
        type Value = Decimal;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(
                formatter,
                "a finite {} number or exact bounded decimal string",
                self.0
            )
        }

        fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Decimal, E> {
            Ok(Decimal::from(value))
        }

        fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Decimal, E> {
            Ok(Decimal::from(value))
        }

        fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<Decimal, E> {
            if !value.is_finite() {
                return Err(E::custom(format!("finite {} number required", self.0)));
            }
            parse(&value.to_string(), self.0).map_err(E::custom)
        }

        fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Decimal, E> {
            parse(value, self.0).map_err(E::custom)
        }
    }
    deserializer.deserialize_any(NumberVisitor(context))
}

/// Structural schema for number-or-exact-string input; checked decoding enforces
/// coefficient, scale, finiteness and nonzero representability.
#[cfg(feature = "schemars")]
pub fn schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({"anyOf":[{"type":"number"},{"type":"string","maxLength":8192}],
        "description":"Financial Decimal: numeric inputs retain binary64 wire interpretation; strings retain exact digits. Checked decoding rejects rounding, overflow and nonzero erasure."})
}

/// Nullable fields use the same checked scalar decoder; omission stays with serde defaults.
pub mod optional {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    #[serde(transparent)]
    struct Present(#[serde(deserialize_with = "super::deserialize")] Decimal);

    /// Serialize a missing value or exact present Decimal with the scalar policy.
    ///
    /// # Errors
    /// Propagates an output serializer failure.
    pub fn serialize<S: Serializer>(
        value: &Option<Decimal>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match value {
            Some(value) => super::serialize(value, serializer),
            None => serializer.serialize_none(),
        }
    }

    /// Decode null or a present exactly representable Decimal.
    ///
    /// # Errors
    /// Rejects every malformed or unrepresentable present scalar.
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Decimal>, D::Error> {
        Option::<Present>::deserialize(deserializer).map(|value| value.map(|value| value.0))
    }

    /// Nullable structural schema with the same checked present-value admission.
    #[cfg(feature = "schemars")]
    pub fn schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let scalar = super::schema(generator).to_value();
        schemars::json_schema!({"anyOf":[scalar,{"type":"null"}]})
    }
}
