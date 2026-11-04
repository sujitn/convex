//! CVX-056 standalone consumer of pure construction/query/dependency APIs.
#![forbid(unsafe_code)]
#![deny(
    unused_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use convex::{
    curve_contracts::*, curve_inputs::*, fixings::FixingSeries, Compounding, Currency, Date,
    DayCountConvention as Dc, InterpolationMethod as Int,
};

fn date(value: &str) -> CurveContractResult<Date> {
    Date::parse(value).map_err(|error| CurveContractError::InvalidInput {
        field: "example.date".into(),
        reason: error.to_string(),
    })
}
fn check(condition: bool, reason: &str) -> CurveContractResult<()> {
    if !condition {
        return Err(CurveContractError::InvalidInput {
            field: "example.postcondition".into(),
            reason: reason.into(),
        });
    }
    Ok(())
}
fn main() -> CurveContractResult<()> {
    let reference = date("2024-01-01")?;
    let end1 = date("2024-04-01")?;
    let end2 = date("2024-07-01")?;
    let key = CurveKey {
        id: "USD.supplied".into(),
        version: "market-v1".into(),
    };
    let helpers = vec![
        CurveHelperInput {
            id: "deposit".into(),
            currency: Currency::USD,
            terms: CurveHelperTerms::MoneyMarket(MoneyMarketTerms::Deposit {
                start_date: reference,
                maturity_date: end1,
                day_count: Dc::Act360,
            }),
            quote: MarketQuote {
                value: 5.0,
                unit: QuoteUnit::PercentRate,
            },
            pillar: HelperPillar::Maturity,
            tolerance: 1e-9,
            weight: 1.0,
            residual_scale: 100.0,
        },
        CurveHelperInput {
            id: "fra".into(),
            currency: Currency::USD,
            terms: CurveHelperTerms::MoneyMarket(MoneyMarketTerms::Fra {
                index_id: "USD.TERM.3M".into(),
                fixing_date: date("2024-03-28")?,
                start_date: end1,
                end_date: end2,
                settlement_date: end1,
                settlement: FraSettlement::AtStart,
                day_count: Dc::Act360,
            }),
            quote: MarketQuote {
                value: 550.0,
                unit: QuoteUnit::BasisPoints,
            },
            pillar: HelperPillar::Maturity,
            tolerance: 1e-9,
            weight: 1.0,
            residual_scale: 10_000.0,
        },
    ];
    let recipe = CurveRecipe {
        key: key.clone(),
        role: CurveRole::Discount {
            collateral_currency: Currency::USD,
        },
        currency: Currency::USD,
        reference_date: reference,
        time_basis: Dc::Act365Fixed,
        representation: CurveRepresentation::DiscountFactors,
        interpolation: Int::LogLinear,
        extrapolation: convex::ExtrapolationMethod::None,
        dependencies: vec![],
        calibration: CalibrationMode::Piecewise,
        source: CurveSource::Helpers(helpers.iter().map(|h| h.id.clone()).collect()),
    };
    let fixings = FixingSeries::default();
    let resources = HelperResources {
        calendars: vec![],
        fixings: &fixings,
    };
    let output = build_helper_curve_set(
        &[HelperCurveRecipe {
            recipe: recipe.clone(),
            helpers,
        }],
        &resources,
        &CurveSet::new(),
    )?;
    let curve = output.curves.get(&key)?;
    let discount = curve.query(&CurveQuery::DiscountFactor {
        point: CurvePoint::Date(end2),
    })?;
    let forward = curve.query(&CurveQuery::ForwardRate {
        start: CurvePoint::Date(end1),
        end: CurvePoint::Date(end2),
        compounding: Compounding::Simple,
        day_count: Dc::Act360,
    })?;
    check(
        (forward.value - 0.055).abs() < 1e-11,
        "FRA native rate does not reprice",
    )?;
    check(
        curve.diagnostics().quotes.len() == 2,
        "all original native quotes required",
    )?;
    check(
        curve.tenor_to_date(curve.date_to_tenor(end2)?, TenorRounding::Nearest)? == end2,
        "date roundtrip",
    )?;
    check(
        curve
            .query(&CurveQuery::DiscountFactor {
                point: CurvePoint::Tenor(1.0),
            })
            .is_err(),
        "disabled extrapolation must fail",
    )?;

    let mut spread = recipe;
    spread.key = CurveKey {
        id: "USD.spread".into(),
        version: "basis-v1".into(),
    };
    spread.representation = CurveRepresentation::ZeroRates {
        compounding: Compounding::Continuous,
    };
    spread.calibration = CalibrationMode::Direct;
    spread.dependencies = vec![key.clone()];
    spread.source = CurveSource::ContinuousZeroSpread {
        base: key,
        spread: MarketQuote {
            value: 100.0,
            unit: QuoteUnit::BasisPoints,
        },
    };
    let spread_key = spread.key.clone();
    let with_spread = build_curve_set(&[spread], &output.curves)?;
    let spread_discount =
        with_spread
            .curves
            .get(&spread_key)?
            .query(&CurveQuery::DiscountFactor {
                point: CurvePoint::Date(end2),
            })?;
    check(
        spread_discount.value < discount.value,
        "positive continuous zero spread reduces discount factor",
    )?;

    let survival_key = CurveKey {
        id: "issuer.survival".into(),
        version: "supplied-survival-v1".into(),
    };
    let survival = CurveRecipe {
        key: survival_key.clone(),
        role: CurveRole::Credit {
            entity_id: "test.entity".into(),
            seniority: "senior".into(),
            recovery: 0.4,
        },
        currency: Currency::USD,
        reference_date: reference,
        time_basis: Dc::Act365Fixed,
        representation: CurveRepresentation::SurvivalProbabilities,
        interpolation: Int::LogLinear,
        extrapolation: convex::ExtrapolationMethod::None,
        dependencies: vec![],
        calibration: CalibrationMode::Direct,
        source: CurveSource::Nodes(vec![
            CurveNode {
                pillar: CurvePoint::Tenor(0.0),
                quote: MarketQuote {
                    value: 1.0,
                    unit: QuoteUnit::SurvivalProbability,
                },
            },
            CurveNode {
                pillar: CurvePoint::Tenor(5.0),
                quote: MarketQuote {
                    value: (-0.02_f64 * 5.0).exp(),
                    unit: QuoteUnit::SurvivalProbability,
                },
            },
        ]),
    };
    let complete = build_curve_set(&[survival], &with_spread.curves)?;
    let hazard = complete
        .curves
        .get(&survival_key)?
        .query(&CurveQuery::HazardRate {
            point: CurvePoint::Tenor(2.0),
            side: HazardSide::Right,
        })?;
    check(
        (hazard.value - 0.02).abs() < 1e-11,
        "analytic survival intensity",
    )?;
    check(
        curve_capabilities()
            .iter()
            .any(|c| c.status == CurveCapabilityStatus::DescribedUnsupported),
        "pending models must remain discoverable",
    )?;
    println!(
        r#"{{"curves":{},"repriced_inputs":2,"discount":{},"forward_decimal":{},"spread_discount":{},"hazard_per_year":{},"certified":false}}"#,
        complete.curves.len(),
        discount.value,
        forward.value,
        spread_discount.value,
        hazard.value
    );
    Ok(())
}
