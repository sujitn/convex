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
    serde_json::from_str(include_str!("fixtures/whole-coupon-normal.json")).unwrap()
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
fn whole_coupon_joint_normal_keeps_full_steps_and_shared_factor_covariance() {
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
            if row["id"] == "whole_coupon_known_rounding" {
                0
            } else {
                2
            }
        );
        assert_eq!(result.segments.len(), 5);
        assert!(
            (result.whole_coupons[0].expected_cash
                - result.segments.iter().map(|x| x.expected_cash).sum::<f64>())
            .abs()
                > 1e-6
                || row["id"] == "whole_coupon_zero_uncertainty"
        );
        assert_eq!(result.whole_coupons.len(), 1);
        let whole = &result.whole_coupons[0];
        assert_eq!(whole.start, date("2024-01-01"));
        assert_eq!(whole.end, date("2024-04-01"));
        assert_eq!(whole.payment_date, date("2024-04-01"));
        for (name, actual) in [
            ("unbounded_cash_mean", whole.unbounded_cash_mean),
            ("cash_standard_deviation", whole.cash_standard_deviation),
            ("expected_cash", whole.expected_cash),
            ("present_value", result.present_value),
            ("dirty_price", result.dirty_price),
            ("clean_price", result.clean_price),
            ("accrued_model", result.accrued_model),
        ] {
            assert!(
                (actual - row["expected"][name].as_f64().unwrap()).abs()
                    <= if name == "accrued_model" { 1e-12 } else { 1e-9 },
                "{name}: {actual}"
            );
        }
        for (actual, expected) in whole
            .cash_factor_loadings
            .iter()
            .zip(row["expected"]["cash_factor_loadings"].as_array().unwrap())
        {
            assert!((actual - expected.as_f64().unwrap()).abs() <= 1e-9);
        }
        assert_eq!(serde_json::to_value(&result.terms).unwrap(), row["terms"]);
        assert!(result.segments[0].normal_factors.is_none());
        assert_eq!(
            result.segments[1].normal_factors.is_some(),
            row["id"] != "whole_coupon_known_rounding"
        );
    }
}
