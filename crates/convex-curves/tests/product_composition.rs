//! Narrow immutable fitted representation and explicit overnight mapping gates.
use convex_core::{daycounts::DayCountConvention, types::Compounding, Currency, Date};
use convex_curves::{contracts::*, ExtrapolationMethod, InterpolationMethod};
fn d(s: &str) -> Date {
    Date::parse(s).unwrap()
}
fn key(id: &str) -> CurveKey {
    CurveKey {
        id: id.into(),
        version: "v1".into(),
    }
}
fn direct() -> CurveRecipe {
    CurveRecipe {
        key: key("DISCOUNT"),
        role: CurveRole::Discount {
            collateral_currency: Currency::USD,
        },
        currency: Currency::USD,
        reference_date: d("2024-01-15"),
        time_basis: DayCountConvention::Act365Fixed,
        representation: CurveRepresentation::ZeroRates {
            compounding: Compounding::Continuous,
        },
        interpolation: InterpolationMethod::Linear,
        extrapolation: ExtrapolationMethod::None,
        dependencies: vec![],
        calibration: CalibrationMode::Direct,
        source: CurveSource::Nodes(vec![
            CurveNode {
                pillar: CurvePoint::Date(d("2024-01-15")),
                quote: MarketQuote {
                    value: 0.04,
                    unit: QuoteUnit::DecimalRate,
                },
            },
            CurveNode {
                pillar: CurvePoint::Date(d("2025-01-15")),
                quote: MarketQuote {
                    value: 0.04,
                    unit: QuoteUnit::DecimalRate,
                },
            },
        ]),
    }
}
#[test]
fn explicit_same_currency_zero_basis_index_mapping_preserves_exact_discount_function() {
    let base = direct();
    let supplied = build_curve_set(std::slice::from_ref(&base), &CurveSet::new())
        .unwrap()
        .curves;
    let mut alias = base.clone();
    alias.key = key("ON");
    alias.role = CurveRole::Projection {
        index_id: "USD.ON".into(),
    };
    alias.dependencies = vec![base.key.clone()];
    alias.source = CurveSource::OvernightProjection {
        discount: base.key.clone(),
        index_id: "USD.ON".into(),
    };
    let built = build_curve_set(&[alias.clone()], &supplied).unwrap().curves;
    for query in [
        CurveQuery::DiscountFactor {
            point: CurvePoint::Date(d("2024-11-01")),
        },
        CurveQuery::ForwardRate {
            start: CurvePoint::Date(d("2024-04-05")),
            end: CurvePoint::Date(d("2024-04-09")),
            compounding: Compounding::Simple,
            day_count: DayCountConvention::Act360,
        },
    ] {
        assert_eq!(
            built.get(&base.key).unwrap().query(&query).unwrap().value,
            built.get(&alias.key).unwrap().query(&query).unwrap().value
        );
    }
    assert_eq!(
        built.get(&alias.key).unwrap().diagnostics().dependencies,
        vec![base.key]
    );
    for change in 0..4 {
        let mut bad = alias.clone();
        match change {
            0 => bad.currency = Currency::EUR,
            1 => {
                bad.role = CurveRole::Projection {
                    index_id: "OTHER".into(),
                }
            }
            2 => bad.reference_date = d("2024-01-16"),
            _ => bad.time_basis = DayCountConvention::Act360,
        };
        assert!(build_curve_set(&[bad], &supplied).is_err());
    }
    let mut wrong = alias;
    wrong.dependencies[0].version = "v2".into();
    if let CurveSource::OvernightProjection { discount, .. } = &mut wrong.source {
        discount.version = "v2".into();
    }
    assert!(matches!(
        build_curve_set(&[wrong], &supplied),
        Err(CurveContractError::MissingDependency(_))
    ));
}
#[test]
fn product_constructor_keeps_recipe_and_rejects_false_native_repricing_records() {
    let base = direct();
    let CurveSource::Nodes(nodes) = &base.source else {
        panic!("nodes")
    };
    let mut fitted = base.clone();
    fitted.source = CurveSource::Helpers(vec!["par-swap".into()]);
    fitted.calibration = CalibrationMode::Piecewise;
    let quote = QuoteRepricing {
        helper_id: "par-swap".into(),
        pillar: d("2025-01-15"),
        input: MarketQuote {
            value: 0.04,
            unit: QuoteUnit::DecimalRate,
        },
        model: MarketQuote {
            value: 0.04,
            unit: QuoteUnit::DecimalRate,
        },
        residual: 0.0,
        tolerance: 1e-12,
    };
    let built =
        build_fitted_rate_curve(&fitted, nodes, vec![quote.clone()], 4, &CurveSet::new()).unwrap();
    assert_eq!(built.recipe(), &fitted);
    assert_eq!(built.diagnostics().iterations, 4);
    assert_eq!(built.diagnostics().method, "product_piecewise_brent");
    for change in 0..5 {
        let mut bad = quote.clone();
        match change {
            0 => bad.helper_id = "OTHER".into(),
            1 => bad.residual = 1e-8,
            2 => {
                bad.model.value = 0.041;
                bad.residual = bad.model.value - bad.input.value;
            }
            3 => bad.tolerance = 0.0,
            _ => bad.model.unit = QuoteUnit::PercentRate,
        };
        assert!(build_fitted_rate_curve(&fitted, nodes, vec![bad], 4, &CurveSet::new()).is_err());
    }
    fitted.dependencies = vec![key("MISSING")];
    assert!(matches!(
        build_fitted_rate_curve(&fitted, nodes, vec![quote], 4, &CurveSet::new()),
        Err(CurveContractError::MissingDependency(_))
    ));
}
