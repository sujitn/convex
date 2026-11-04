//! Independent mathematical option references before bond integration.
use convex_bonds::options::{
    floating_coupon::{bounded_floating_coupon, forward_rate_option, ForwardCouponModel},
    hw1f::OptionSide,
};
use serde_json::Value;

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/floating-coupon-options.json")).unwrap()
}
fn close(value: f64, row: &Value, field: &str, bound: f64) {
    assert!(
        (value - row["expected"][field].as_f64().unwrap()).abs() <= bound,
        "{} {field}: {value:?}",
        row["id"]
    );
}

#[test]
fn independent_normal_and_shifted_lognormal_option_values_and_derivatives() {
    let f = fixture();
    for row in f["cases"].as_array().unwrap() {
        let model = serde_json::from_value(row["model"].clone()).unwrap();
        let side = serde_json::from_value(row["side"].clone()).unwrap();
        let r = forward_rate_option(
            model,
            row["expiry_time"].as_f64().unwrap(),
            row["forward"].as_f64().unwrap(),
            row["strike"].as_f64().unwrap(),
            side,
        )
        .unwrap();
        for (field, value) in [
            ("expected_payoff", r.expected_payoff),
            ("forward_delta", r.forward_delta.unwrap()),
            ("forward_gamma", r.forward_gamma.unwrap()),
            ("annual_vol_vega", r.annual_vol_vega.unwrap()),
        ] {
            close(
                value,
                row,
                field,
                f["absolute_bounds"][field].as_f64().unwrap(),
            );
        }
    }
}

#[test]
fn independent_caps_floors_collars_and_gearing_keep_true_time_value() {
    let f = fixture();
    for row in f["coupon_cases"].as_array().unwrap() {
        let model = serde_json::from_value(row["model"].clone()).unwrap();
        let r = bounded_floating_coupon(
            model,
            row["expiry_time"].as_f64().unwrap(),
            row["forward"].as_f64().unwrap(),
            row["gearing"].as_f64().unwrap(),
            row["additive"].as_f64().unwrap(),
            row["cap"].as_f64(),
            row["floor"].as_f64(),
        )
        .unwrap();
        for (field, value) in [
            ("expected_rate", r.expected_rate),
            ("cap_adjustment", r.cap_adjustment),
            ("floor_adjustment", r.floor_adjustment),
            ("forward_delta", r.forward_delta.unwrap()),
            ("forward_gamma", r.forward_gamma.unwrap()),
            ("annual_vol_vega", r.annual_vol_vega.unwrap()),
        ] {
            close(
                value,
                row,
                field,
                if field == "forward_gamma" {
                    1e-10
                } else {
                    1e-12
                },
            );
        }
    }
    let m = ForwardCouponModel::Normal { volatility: 0.0125 };
    let r = bounded_floating_coupon(m, 1.5, 0.04, 1.0, 0.0, Some(0.04), None).unwrap();
    assert!(r.expected_rate < 0.04 && r.cap_adjustment > 0.0);
}

#[test]
fn zero_variance_intrinsic_kinks_are_explicit_and_constant_collars_are_smooth() {
    for model in [
        ForwardCouponModel::Normal { volatility: 0.0 },
        ForwardCouponModel::ShiftedLognormal {
            volatility: 0.0,
            displacement: 0.03,
        },
    ] {
        let kink = forward_rate_option(model, 1.0, 0.04, 0.04, OptionSide::Call).unwrap();
        assert_eq!(kink.expected_payoff, 0.0);
        assert_eq!(kink.forward_delta, None);
        assert_eq!(kink.forward_gamma, None);
        assert_eq!(kink.annual_vol_vega, None);
        let r = forward_rate_option(model, 1.0, 0.04, 0.03, OptionSide::Call).unwrap();
        assert!((r.expected_payoff - 0.01).abs() < 1e-12);
        assert_eq!(r.forward_delta, Some(1.0));
        let c =
            bounded_floating_coupon(model, 1.0, 0.04, 1.0, 0.0, Some(0.04), Some(0.04)).unwrap();
        assert_eq!(c.expected_rate, 0.04);
        assert_eq!(c.forward_delta, Some(0.0));
        assert_eq!(c.forward_gamma, Some(0.0));
        assert_eq!(c.annual_vol_vega, Some(0.0));
    }
    let fixed = forward_rate_option(
        ForwardCouponModel::Normal { volatility: 0.01 },
        0.0,
        0.04,
        0.03,
        OptionSide::Put,
    )
    .unwrap();
    assert_eq!(fixed.expected_payoff, 0.0);
}

#[test]
fn put_call_parity_and_same_formula_bumps_preserve_derivatives() {
    for model in [
        ForwardCouponModel::Normal { volatility: 0.0125 },
        ForwardCouponModel::ShiftedLognormal {
            volatility: 0.25,
            displacement: 0.03,
        },
    ] {
        let f = 0.02;
        let k = 0.03;
        let t = 1.5;
        let h = 1e-6;
        let p = forward_rate_option(model, t, f, k, OptionSide::Put).unwrap();
        let c = forward_rate_option(model, t, f, k, OptionSide::Call).unwrap();
        assert!((c.expected_payoff - p.expected_payoff - (f - k)).abs() < 1e-12);
        let up = forward_rate_option(model, t, f + h, k, OptionSide::Call).unwrap();
        let down = forward_rate_option(model, t, f - h, k, OptionSide::Call).unwrap();
        assert!(
            ((up.expected_payoff - down.expected_payoff) / (2.0 * h) - c.forward_delta.unwrap())
                .abs()
                < 1e-7
        );
        assert!(
            ((up.forward_delta.unwrap() - down.forward_delta.unwrap()) / (2.0 * h)
                - c.forward_gamma.unwrap())
            .abs()
                < 1e-6
        );
    }
}

#[test]
fn invalid_original_models_inputs_and_unsupported_payoff_scales_fail() {
    let normal = ForwardCouponModel::Normal { volatility: 0.0125 };
    for vol in [f64::NAN, f64::INFINITY, -0.01, 1.01] {
        assert!(forward_rate_option(
            ForwardCouponModel::Normal { volatility: vol },
            1.0,
            0.02,
            0.03,
            OptionSide::Call
        )
        .is_err());
    }
    for t in [f64::NAN, f64::INFINITY, -0.1, 150.01] {
        assert!(forward_rate_option(normal, t, 0.02, 0.03, OptionSide::Call).is_err());
    }
    assert!(forward_rate_option(
        ForwardCouponModel::ShiftedLognormal {
            volatility: 0.25,
            displacement: 0.0
        },
        1.0,
        -0.01,
        0.03,
        OptionSide::Call
    )
    .is_err());
    assert!(bounded_floating_coupon(normal, 1.0, 0.02, 1.0, 0.0, Some(0.01), Some(0.02)).is_err());
    assert!(bounded_floating_coupon(normal, 1.0, 0.02, f64::NAN, 0.0, None, None).is_err());
    assert!(bounded_floating_coupon(normal, 1.0, 0.02, 1e-20, 0.0, Some(0.03), None).is_err());
    assert!(serde_json::from_str::<ForwardCouponModel>(
        r#"{"kind":"normal","volatility":0.01,"discarded":1}"#
    )
    .is_err());
}

#[test]
fn positive_variance_underflow_cannot_become_an_intrinsic_price() {
    let model = ForwardCouponModel::Normal {
        volatility: f64::from_bits(1),
    };
    let result = forward_rate_option(model, f64::from_bits(1), 0.04, 0.04, OptionSide::Call);
    assert!(result.is_err());
}
