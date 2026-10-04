//! Schema for the currently supported fixed-bullet operator document; runtime validation is authoritative.
use serde_json::{json, Value};
fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","additionalProperties":false,"properties":properties,"required":required})
}
fn reference(name: &str) -> Value {
    json!({"$ref":format!("#/$defs/{name}")})
}
fn nullable(value: Value) -> Value {
    json!({"anyOf":[value,{"type":"null"}]})
}
fn array(value: Value, max: usize) -> Value {
    json!({"type":"array","items":value,"maxItems":max})
}

/// Draft-2020-12 JSON schema for the supported schema-1 configuration input.
/// Cross-record integrity, time domains, finite numbers and numerical fixtures remain use-case checks.
pub fn configuration_schema() -> Value {
    let id = json!({"type":"string","minLength":1,"maxLength":128});
    let time = json!({"type":"string","format":"date-time"});
    let date = json!({"type":"string","format":"date"});
    let version = json!({"type":"integer","minimum":1});
    let age = json!({"type":"integer","minimum":1,"maximum":86400000});
    let number = json!({"type":"number"});
    let mut filter = serde_json::Map::new();
    for field in [
        "isin",
        "cusip",
        "instrument_id",
        "issuer_id",
        "sector",
        "country",
        "currency",
        "bond_type",
        "issuer_type",
        "text_search",
    ] {
        filter.insert(
            field.into(),
            nullable(json!({"type":"string","maxLength":1024})),
        );
    }
    for field in ["is_floating", "is_inflation_linked", "is_callable"] {
        filter.insert(field.into(), nullable(json!({"type":"boolean"})));
    }
    for field in ["maturity_from", "maturity_to"] {
        filter.insert(field.into(), nullable(date.clone()));
    }
    let mut definitions = serde_json::Map::new();
    definitions.insert(
        "pin".into(),
        object(json!({"id":id,"version":version}), &["id", "version"]),
    );
    definitions.insert(
        "instrument_pin".into(),
        object(
            json!({"instrument_id":id,"version":version}),
            &["instrument_id", "version"],
        ),
    );
    definitions.insert(
        "quote_key".into(),
        object(
            json!({"instrument_id":id,"source":id}),
            &["instrument_id", "source"],
        ),
    );
    definitions.insert("numerical".into(), object(json!({"policy_id":{"const":"convex-bond-router-v1"},"bump_bps":{"type":"number","exclusiveMinimum":0,"maximum":10000}}), &["policy_id","bump_bps"]));
    definitions.insert("quote".into(), object(json!({"key":reference("quote_key"),"fields":{"type":"array","minItems":1,"maxItems":12,"uniqueItems":true,"items":{"enum":["bid_price","ask_price","mid_price","last_price","bid_yield","ask_yield","mid_yield","bid_spread","ask_spread","mid_spread","bid_size","ask_size"]}},"max_age_ms":age}), &["key","fields","max_age_ms"]));
    definitions.insert(
        "input_version".into(),
        object(
            json!({"kind":id,"id":id,"version":version}),
            &["kind", "id", "version"],
        ),
    );
    definitions.insert(
        "point".into(),
        object(
            json!({"tenor_years":{"type":"number","exclusiveMinimum":0},"continuous_zero":number}),
            &["tenor_years", "continuous_zero"],
        ),
    );
    definitions.insert("curve".into(), object(json!({"curve_id":id,"version":version,"reference_date":date,"source_time":time,"recorded_at":time,"source":id,"model_id":id,"input_versions":array(reference("input_version"),256),"quality":{"enum":["Valid","Indicative","Delayed","Stale","Invalid","Demo"]},"max_age_ms":age,"interpolation":{"enum":["linear","monotone_convex"]},"extrapolation":{"enum":["flat_zero","flat_forward"]},"points":{"type":"array","minItems":2,"maxItems":4096,"items":reference("point")}}), &["curve_id","version","reference_date","source_time","recorded_at","source","model_id","input_versions","quality","max_age_ms","interpolation","extrapolation","points"]));
    definitions.insert("spread".into(), object(json!({"half_spread_bps":{"type":"number","minimum":0},"bid_spread_bps":nullable(json!({"type":"number","minimum":0})),"ask_spread_bps":nullable(json!({"type":"number","minimum":0})),"prefer_market_spread":{"type":"boolean"}}), &["half_spread_bps","bid_spread_bps","ask_spread_bps","prefer_market_spread"]));
    definitions.insert("curves".into(), object(json!({"discount_curve":id,"government_curve":nullable(id.clone()),"swap_curve":nullable(id.clone()),"frn_reference_curve":nullable(id.clone())}), &["discount_curve","government_curve","swap_curve","frn_reference_curve"]));
    definitions.insert("pricing".into(), object(json!({"config_id":id,"description":{"type":"string","maxLength":4096},"applies_to":object(Value::Object(filter), &[]),"pricing_spec":object(json!({"MarketQuote":object(json!({"side":{"const":"Mid"}}), &["side"])}), &["MarketQuote"]),"bid_ask_spread":nullable(reference("spread")),"analytics_curves":reference("curves"),"priority":nullable(json!({"type":"integer","minimum":-2147483648i64,"maximum":2147483647i64})),"active":{"const":true},"version":version,"created_at":{"type":"integer","minimum":0},"updated_at":{"type":"integer","minimum":0}}), &["config_id","description","applies_to","pricing_spec","bid_ask_spread","analytics_curves","priority","active","version","created_at","updated_at"]));
    definitions.insert("process".into(), object(json!({"storage":id,"result_transport":id,"max_concurrent_calcs":{"type":"integer","minimum":1,"maximum":64},"shared_writers":{"type":"boolean"}}), &["storage","result_transport","max_concurrent_calcs","shared_writers"]));
    definitions.insert("provider".into(), object(json!({"source":id,"capabilities":{"type":"array","minItems":1,"maxItems":3,"uniqueItems":true,"items":{"enum":["canonical_quotes","replay","live_images"]}},"credential":nullable(json!({"type":"string","pattern":"^(env://[A-Z0-9_]{1,128}|secret://[A-Za-z0-9_./-]{1,256})$"}))}), &["source","capabilities","credential"]));
    definitions.insert("runtime".into(), object(json!({"runtime_id":id,"mode":{"enum":["captured","replay","live"]},"settlement_date":date,"build_id":id,"measures":{"type":"array","minItems":1,"maxItems":4,"uniqueItems":true,"items":{"enum":["clean_price_mid","dirty_price_mid","accrued_interest","ytm_mid"]}},"numerical_settings":reference("numerical")}), &["runtime_id","mode","settlement_date","build_id","measures","numerical_settings"]));
    definitions.insert("assembly".into(), object(json!({"as_of":time,"known_at":time,"generation":{"const":1},"configuration":reference("pin"),"instruments":{"type":"array","minItems":1,"maxItems":256,"items":reference("instrument_pin")},"quotes":{"type":"array","minItems":1,"maxItems":256,"items":reference("quote")},"fixings":{"type":"array","maxItems":0},"required_curves":array(id.clone(),64),"curves":array(reference("curve"),64),"max_source_skew_ms":{"type":"integer","minimum":0,"maximum":86400000}}), &["as_of","known_at","generation","configuration","instruments","quotes","fixings","required_curves","curves","max_source_skew_ms"]));
    definitions.insert("override".into(), object(json!({"id":id,"scope":reference("quote_key"),"max_age_ms":age,"actor":id,"reason":{"type":"string","minLength":1,"maxLength":4096},"created_at":time,"expires_at":time,"approved_by":id,"approved_at":time}), &["id","scope","max_age_ms","actor","reason","created_at","expires_at","approved_by","approved_at"]));
    let mut root = object(
        json!({"schema_version":{"const":1},"pricing":reference("pricing"),"process":reference("process"),"providers":{"type":"array","minItems":1,"maxItems":64,"items":reference("provider")},"runtime":reference("runtime"),"assembly":reference("assembly"),"overrides":array(reference("override"),256)}),
        &[
            "schema_version",
            "pricing",
            "process",
            "providers",
            "runtime",
            "assembly",
            "overrides",
        ],
    );
    root["$schema"] = json!("https://json-schema.org/draft/2020-12/schema");
    root["$id"] = json!("urn:convex:configuration:1");
    root["title"] = json!("Convex fixed-bullet application configuration");
    root["$defs"] = Value::Object(definitions);
    root
}
