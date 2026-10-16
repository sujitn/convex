//! Complete native DTO schemas, with registry-name alternatives owned by FFI.

use serde_json::{json, Value};

pub fn lookup(name: &str) -> Result<String, String> {
    let mut schema = match name {
        "LibraryRequest" => convex::library::request_schema(),
        "BondSpec" => convex_analytics::dto::bond_spec_schema(),
        "CurveSpec" => convex_analytics::dto::curve_spec_schema(),
        _ => convex_analytics::dto::rpc_schema(name)
            .ok_or_else(|| format!("unknown schema name {name:?}"))?,
    };
    let original = schema.clone();
    for path in crate::dispatch::schema_reference_paths(name) {
        allow_registered_name(&mut schema, path, &original)?;
    }
    Ok(schema.to_string())
}

fn allow_registered_name(schema: &mut Value, path: &[&str], root: &Value) -> Result<(), String> {
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        let target = reference
            .strip_prefix('#')
            .and_then(|pointer| root.pointer(pointer))
            .ok_or_else(|| {
                "generated registry field schema has an unresolved reference".to_owned()
            })?;
        let mut expanded = target.clone();
        if let (Some(fields), Some(overrides)) = (expanded.as_object_mut(), schema.as_object()) {
            for (key, value) in overrides {
                if key != "$ref" {
                    fields.insert(key.clone(), value.clone());
                }
            }
        }
        *schema = expanded;
    }
    let Some((head, remaining)) = path.split_first() else {
        return Err("generated registry field path is empty".to_owned());
    };
    let field = schema
        .get_mut("properties")
        .and_then(|fields| fields.get_mut(*head))
        .ok_or_else(|| format!("generated schema omitted registry field {head}"))?;
    if remaining.is_empty() {
        let native = field.take();
        *field = json!({"anyOf": [native, {"type": "string", "minLength": 1,
            "description": "Registered bond or curve name, resolved to a handle by this FFI boundary."}]});
        Ok(())
    } else {
        allow_registered_name(field, remaining, root)
    }
}
