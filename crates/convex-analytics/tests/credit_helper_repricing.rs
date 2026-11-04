//! Actual original ISDA controls and full native helper diagnostics.
mod credit_support;
use convex_analytics::credit_calibration::*;
use convex_core::contracts::Version;
use convex_credit::CdsSide;
use convex_curves::contracts::*;
use credit_support::*;
use std::error::Error;

fn original_curve(r: &CdsCurveRecipe, supplied: &CurveSet) -> Result<CurveSet, Box<dyn Error>> {
    let reference: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/comparisons/credit-isda-1.8.3/isda-reference.json"
    ))?;
    let values = reference["experiments"][0]["survival_probabilities"]
        .as_array()
        .ok_or("missing original survival")?;
    let mut curve = r.curve.clone();
    curve.source = CurveSource::Nodes(
        std::iter::once(CurveNode {
            pillar: CurvePoint::Date(curve.reference_date),
            quote: MarketQuote {
                value: 1.0,
                unit: QuoteUnit::SurvivalProbability,
            },
        })
        .chain(r.pillars.iter().zip(values).map(|(&date, v)| CurveNode {
            pillar: CurvePoint::Date(date),
            quote: MarketQuote {
                value: v.as_f64().unwrap(),
                unit: QuoteUnit::SurvivalProbability,
            },
        }))
        .collect(),
    );
    curve.calibration = CalibrationMode::Direct;
    curve.dependencies.clear();
    Ok(build_curve_set(&[curve], supplied)?.curves)
}

#[test]
fn actual_original_isda_quotes_and_full_valuations_reprice_without_fitting(
) -> Result<(), Box<dyn Error>> {
    let data = inputs()?;
    let r = mixed(&recipe(&data)?)?;
    let cal = calendar(&data);
    let market = original_curve(&r, &supplied(&data)?)?;
    let before = market.get(&r.curve.key)?.interpolation_grid()?.1.to_vec();
    let reference: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/comparisons/credit-isda-1.8.3/isda-reference.json"
    ))?;
    for r in [r.clone(), global(&r)] {
        let bindings = calendars(&r, &cal);
        let out = reprice_cds_helpers(
            &CdsCurveBuildInput {
                recipe: &r,
                calendars: &bindings,
            },
            &market,
        )?;
        assert_eq!(out.recipe, r);
        assert_eq!(out.price_evaluations, 6);
        assert_eq!(out.valuations.len(), 6);
        assert!(out.within_tolerance);
        for (i, (h, q)) in r.helpers.iter().zip(&out.quotes).enumerate() {
            assert_eq!(q.helper_id, h.id);
            assert_eq!(q.input, h.quote);
            assert_eq!(q.model.unit, h.quote.unit);
            assert_eq!(q.tolerance, h.tolerance);
            assert!(q.residual.abs() <= h.tolerance);
            let coupon = h.terms.coupon_decimal.to_model()?.get();
            let row = reference["experiments"][0]["valuations"]
                .as_array()
                .ok_or("missing original valuations")?
                .iter()
                .find(|v| v["pillar_index"] == i && v["coupon_decimal"].as_f64() == Some(coupon))
                .ok_or("missing original full valuation")?;
            let v = &out.valuations[i];
            assert!(
                (v.clean_upfront_fraction - row["clean_upfront_fraction"].as_f64().unwrap()).abs()
                    <= data.bounds["upfront_fraction_absolute"].as_f64().unwrap()
            );
            assert!(
                (v.dirty_upfront_fraction - row["dirty_upfront_fraction"].as_f64().unwrap()).abs()
                    <= data.bounds["upfront_fraction_absolute"].as_f64().unwrap()
            );
            assert!(
                (v.net_pv - row["net_pv"].as_f64().unwrap()).abs()
                    <= data.bounds["pv_currency_absolute"].as_f64().unwrap()
            );
        }
    }
    assert_eq!(market.get(&r.curve.key)?.interpolation_grid()?.1, before);
    Ok(())
}

