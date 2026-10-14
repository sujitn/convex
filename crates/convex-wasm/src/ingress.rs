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

fn admitted_field(name: &str) -> bool {
    FIELD_NAMES.contains(&name)
}

/// serde-wasm-bindgen visits only declared struct fields, so serde's JSON unknown
/// field guard is insufficient here. Inspect all own keys before decoding, and
/// copy only own values into a snapshot so inherited values cannot supply terms.
pub(crate) fn decode_bond_params(value: JsValue) -> Result<BondParams, String> {
    if value.is_null() || !value.is_object() || Array::is_array(&value) {
        return Err("Bond parameters must be a plain object".into());
    }
    let prototype =
        Reflect::get_prototype_of(&value).map_err(|_| "Cannot inspect bond parameter prototype")?;
    let ordinary = Object::get_prototype_of(&Object::new());
    if !prototype.is_null() && prototype != ordinary {
        return Err("Bond parameters must be a plain object".into());
    }
    let keys = Reflect::own_keys(&value).map_err(|_| "Cannot inspect bond parameter fields")?;
    if keys.length() as usize > FIELD_NAMES.len() {
        return Err("Too many bond parameter fields".into());
    }
    let snapshot = Object::new();
    for name in FIELD_NAMES {
        Reflect::set(&snapshot, &JsValue::from_str(name), &JsValue::UNDEFINED)
            .map_err(|_| "Cannot initialize bond parameter snapshot")?;
    }
    for key in keys.iter() {
        let name = key
            .as_string()
            .ok_or("Symbol bond parameter fields are unsupported")?;
        if !admitted_field(&name) {
            return Err(format!("Unknown bond parameter field: {name}"));
        }
        let owned = Reflect::get(&value, &key).map_err(|_| "Cannot read bond parameter field")?;
        Reflect::set(&snapshot, &key, &owned)
            .map_err(|_| "Cannot snapshot bond parameter field")?;
    }
    serde_wasm_bindgen::from_value(snapshot.into())
        .map_err(|error| format!("Invalid bond parameters: {error}"))
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
