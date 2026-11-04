//! Standalone full supplied mixed OIS, IRS and tenor-basis calibration graph.
#![forbid(unsafe_code)]
#![deny(
    unused_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

#[path = "support/rate_fixture.rs"]
mod fixture;
use convex::{
    curve_contracts::{CurveSet, FitNumericalControls},
    rate_calibration::{build_rate_curve_set, RateCurveBuildInput},
};
use fixture::{compare_reference, Error, Fixture};
fn main() -> Result<(), Error> {
    let fixture = Fixture::load("nonflat-positive")?;
    let resources = fixture.resources(&fixture.recipes);
    let inputs: Vec<_> = fixture
        .recipes
        .iter()
        .zip(&resources)
        .map(|(recipe, resources)| RateCurveBuildInput { recipe, resources })
        .collect();
    let output = build_rate_curve_set(
        &inputs,
        &CurveSet::new(),
        &fixture.history,
        fixture.context()?,
        FitNumericalControls::default(),
        |i, d| fixture.publication(i, d),
    )?;
    compare_reference(&fixture, &output)?;
    println!(
        "{}",
        serde_json::json!({
            "curves":output.build_order,"helper_valuations":output.price_evaluations,
            "original_quotes":output.calibrations.iter().flat_map(|c|c.curve.diagnostics().quotes.iter()).collect::<Vec<_>>(),
            "qualification":"Independent synthetic engineering references; no production or quantitative certificate",
        })
    );
    Ok(())
}
