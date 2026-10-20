//! An unavailable government operation cannot remain in the advertised enum.
use convex_mcp::server::{ComputeSpreadParams, SpreadKind};
use rmcp::schemars::schema_for;
use serde_json::json;

#[test]
fn compute_spread_schema_and_admission_have_no_government_proxy() {
    let schema = serde_json::to_value(schema_for!(SpreadKind)).unwrap();
    // Documented variants have separate schema branches in schemars 1.x.
    let variants: Vec<_> = schema["oneOf"]
        .as_array()
        .unwrap_or_else(|| panic!("unexpected spread schema: {schema}"))
        .iter()
        .map(|variant| variant["const"].clone())
        .collect();
    assert_eq!(variants, [json!("z_spread"), json!("i_spread")], "{schema}");
    let request = |kind| {
        json!({"bond":"supplied.bond","curve":"supplied.curve", "settlement":{"year":2025,"month":4,"day":15},
            "clean_price_per_100":99.5,"kind":kind})
    };
    for kind in ["z_spread", "i_spread"] {
        assert!(serde_json::from_value::<ComputeSpreadParams>(request(kind)).is_ok());
    }
    let error = serde_json::from_value::<ComputeSpreadParams>(request("g_spread")).unwrap_err();
    assert!(error.to_string().contains("unknown variant `g_spread`"));
}
