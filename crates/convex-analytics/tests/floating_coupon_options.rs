//! Full original term/reset/profile/history inputs versus predeclared mathematics.
use convex_analytics::floating_coupon::*;
use convex_bonds::contracts::*;
use convex_core::{
    cashflows::CashflowError,
    contracts::{IndexId, UtcTimestamp},
    daycounts::DayCountConvention as Dc,
    fixings::FixingSeries,
    types::{Compounding, Currency},
    Date,
};
use convex_curves::{contracts::*, ExtrapolationMethod as Ex, InterpolationMethod as Int};
use rust_decimal::Decimal;
use serde_json::Value;
use std::cell::Cell;

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/term-coupon-options.json")).unwrap()
}
fn date(s: &str) -> Date {
    Date::parse(s).unwrap()
}
fn curve(reference: Date) -> BuiltCurve {
    let key = CurveKey {
        id: "declared-discount".into(),
        version: "v1".into(),
    };
    let recipe = CurveRecipe {
        key: key.clone(),
        role: CurveRole::Discount {
            collateral_currency: Currency::USD,
        },
        currency: Currency::USD,
        reference_date: reference,
        time_basis: Dc::Act365Fixed,
        representation: CurveRepresentation::ZeroRates {
            compounding: Compounding::Continuous,
        },
        interpolation: Int::Linear,
        extrapolation: Ex::Flat,
        dependencies: vec![],
        calibration: CalibrationMode::Direct,
        source: CurveSource::Nodes(
            [0.01, 30.0]
                .into_iter()
                .map(|t| CurveNode {
                    pillar: CurvePoint::Tenor(t),
                    quote: MarketQuote {
                        value: 0.03,
                        unit: QuoteUnit::DecimalRate,
                    },
                })
                .collect(),
        ),
    };
    build_curve_set(&[recipe], &CurveSet::new())
        .unwrap()
        .curves
        .get(&key)
        .unwrap()
        .as_ref()
        .clone()
}
fn publication(_: &IndexId, day: Date) -> Result<UtcTimestamp, CashflowError> {
    Ok(UtcTimestamp::parse(&format!("{day}T16:00:00Z")).unwrap())
}

#[test]
fn full_native_steps_fixings_cash_and_original_face_quotes_match_independent_references() {
    let f = fixture();
    for row in f["cases"].as_array().unwrap() {
        let terms: BondTerms = serde_json::from_value(row["terms"].clone()).unwrap();
        let bond = terms.build().unwrap();
        let context: BondProjectionContext =
            serde_json::from_value(row["context"].clone()).unwrap();
        let history =
            FixingSeries::new(serde_json::from_value(row["history"].clone()).unwrap()).unwrap();
        let options: TermCouponOptionInputs =
            serde_json::from_value(row["options"].clone()).unwrap();
        let discount = curve(context.settlement);
        let forecasts = Cell::new(0usize);
        let result = price_term_coupon_options(
            TermCouponPricing {
                bond: &bond,
                context: &context,
                history: &history,
                discount: &discount,
                options: &options,
            },
            publication,
            |_| {
                forecasts.set(forecasts.get() + 1);
                Ok(Decimal::new(4, 2))
            },
        )
        .unwrap();
        assert_eq!(
            forecasts.get(),
            if row["id"] == "known_exact_rounding" {
                0
            } else {
                2
            }
        );
        assert_eq!(result.segments.len(), 6);
        for (actual, expected) in result
            .segments
            .iter()
            .zip(row["declared_segments"].as_array().unwrap())
        {
            assert_eq!(actual.start, date(expected["start"].as_str().unwrap()));
            assert_eq!(actual.end, date(expected["end"].as_str().unwrap()));
            assert_eq!(
                actual.request.observation_date,
                date(expected["observation_date"].as_str().unwrap())
            );
            assert_eq!(actual.payment_date, date("2024-04-01"));
            assert_eq!(
                actual.principal,
                expected["principal"].as_str().unwrap().parse().unwrap()
            );
            assert!(
                (actual.expected_cash - expected["expected_cash"].as_f64().unwrap()).abs() <= 1e-9
            );
        }
        for (name, actual) in [
            ("present_value", result.present_value),
            ("dirty_price", result.dirty_price),
            ("clean_price", result.clean_price),
            ("accrued_model", result.accrued_model),
        ] {
            assert!(
                (actual - row["expected"][name].as_f64().unwrap()).abs()
                    <= if name == "accrued_model" { 1e-12 } else { 1e-9 },
                "{} {name}: {actual}",
                row["id"]
            );
        }
        assert_eq!(serde_json::to_value(&result.terms).unwrap(), row["terms"]);
        assert!(result.segments[0].model.is_none());
        assert_eq!(
            result.segments[1].model.is_some(),
            row["id"] != "known_exact_rounding"
        );
    }
}

#[test]
fn all_missing_published_history_is_rejected_before_forecast() {
    let row = &fixture()["cases"][0];
    let terms: BondTerms = serde_json::from_value(row["terms"].clone()).unwrap();
    let bond = terms.build().unwrap();
    let context: BondProjectionContext = serde_json::from_value(row["context"].clone()).unwrap();
    let options: TermCouponOptionInputs = serde_json::from_value(row["options"].clone()).unwrap();
    let discount = curve(context.settlement);
    let history = FixingSeries::new(vec![]).unwrap();
    let forecasts = Cell::new(0usize);
    let error = price_term_coupon_options(
        TermCouponPricing {
            bond: &bond,
            context: &context,
            history: &history,
            discount: &discount,
            options: &options,
        },
        publication,
        |_| {
            forecasts.set(forecasts.get() + 1);
            Ok(Decimal::new(4, 2))
        },
    )
    .unwrap_err();
    assert_eq!(forecasts.get(), 0);
    assert!(matches!(
        error,
        TermCouponOptionError::Cash(CashflowError::MissingFixings(_))
    ));
}

#[test]
fn missing_duplicate_future_information_and_unused_model_points_fail() {
    let row = &fixture()["cases"][0];
    let terms: BondTerms = serde_json::from_value(row["terms"].clone()).unwrap();
    let bond = terms.build().unwrap();
    let context: BondProjectionContext = serde_json::from_value(row["context"].clone()).unwrap();
    let original: TermCouponOptionInputs = serde_json::from_value(row["options"].clone()).unwrap();
    let history =
        FixingSeries::new(serde_json::from_value(row["history"].clone()).unwrap()).unwrap();
    let discount = curve(context.settlement);
    for variant in 0..4 {
        let mut options = original.clone();
        match variant {
            0 => {
                options.models.pop();
            }
            1 => options.models.push(options.models[0].clone()),
            2 => options.models[0].known_at = UtcTimestamp::parse("2024-01-16T18:00:00Z").unwrap(),
            _ => {
                options.models[0].observation_date = date("2024-01-01");
            }
        }
        assert!(price_term_coupon_options(
            TermCouponPricing {
                bond: &bond,
                context: &context,
                history: &history,
                discount: &discount,
                options: &options
            },
            publication,
            |_| Ok(Decimal::new(4, 2))
        )
        .is_err());
    }
}
