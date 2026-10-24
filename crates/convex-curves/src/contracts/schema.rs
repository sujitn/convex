//! Schemas for original curve method enums; no interpolation changes.
impl schemars::JsonSchema for crate::InterpolationMethod {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "InterpolationMethod".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type":"string", "enum":["Linear","LogLinear",
            "CubicSpline","MonotoneConvex","FlatForward","PiecewiseConstant",
            "NelsonSiegel","Svensson","Segmented","Delegated"]})
    }
}
impl schemars::JsonSchema for crate::ExtrapolationMethod {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ExtrapolationMethod".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"oneOf":[
            {"type":"string", "enum":["None","Flat","Linear","FlatForward"]},
            {"type":"object", "required":["UfrConvergence"], "additionalProperties":false,
             "properties":{"UfrConvergence":{"type":"object","required":["ufr","alpha"],
                "additionalProperties":false,"properties":{"ufr":{"type":"number"},
                "alpha":{"type":"number"}}}}}
        ]})
    }
}
