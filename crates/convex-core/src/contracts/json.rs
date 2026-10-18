//! One bounded JSON admission owner for native and application boundaries.
//! No financial formulas, runtime, provider, clock or database access.

use serde::de::{DeserializeSeed, Error, MapAccess, SeqAccess, Visitor};
use serde_json::Value;
use std::fmt;

/// Maximum UTF8 JSON input size admitted before parsing or numeric checks.
pub const MAX_INPUT_BYTES: usize = 2 * 1024 * 1024;
const MAX_NODES: usize = 32_768;
const MAX_DEPTH: usize = 64;
const MAX_ITEMS: usize = 4_096;
const MAX_TEXT_BYTES: usize = 8_192;

/// Parse without duplicate-field collapse, with allocation and recursion bounds.
///
/// # Errors
/// Rejects malformed, duplicate, oversized or over-nested JSON, and nonzero
/// numeric tokens that binary64 parsing would erase to zero.
pub fn parse(input: &str) -> Result<Value, String> {
    parse_bounded(input, MAX_NODES)
}

/// Parse a managed mixed-book request with a larger aggregate node budget.
/// Byte, depth, per-container, text, duplicate and lexical-number guards stay the same.
///
/// # Errors
/// Rejects invalid JSON or exhausted bounds, exactly as [`parse`].
pub fn parse_book(input: &str) -> Result<Value, String> {
    parse_bounded(input, 131_072)
}

fn parse_bounded(input: &str, nodes: usize) -> Result<Value, String> {
    if input.len() > MAX_INPUT_BYTES {
        return Err("input byte limit exceeded".into());
    }
    reject_erased_numbers(input)?;
    let mut deserializer = serde_json::Deserializer::from_str(input);
    let mut remaining = nodes;
    let value = Seed {
        remaining: &mut remaining,
        depth: 0,
    }
    .deserialize(&mut deserializer)
    .map_err(|e| e.to_string())?;
    deserializer.end().map_err(|e| e.to_string())?;
    Ok(value)
}

/// Deserialize a value with the shared duplicate, depth, node and text bounds.
/// Raw JSON callers still use [`parse`] for its byte and lexical-number guards.
///
/// # Errors
/// Rejects duplicate fields, malformed values and exhausted structural bounds.
pub fn deserialize_value<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Value, D::Error> {
    let mut remaining = MAX_NODES;
    Seed {
        remaining: &mut remaining,
        depth: 0,
    }
    .deserialize(deserializer)
}

// Keep numeric intent until the formal JSON parser can consume it. Strings and
// their escapes are skipped without interpretation; syntax and overflow errors
// retain the existing parser owner. Input bytes are already bounded above.
fn reject_erased_numbers(input: &str) -> Result<(), String> {
    let bytes = input.as_bytes();
    let (mut quoted, mut escaped, mut index) = (false, false, 0usize);
    while index < bytes.len() {
        let byte = bytes[index];
        if quoted {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
            index += 1;
            continue;
        }
        if byte == b'"' {
            quoted = true;
            index += 1;
            continue;
        }
        if byte == b'-' || byte.is_ascii_digit() {
            let start = index;
            index += 1;
            while index < bytes.len()
                && (bytes[index].is_ascii_digit()
                    || matches!(bytes[index], b'.' | b'e' | b'E' | b'+' | b'-'))
            {
                index += 1;
            }
            let token = &input[start..index];
            if token.parse::<f64>().is_ok_and(|value| value == 0.0) {
                let mantissa = token
                    .split_once(['e', 'E'])
                    .map_or(token, |(mantissa, _)| mantissa);
                if mantissa.bytes().any(|digit| (b'1'..=b'9').contains(&digit)) {
                    return Err("nonzero JSON number underflows binary64 representation".into());
                }
            }
        } else {
            index += 1;
        }
    }
    Ok(())
}

struct Seed<'a> {
    remaining: &'a mut usize,
    depth: usize,
}
impl<'de> DeserializeSeed<'de> for Seed<'_> {
    type Value = Value;
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<Value, D::Error> {
        if self.depth > MAX_DEPTH || *self.remaining == 0 {
            return Err(D::Error::custom("JSON depth or node limit exceeded"));
        }
        *self.remaining -= 1;
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Seed<'_> {
    type Value = Value;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("bounded JSON")
    }
    fn visit_bool<E: Error>(self, v: bool) -> Result<Value, E> {
        Ok(v.into())
    }
    fn visit_i64<E: Error>(self, v: i64) -> Result<Value, E> {
        Ok(v.into())
    }
    fn visit_u64<E: Error>(self, v: u64) -> Result<Value, E> {
        Ok(v.into())
    }
    fn visit_f64<E: Error>(self, v: f64) -> Result<Value, E> {
        serde_json::Number::from_f64(v)
            .map(Value::Number)
            .ok_or_else(|| E::custom("number must be finite"))
    }
    fn visit_str<E: Error>(self, v: &str) -> Result<Value, E> {
        self.visit_string(v.to_owned())
    }
    fn visit_string<E: Error>(self, v: String) -> Result<Value, E> {
        if v.len() > MAX_TEXT_BYTES {
            return Err(E::custom("JSON text limit exceeded"));
        }
        Ok(v.into())
    }
    fn visit_unit<E: Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Value, A::Error> {
        let mut values = Vec::new();
        while let Some(value) = a.next_element_seed(Seed {
            remaining: self.remaining,
            depth: self.depth + 1,
        })? {
            if values.len() >= MAX_ITEMS {
                return Err(A::Error::custom("JSON array limit exceeded"));
            }
            values.push(value);
        }
        Ok(Value::Array(values))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Value, A::Error> {
        let mut values = serde_json::Map::new();
        while let Some(key) = a.next_key::<String>()? {
            if key.len() > MAX_TEXT_BYTES || values.len() >= MAX_ITEMS {
                return Err(A::Error::custom("JSON object limit exceeded"));
            }
            if values.contains_key(&key) {
                return Err(A::Error::custom("duplicate JSON field"));
            }
            let value = a.next_value_seed(Seed {
                remaining: self.remaining,
                depth: self.depth + 1,
            })?;
            values.insert(key, value);
        }
        Ok(Value::Object(values))
    }
}
