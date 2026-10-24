//! Complete pure lifecycle over explicit synthetic native terms and supplied events.
#[path = "support/lifecycle_fixture.rs"]
mod fixture;
/// Run through the public facade as well as the complete standalone replay.
pub fn run() -> Result<serde_json::Value, fixture::Error> {
    let f = fixture::Fixture::load()?;
    let state = fixture::bond_state(&f)?;
    let event = fixture::event(
        &state,
        "facade.step",
        "2024-04-02",
        convex::lifecycle::LifecycleAction::ContractStep,
    )?;
    let first = convex::lifecycle::transition(
        &state,
        &event,
        &convex::lifecycle::LifecycleInputs::default(),
    )?;
    let mut result = fixture::run()?;
    result["facade_first_step"] = serde_json::to_value(first.next.view())?;
    Ok(result)
}
/// Application output is separate from pure economic calculations.
pub fn main() -> Result<(), fixture::Error> {
    println!("{}", run()?);
    Ok(())
}
