//! Standalone complete supplied cash quote risk through the public facade.
#[path = "support/quote_fixture.rs"]
mod support;
fn main() -> Result<(), support::Error> {
    let f = support::Fixture::load()?;
    let result = f.raw_yield(&f.helper.terms.face_value.to_string(), 0.0001)?;
    result.validate()?;
    let json = serde_json::to_string(&result)?;
    let decoded: convex::quote_risk::QuoteSensitivity = serde_json::from_str(&json)?;
    decoded.validate()?;
    if result.value >= 0.0 {
        return Err("long full-cash yield PV01 must be negative".into());
    }
    println!("{json}");
    Ok(())
}
