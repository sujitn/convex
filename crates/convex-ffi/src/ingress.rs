//! Limits for the existing host ABI. No pricing mathematics lives here.

use serde_json::Value;

pub use convex_core::contracts::json::MAX_INPUT_BYTES;
const MAX_ITEMS: usize = 4_096;

/// Parse without duplicate-field collapse, with allocation and recursion bounds.
pub fn parse(input: &str) -> Result<Value, String> {
    let value = convex_core::contracts::json::parse(input)?;
    validate_fields(&value)?;
    Ok(value)
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
