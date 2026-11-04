//! CVX-060 standalone negative-rate fit, equivalent views and explicit rolling.
#![forbid(unsafe_code)]
#![deny(
    unused_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use convex::{
    curve_contracts::*, Compounding, Currency, Date, DayCountConvention as Dc,
    ExtrapolationMethod as Ext, InterpolationMethod as Int,
};

fn date(value: &str) -> CurveContractResult<Date> {
    Date::parse(value).map_err(|e| CurveContractError::InvalidInput {
        field: "example.date".into(),
        reason: e.to_string(),
    })
}
fn require(value: bool, reason: &str) -> CurveContractResult<()> {
    if !value {
        return Err(CurveContractError::Numerical(reason.into()));
    }
    Ok(())
}
fn discount(curve: &BuiltCurve, point: CurvePoint) -> CurveContractResult<f64> {
    Ok(curve.query(&CurveQuery::DiscountFactor { point })?.value)
}
fn recipe() -> CurveContractResult<(CurveRecipe, Vec<Date>)> {
    let reference = date("2024-01-01")?;
    let dates = vec![date("2024-07-01")?, date("2025-01-01")?];
    let helpers = dates
        .iter()
        .enumerate()
        .map(|(i, &end)| {
            let tau = year_fraction(reference, end, Dc::Act360)?;
            let time = year_fraction(reference, end, Dc::Act365Fixed)?;
            Ok(MoneyMarketHelper {
                id: format!("deposit-{i}"),
                terms: MoneyMarketTerms::Deposit {
                    start_date: reference,
                    maturity_date: end,
                    day_count: Dc::Act360,
                },
                quote: MarketQuote {
                    value: (-0.02 * time).exp_m1() / tau * [1.0, 10_000.0][i],
                    unit: [QuoteUnit::DecimalRate, QuoteUnit::BasisPoints][i],
                },
                tolerance: [1e-10, 1e-6][i],
            })
        })
        .collect::<CurveContractResult<Vec<_>>>()?;
    Ok((
        CurveRecipe {
            key: CurveKey {
                id: "USD.analytic".into(),
                version: "supplied-v1".into(),
            },
            currency: Currency::USD,
            role: CurveRole::Discount {
                collateral_currency: Currency::USD,
            },
            reference_date: reference,
            time_basis: Dc::Act365Fixed,
            representation: CurveRepresentation::ZeroRates {
                compounding: Compounding::Continuous,
            },
            interpolation: Int::Linear,
            extrapolation: Ext::None,
            dependencies: vec![],
            source: CurveSource::MoneyMarket(helpers),
            calibration: CalibrationMode::Global(GlobalFitSettings {
                max_iterations: 100,
                objective_tolerance: 1e-11,
                initial_zeros: vec![0.03, 0.01],
                lower_zero: -0.05,
                upper_zero: 0.1,
                quote_scales: vec![1.0, 10_000.0],
                weights: vec![1.0, 2.0],
            }),
        },
        dates,
    ))
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (recipe, dates) = recipe()?;
    let output = fit_money_market_curve(&recipe, &dates, FitNumericalControls::default())?;
    require(output.solver.rank == Some(2), "unidentified fit")?;
    require(
        output
            .curve
            .diagnostics()
            .quotes
            .iter()
            .all(|q| q.residual.abs() <= q.tolerance),
        "native repricing failed",
    )?;
    let curves = CurveSet::new().with_curve(output.curve)?;
    let mut view = recipe.clone();
    view.key.id = "USD.daily-view".into();
    view.representation = CurveRepresentation::ZeroRates {
        compounding: Compounding::Daily,
    };
    view.dependencies = vec![recipe.key.clone()];
    view.calibration = CalibrationMode::Direct;
    view.source = CurveSource::DiscountEquivalent {
        base: recipe.key.clone(),
    };
    let mut roll = view.clone();
    roll.key.id = "USD.forward-roll".into();
    roll.reference_date = date("2024-03-01")?;
    roll.representation = CurveRepresentation::DiscountFactors;
    roll.source = CurveSource::Rolled {
        base: recipe.key.clone(),
        policy: RollPolicy::StickyForward,
    };
    let built = build_curve_set(&[view.clone(), roll.clone()], &curves)?;
    let original = built.curves.get(&recipe.key)?;
    let equivalent = built.curves.get(&view.key)?;
    let rolled = built.curves.get(&roll.key)?;
    let point = CurvePoint::Date(date("2024-09-01")?);
    let original_df = discount(original, point)?;
    require(
        original_df > 1.0,
        "negative-rate discount must remain valid",
    )?;
    require(
        discount(equivalent, point)? == original_df,
        "representation view changed discounts",
    )?;
    let rolled_df = discount(rolled, point)?;
    let expected = original_df / discount(original, CurvePoint::Date(roll.reference_date))?;
    require(
        (rolled_df - expected).abs() <= 1e-11 + 1e-11 * expected.abs(),
        "roll changed forwards",
    )?;
    let mut limited = recipe.clone();
    if let CalibrationMode::Global(settings) = &mut limited.calibration {
        settings.max_iterations = 1;
    }
    let failed = fit_money_market_curve(&limited, &dates, FitNumericalControls::default());
    require(
        matches!(failed, Err(CurveFitError::Solver(_))),
        "limited solve returned a curve",
    )?;
    let rank = output
        .solver
        .rank
        .ok_or_else(|| CurveContractError::Numerical("missing fitted rank".into()))?;
    println!("{{\"curves\":{},\"repriced_inputs\":2,\"rank\":{},\"iterations\":{},\"discount\":{},\"rolled_discount\":{},\"failure_checked\":true,\"certified\":false}}",
        built.curves.len(), rank, output.solver.iterations, original_df, rolled_df);
    Ok(())
}
