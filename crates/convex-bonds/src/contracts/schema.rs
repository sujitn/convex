//! Calendar identity schema; supplied values/version pins remain separate.
impl schemars::JsonSchema for crate::types::CalendarId {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "CalendarId".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type":"string", "minLength":1})
    }
}