#[test]
fn full_original_guards_return_no_partial_diagnostics() -> Result<(), Box<dyn Error>> {
    let data = inputs()?;
    let r = mixed(&recipe(&data)?)?;
    let cal = calendar(&data);
    let market = original_curve(&r, &supplied(&data)?)?;
    let original = calendars(&r, &cal);
    for change in 0..9 {
        let mut bad = r.clone();
        match change {
            0 => bad.helpers[5].quote.unit = QuoteUnit::BasisPoints,
            1 => bad.helpers[5].terms.premium_calendar.version = Version::new(2)?,
            2 => bad.helpers[5].tolerance = 0.0,
            3 => bad.helpers[5].quote.value = f64::NAN,
            4 => bad.helpers[5].terms.reference.document.version = Version::new(2)?,
            5 => bad.curve.dependencies.clear(),
            6 => bad.curve.key.version = "missing".into(),
            7 => bad.curve.extrapolation = convex_curves::ExtrapolationMethod::Flat,
            _ => bad.helpers[5].context.valuation_date = convex_core::Date::parse("2022-06-20")?,
        }
        assert!(
            reprice_cds_helpers(
                &CdsCurveBuildInput {
                    recipe: &bad,
                    calendars: &original
                },
                &market
            )
            .is_err(),
            "mutation {change}"
        );
    }
    assert!(reprice_cds_helpers(
        &CdsCurveBuildInput {
            recipe: &r,
            calendars: &original[..5]
        },
        &market
    )
    .is_err());
    assert!(reprice_cds_helpers(
        &CdsCurveBuildInput {
            recipe: &r,
            calendars: &original
        },
        &supplied(&data)?
    )
    .is_err());
    // The existing builder still rejects a supplied immutable output identity.
    assert!(build_credit_curve_set(
        &[CdsCurveBuildInput {
            recipe: &r,
            calendars: &original
        }],
        &market,
        FitNumericalControls::default()
    )
    .is_err());
    assert_eq!(market.len(), 2);
    Ok(())
}

#[test]
fn off_market_and_signed_quotes_remain_diagnostics_and_fitted_curves_are_reusable(
) -> Result<(), Box<dyn Error>> {
    let data = inputs()?;
    let r = mixed(&recipe(&data)?)?;
    let cal = calendar(&data);
    let original = calendars(&r, &cal);
    let fitted = build_credit_curve_set(
        &[CdsCurveBuildInput {
            recipe: &r,
            calendars: &original,
        }],
        &supplied(&data)?,
        FitNumericalControls::default(),
    )?;
    let base = reprice_cds_helpers(
        &CdsCurveBuildInput {
            recipe: &r,
            calendars: &original,
        },
        &fitted.curves,
    )?;
    assert!(base.within_tolerance);
    let mut off = r.clone();
    off.helpers[0].quote.value += 1.0;
    let bindings = calendars(&off, &cal);
    let result = reprice_cds_helpers(
        &CdsCurveBuildInput {
            recipe: &off,
            calendars: &bindings,
        },
        &fitted.curves,
    )?;
    assert!(!result.within_tolerance);
    assert_eq!(result.price_evaluations, 6);
    assert_eq!(result.quotes[0].model, base.quotes[0].model);
    let mut seller = r.clone();
    for h in &mut seller.helpers {
        h.terms.side = CdsSide::ProtectionSeller;
    }
    let bindings = calendars(&seller, &cal);
    let result = reprice_cds_helpers(
        &CdsCurveBuildInput {
            recipe: &seller,
            calendars: &bindings,
        },
        &fitted.curves,
    )?;
    assert_eq!(result.quotes, base.quotes);
    for (a, b) in result.valuations.iter().zip(&base.valuations) {
        assert_eq!(a.net_pv, -b.net_pv);
    }
    Ok(())
}
