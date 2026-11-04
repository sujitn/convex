//! Schema for an existing convention; original date mathematics is unchanged.
use crate::calendars::BusinessDayConvention;
impl schemars::JsonSchema for BusinessDayConvention {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "BusinessDayConvention".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type":"string", "enum":["Unadjusted","Following",
            "ModifiedFollowing","Preceding","ModifiedPreceding","Nearest","EndOfMonth"]})
    }
}
