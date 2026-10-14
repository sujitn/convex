//! Checked JavaScript-object ingress shared by the fixed bond conveniences.

use crate::dto::BondParams;
use js_sys::{Array, Object, Reflect};
use wasm_bindgen::JsValue;

const FIELD_NAMES: &[&str] = &[
    "coupon_rate",
    "maturity_date",
    "issue_date",
    "settlement_date",
    "face_value",
    "frequency",
    "day_count",
    "currency",
    "first_coupon_date",
    "market",
    "instrument_type",
    "yield_convention",
    "compounding",
    "settlement_days",
    "ex_dividend_days",
    "use_business_days",
];

#[cfg(test)]
fn admitted_field(name: &str) -> bool {
    FIELD_NAMES.contains(&name)
}

/// serde-wasm-bindgen visits only declared struct fields, so serde's JSON unknown
/// field guard is insufficient here. Inspect all own keys before decoding, and
/// copy only own values into a snapshot so inherited values cannot supply terms.
fn snapshot_record(value: JsValue, fields: &[&str]) -> Result<JsValue, String> {
    if value.is_null() || !value.is_object() || Array::is_array(&value) {
        return Err("Record must be a plain object".into());
    }
    let prototype =
        Reflect::get_prototype_of(&value).map_err(|_| "Cannot inspect record prototype")?;
    let ordinary = Object::get_prototype_of(&Object::new());
    if !prototype.is_null() && prototype != ordinary {
        return Err("Record must be a plain object".into());
    }
    let keys = Reflect::own_keys(&value).map_err(|_| "Cannot inspect record fields")?;
    if keys.length() as usize > fields.len() {
        return Err("Too many record fields".into());
    }
    let snapshot = Object::new();
    for name in fields {
        Reflect::set(&snapshot, &JsValue::from_str(name), &JsValue::UNDEFINED)
            .map_err(|_| "Cannot initialize record snapshot")?;
    }
    for key in keys.iter() {
        let name = key
            .as_string()
            .ok_or("Symbol record fields are unsupported")?;
        if !fields.contains(&name.as_str()) {
            return Err(format!("Unknown record field: {name}"));
        }
        let owned = Reflect::get(&value, &key).map_err(|_| "Cannot read record field")?;
        Reflect::set(&snapshot, &key, &owned).map_err(|_| "Cannot snapshot record field")?;
    }
    Ok(snapshot.into())
}

pub(crate) fn decode_bond_params(value: JsValue) -> Result<BondParams, String> {
    serde_wasm_bindgen::from_value(snapshot_record(value, FIELD_NAMES)?)
        .map_err(|error| format!("Invalid bond parameters: {error}"))
}

pub(crate) fn decode_curve_points(value: JsValue) -> Result<Vec<crate::dto::CurvePoint>, String> {
    if !Array::is_array(&value) {
        return Err("Curve points must be an array".into());
    }
    let points = Array::from(&value);
    if points.length() == 0 || points.length() > 4096 {
        return Err("Curve point count must be1..4096".into());
    }
    points
        .iter()
        .map(|point| {
            serde_wasm_bindgen::from_value(snapshot_record(point, &["date", "rate"])?)
                .map_err(|error| format!("Invalid curve point: {error}"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn admitted_names_match_every_serialized_fixed_field() {
        let input = serde_json::json!({"coupon_rate":5.0,"maturity_date":"2030-06-15",
            "issue_date":"2020-06-15","settlement_date":"2026-06-15"});
        let params: BondParams = serde_json::from_value(input).unwrap();
        let encoded = serde_json::to_value(params).unwrap();
        let actual: BTreeSet<_> = encoded
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        let declared: BTreeSet<_> = FIELD_NAMES.iter().copied().collect();
        assert_eq!(actual, declared);
        assert_eq!(FIELD_NAMES.len(), declared.len());
        assert!(FIELD_NAMES.iter().all(|name| admitted_field(name)));
    }

    #[test]
    fn retired_and_unknown_names_are_never_admitted() {
        for name in [
            "call_schedule",
            "volatility",
            "cap",
            "floor",
            "notice_days",
            "",
            "couponRate",
        ] {
            assert!(!admitted_field(name), "{name} was admitted");
        }
    }
}
