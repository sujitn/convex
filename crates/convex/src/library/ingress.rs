//! Complete native financial-term reconstruction over the shared bounded JSON owner.

pub use convex_core::contracts::json::parse;
use serde_json::Value;

/// Decode full native inputs without duplicate collapse, economic-term omission,
/// or precision changes from numeric JSON cash. Old v1 ingress remains separate.
pub(super) fn decode(input: &str) -> super::LibraryResult<super::LibraryRequest> {
    decode_using(input, parse)
}

pub(super) fn decode_book(input: &str) -> super::LibraryResult<super::LibraryRequest> {
    let request = decode_using(input, convex_core::contracts::json::parse_book)?;
    if !matches!(
        request.operation,
        super::LibraryOperation::BookPnl { .. }
            | super::LibraryOperation::GraphHedgeRisk { .. }
            | super::LibraryOperation::LifecycleHedgeRisk { .. }
            | super::LibraryOperation::Hedge { .. }
    ) {
        return Err(super::LibraryError::invalid(
            "operation",
            "managed book operation required",
        ));
    }
    Ok(request)
}

fn decode_using(
    input: &str,
    parser: fn(&str) -> Result<Value, String>,
) -> super::LibraryResult<super::LibraryRequest> {
    let original = parser(input).map_err(|e| {
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
