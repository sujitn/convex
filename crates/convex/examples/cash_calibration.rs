//! Pure full cash quotes -> government/issuer/repo curves -> fixed-date carry.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[path = "support/cash_fixture.rs"]
mod fixture;
use convex::{cash_calibration::*, curve_contracts::*};
use fixture::*;

fn main() -> Result<(), Error> {
    let mut original_quotes = Vec::new();
    let mut total = 0;
    for id in ["positive", "negative"] {
        let f = Fixture::load(id)?;
        let resources = f.resources(&f.recipes);
        let input: Vec<_> = f
            .recipes
            .iter()
            .zip(&resources)
            .map(|(recipe, resources)| CashCurveBuildInput { recipe, resources })
            .collect();
        let out = build_cash_curve_set(&input, &f.supplied()?, FitNumericalControls::default())?;
        compare_reference(&f, &out)?;
        total += out.price_evaluations;
        for c in &out.calibrations {
            original_quotes.extend(c.curve.diagnostics().quotes.clone());
        }
        for (t, expected) in f
            .forwards
            .iter()
            .zip(f.row["carry"].as_array().ok_or("missing original carry")?)
        {
            let resources = f.forward_resources(t);
            let value = price_repo_bond_forward(&RepoBondForwardInputs {
                terms: t,
                resources: &resources,
                delivery_calendar: f.delivery_calendar(),
                curves: &out.curves,
            })?;
            compare(
                value.clean_forward_per100,
                number(&expected["expected"]["clean_forward_per100"])?,
                1e-8,
                1e-11,
            )?;
        }
    }
    let c = CreditFixture::load()?;
    let recipes = vec![c.recipe.clone()];
    let resources = resources(&recipes, &c.calendar);
    let input = [CashCurveBuildInput {
        recipe: &recipes[0],
        resources: &resources[0],
    }];
    let out = build_cash_curve_set(&input, &c.supplied, FitNumericalControls::default())?;
    total += out.price_evaluations;
    for fitted in &out.calibrations {
        original_quotes.extend(fitted.curve.diagnostics().quotes.clone());
        for (value, expected) in fitted.repriced_helpers.iter().zip(
            c.data["credit_case"]["expected"]
                .as_array()
                .ok_or("missing independent credit bond")?,
        ) {
            let CashHelperValuation::Bond { value, .. } = value else {
                return Err("missing complete credit bond price".into());
            };
            compare_price(value, expected, &c.data["tolerances"])?;
        }
    }
    println!(
        "{}",
        serde_json::json!({"original_native_quotes":original_quotes,"actual_helper_valuations":total,"qualification":"Synthetic cash/basis/carry engineering; supplied original CDS calibration reused; no release certificate"})
    );
    Ok(())
}
