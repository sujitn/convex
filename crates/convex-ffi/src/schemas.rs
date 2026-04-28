//! Hand-curated JSON schemas for the wire-format DTOs.
//!
//! Foreign types like `Date`, `Decimal`, `BusinessDayConvention` don't derive
//! `JsonSchema`, so we serve a stable curated description here instead of
//! a `schema_for!` derive. Adding a new DTO is one match arm.

pub fn lookup(name: &str) -> Result<String, String> {
    let body = match name {
        "Mark" => MARK,
        "BondSpec" => BOND_SPEC,
        "CurveSpec" => CURVE_SPEC,
        "PricingRequest" => PRICING_REQUEST,
        "PricingResponse" => PRICING_RESPONSE,
        "RiskRequest" => RISK_REQUEST,
        "RiskResponse" => RISK_RESPONSE,
        "SpreadRequest" => SPREAD_REQUEST,
        "SpreadResponse" => SPREAD_RESPONSE,
        "CashflowRequest" => CASHFLOW_REQUEST,
        "CashflowResponse" => CASHFLOW_RESPONSE,
        "CurveQueryRequest" => CURVE_QUERY_REQUEST,
        "CurveQueryResponse" => CURVE_QUERY_RESPONSE,
        other => return Err(format!("unknown schema name {other:?}")),
    };
    Ok(body.to_string())
}

const MARK: &str = r##"{
  "title": "Mark",
  "description": "Trader mark — accepted as a tagged JSON object or, in MarkInput contexts, as a textual shorthand.",
  "oneOf": [
    { "type": "object", "required": ["mark","value","kind"], "properties": {
        "mark": {"const": "price"},
        "value": {"type": "number"},
        "kind": {"enum": ["Clean","Dirty"]}
    }},
    { "type": "object", "required": ["mark","value","frequency"], "properties": {
        "mark": {"const": "yield"},
        "value": {"type": "number", "description": "decimal (0.05 = 5%)"},
        "frequency": {"enum": ["Annual","SemiAnnual","Quarterly","Monthly","Zero"]}
    }},
    { "type": "object", "required": ["mark","value","benchmark"], "properties": {
        "mark": {"const": "spread"},
        "value": {"type": "object", "description": "Spread {value_bps, spread_type}"},
        "benchmark": {"type": "string"}
    }}
  ],
  "examples": ["99.5C","99.5D","4.65%","4.65%@SA","+125bps@USD.SOFR","125 OAS@USD.TSY","99-16+"]
}"##;

const BOND_SPEC: &str = r##"{
  "title": "BondSpec",
  "type": "object",
  "discriminator": "type",
  "oneOf": [
    {"description": "fixed_rate: { id?, coupon_rate, frequency, maturity, issue, day_count?, currency?, face_value?, business_day_convention? }"},
    {"description": "callable: fixed_rate fields + call_schedule:[{date,price,end_date?}], call_style?, put_schedule?"},
    {"description": "floating_rate: id?, spread_bps, rate_index?, maturity, issue, frequency?, day_count?, currency?, face_value?, cap?, floor?"},
    {"description": "zero_coupon: id?, maturity, issue, compounding?, day_count?, currency?, face_value?"},
    {"description": "sinking_fund: fixed_rate fields + schedule:[{date,amount,price?}]"}
  ],
  "example": { "type":"fixed_rate","cusip":"037833100","coupon_rate":0.05,"frequency":"SemiAnnual","maturity":"2035-01-15","issue":"2025-01-15","day_count":"Thirty360US","currency":"USD","face_value":100 }
}"##;

const CURVE_SPEC: &str = r##"{
  "title": "CurveSpec",
  "type": "object",
  "discriminator": "type",
  "oneOf": [
    {"description": "discrete: { name?, ref_date, tenors:[f64], values:[f64], value_kind?:'zero_rate'|'discount_factor', interpolation?, day_count?, compounding? }"},
    {"description": "bootstrap: { name?, ref_date, method?:'global_fit'|'piecewise', instruments:[{kind:'deposit'|'fra'|'swap'|'ois', tenor, rate}], interpolation?, day_count? }"}
  ]
}"##;

const PRICING_REQUEST: &str = r##"{
  "title": "PricingRequest",
  "type": "object",
  "required": ["bond","settlement","mark"],
  "properties": {
    "bond": {"type": "integer"},
    "settlement": {"type": "string", "format": "date"},
    "mark": {"$ref": "#/definitions/Mark"},
    "curve": {"type": ["integer","null"]},
    "quote_frequency": {"$ref": "#/definitions/Frequency"},
    "forward_curve": {"type": ["integer","null"], "description": "FRN projection curve (default: discount curve)"}
  }
}"##;

