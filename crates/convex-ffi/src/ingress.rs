//! Limits for the existing host ABI. No pricing mathematics lives here.

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
    validate_fields(&value)?;
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

fn validate_fields(value: &Value) -> Result<(), String> {
    match value {
        Value::Object(fields) => {
            if let (Some(issue), Some(maturity)) = (
                fields.get("issue").and_then(Value::as_str),
                fields.get("maturity").and_then(Value::as_str),
            ) {
                let issue =
                    convex_core::Date::parse(issue).map_err(|_| "invalid issue".to_owned())?;
                let maturity = convex_core::Date::parse(maturity)
                    .map_err(|_| "invalid maturity".to_owned())?;
                if issue >= maturity || issue.days_between(&maturity) > 150 * 366 {
                    return Err("instrument date span limit exceeded".into());
                }
            }
            for (key, value) in fields {
                let limit = match key.as_str() {
                    "instruments" => 128,
                    "key_rate_tenors" | "scenarios" | "proposals" => 64,
                    "benchmarks" => 128,
                    "basket_overrides" => 32,
                    _ => MAX_ITEMS,
                };
                if value.as_array().is_some_and(|v| v.len() > limit) {
                    return Err(format!("{key} item limit exceeded"));
                }
                if matches!(
                    key.as_str(),
                    "issue"
                        | "maturity"
                        | "settlement"
                        | "ref_date"
                        | "date"
                        | "end_date"
                        | "call_date"
                ) {
                    if let Some(text) = value.as_str() {
                        let date = convex_core::Date::parse(text)
                            .map_err(|_| "invalid date".to_owned())?;
                        if !(1..=9999).contains(&date.year()) {
                            return Err("date year limit exceeded".into());
                        }
                    }
                }
                if matches!(key.as_str(), "tenor" | "tenor_end" | "pivot_tenor") {
                    if key == "tenor"
                        && fields.contains_key("yield_rate")
                        && fields.contains_key("sovereign")
                        && fields.contains_key("maturity")
                    {
                        let tenor: convex_bonds::types::Tenor =
                            serde_json::from_value(value.clone())
                                .map_err(|_| "invalid government tenor")?;
                        if tenor.months() > 1800 {
                            return Err("government tenor exceeds 150 years".into());
                        }
                        continue;
                    }
                    check_number(value, 0.0, 150.0, key)?;
                }
                if matches!(key.as_str(), "rate" | "coupon_rate") {
                    check_number(value, -10.0, 10.0, key)?;
                }
                if key == "volatility" {
                    check_number(value, 0.0, 10.0, key)?;
                }
                if matches!(key.as_str(), "tenors" | "key_rate_tenors") {
                    if let Some(values) = value.as_array() {
                        for value in values {
                            check_number(value, 0.0, 150.0, key)?;
                        }
                    }
                }
                if matches!(key.as_str(), "face_value" | "notional_face") {
                    check_number(value, -1e18, 1e18, key)?;
                }
                validate_fields(value)?;
            }
        }
        Value::Array(values) => {
            for value in values {
                validate_fields(value)?;
            }
        }
        Value::Number(number)
            if number
                .as_f64()
                .is_none_or(|n| !n.is_finite() || n.abs() > 1e18) =>
        {
            return Err("JSON numerical magnitude limit exceeded".into());
        }
        _ => {}
    }
    Ok(())
}

fn check_number(value: &Value, minimum: f64, maximum: f64, key: &str) -> Result<(), String> {
    if value.is_null() {
        return Ok(());
    }
    // Some existing exact-decimal DTOs also accept strings. This is a bound
    // check only; pass the original input to serde so its economic digits survive.
    let number = value
        .as_f64()
        .or_else(|| value.as_str().and_then(|s| s.parse::<f64>().ok()))
        .ok_or_else(|| format!("invalid {key}"))?;
    if !number.is_finite() || number < minimum || number > maximum {
        return Err(format!("{key} numerical limit exceeded"));
    }
    Ok(())
}
