//! Bounds for the new full-term supplied-input API. No financial formulas.

use serde::de::{DeserializeSeed, Error, MapAccess, SeqAccess, Visitor};
use serde_json::Value;
use std::fmt;

pub const MAX_INPUT_BYTES: usize = 2 * 1024 * 1024;
const MAX_NODES: usize = 32_768;
const MAX_DEPTH: usize = 64;
const MAX_ITEMS: usize = 4_096;
const MAX_TEXT_BYTES: usize = 8_192;

/// Parse without duplicate-field collapse, with allocation and recursion bounds.
pub fn parse(input: &str) -> Result<Value, String> {
    if input.len() > MAX_INPUT_BYTES {
        return Err("input byte limit exceeded".into());
    }
    let mut remaining = MAX_NODES;
    let mut deserializer = serde_json::Deserializer::from_str(input);
    let value = Seed {
        remaining: &mut remaining,
        depth: 0,
    }
    .deserialize(&mut deserializer)
    .map_err(|e| e.to_string())?;
    deserializer.end().map_err(|e| e.to_string())?;
    Ok(value)
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

/// Decode full native inputs without duplicate collapse, economic-term omission,
/// or precision changes from numeric JSON cash. Old v1 ingress remains separate.
pub(super) fn decode(input: &str) -> super::LibraryResult<super::LibraryRequest> {
    let original = parse(input).map_err(|e| {
        if e.contains("limit exceeded") {
            super::LibraryError::limit(&e)
        } else {
            super::LibraryError::invalid("json", &e)
        }
    })?;
    let typed: super::LibraryRequest = serde_json::from_value(original.clone())
        .map_err(|e| super::LibraryError::invalid("request", &e.to_string()))?;
    let canonical = serde_json::to_value(&typed)
        .map_err(|e| super::LibraryError::invalid("request", &e.to_string()))?;
    check_complete(&original, &canonical, "request")?;
    Ok(typed)
}
fn check_complete(original: &Value, canonical: &Value, path: &str) -> super::LibraryResult<()> {
    match (original, canonical) {
        (Value::Object(a), Value::Object(b)) => {
            for (key, value) in a {
                let target = b.get(key).ok_or_else(|| {
                    super::LibraryError::invalid(
                        path,
                        "unknown or silently discarded economic field",
                    )
                })?;
                check_complete(value, target, &format!("{path}.{key}"))?;
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            if a.len() != b.len() {
                return Err(super::LibraryError::invalid(
                    path,
                    "input items were discarded",
                ));
            }
            for (i, (x, y)) in a.iter().zip(b).enumerate() {
                check_complete(x, y, &format!("{path}[{i}]"))?;
            }
        }
        (Value::Number(_), Value::String(_)) => {
            return Err(super::LibraryError::invalid(
                path,
                "exact decimal strings required; numeric cash can lose precision",
            ))
        }
        (Value::String(a), Value::String(b)) if a != b => {
            let exact = rust_decimal::Decimal::from_str_exact(a)
                .ok()
                .zip(rust_decimal::Decimal::from_str_exact(b).ok());
            if !exact.is_some_and(|(x, y)| x == y) {
                return Err(super::LibraryError::invalid(
                    path,
                    "economic text/decimal was normalized or lost precision",
                ));
            }
        }
        _ => {}
    }
    Ok(())
}