const PRICING_RESPONSE: &str = r##"{
  "title": "PricingResponse",
  "type": "object",
  "required": ["clean_price","dirty_price","accrued","ytm_decimal"],
  "properties": {
    "clean_price": {"type": "number"},
    "dirty_price": {"type": "number"},
    "accrued": {"type": "number"},
    "ytm_decimal": {"type": "number"},
    "z_spread_bps": {"type": ["number","null"]}
  }
}"##;

const RISK_REQUEST: &str = r##"{
  "title": "RiskRequest",
  "type": "object",
  "required": ["bond","settlement","mark"],
  "properties": {
    "bond": {"type": "integer"},
    "settlement": {"type": "string", "format": "date"},
    "mark": {"$ref": "#/definitions/Mark"},
    "curve": {"type": ["integer","null"]},
    "quote_frequency": {"$ref": "#/definitions/Frequency"},
    "key_rate_tenors": {"type": "array", "items": {"type": "number"}}
  }
}"##;

const RISK_RESPONSE: &str = r##"{
  "title": "RiskResponse",
  "type": "object",
  "required": ["modified_duration","macaulay_duration","convexity","dv01"],
  "properties": {
    "modified_duration": {"type": "number"},
    "macaulay_duration": {"type": "number"},
    "convexity": {"type": "number"},
    "dv01": {"type": "number"},
    "spread_duration": {"type": ["number","null"]},
    "key_rates": {"type": "array", "items": {"type":"object","properties":{"tenor":{"type":"number"},"duration":{"type":"number"}}}}
  }
}"##;

const SPREAD_REQUEST: &str = r##"{
  "title": "SpreadRequest",
  "type": "object",
  "required": ["bond","curve","settlement","mark","spread_type"],
  "properties": {
    "bond": {"type": "integer"},
    "curve": {"type": "integer"},
    "settlement": {"type": "string", "format": "date"},
    "mark": {"$ref": "#/definitions/Mark"},
    "spread_type": {"enum": ["ZSpread","GSpread","ISpread","AssetSwapPar","AssetSwapProceeds","OAS","Credit","DiscountMargin"]},
    "params": {"type":"object","properties":{
        "volatility":{"type":"number","description":"OAS short-rate volatility (decimal, 0.01 = 1%)"},
        "forward_curve":{"type":"integer","description":"DM projection curve (default: discount curve)"},
        "current_index":{"type":"number","description":"Simple-margin current index rate (decimal)"},
        "govt_curve":{"type":"integer","description":"G-spread government curve handle (required)"}
    }}
  }
}"##;

const SPREAD_RESPONSE: &str = r##"{
  "title": "SpreadResponse",
  "type": "object",
  "required": ["spread_bps"],
  "properties": {
    "spread_bps": {"type": "number"},
    "spread_dv01": {"type": ["number","null"]},
    "spread_duration": {"type": ["number","null"]},
    "option_value": {"type": ["number","null"]},
    "effective_duration": {"type": ["number","null"]},
    "effective_convexity": {"type": ["number","null"]}
  }
}"##;

const CASHFLOW_REQUEST: &str = r##"{
  "title": "CashflowRequest",
  "type": "object",
  "required": ["bond","settlement"],
  "properties": {
    "bond": {"type": "integer"},
    "settlement": {"type": "string", "format": "date"}
  }
}"##;

const CASHFLOW_RESPONSE: &str = r##"{
  "title": "CashflowResponse",
  "type": "object",
  "required": ["flows"],
  "properties": {
    "flows": {"type":"array","items":{"type":"object","required":["date","amount","kind"],"properties":{"date":{"type":"string","format":"date"},"amount":{"type":"number"},"kind":{"type":"string"}}}}
  }
}"##;

const CURVE_QUERY_REQUEST: &str = r##"{
  "title": "CurveQueryRequest",
  "type": "object",
  "required": ["curve","query","tenor"],
  "properties": {
    "curve": {"type": "integer"},
    "query": {"enum": ["zero","df","forward"]},
    "tenor": {"type": "number"},
    "tenor_end": {"type": ["number","null"]}
  }
}"##;

const CURVE_QUERY_RESPONSE: &str = r##"{
  "title": "CurveQueryResponse",
  "type": "object",
  "required": ["value"],
  "properties": { "value": {"type": "number"} }
}"##;
