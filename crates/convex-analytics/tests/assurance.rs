use convex_analytics::calibration::{calibrate_hw1f_sigma, CoterminalSwaptionHelper};
use convex_core::types::{Compounding, Date};
use convex_curves::{CurveError, CurveResult, RateCurveDyn};

struct UnavailableCurve;
impl RateCurveDyn for UnavailableCurve {
    fn discount_factor(&self, _: f64) -> CurveResult<f64> {
        Err(CurveError::invalid_value("missing discount"))
    }
    fn zero_rate(&self, _: f64, _: Compounding) -> CurveResult<f64> {
        Err(CurveError::invalid_value("missing zero"))
    }
    fn forward_rate(&self, _: f64, _: f64) -> CurveResult<f64> {
        Err(CurveError::invalid_value("missing forward"))
    }
    fn instantaneous_forward(&self, _: f64) -> CurveResult<f64> {
        Err(CurveError::invalid_value("missing forward"))
    }
    fn reference_date(&self) -> Date {
        Date::from_ymd(2025, 1, 1).unwrap()
    }
    fn max_date(&self) -> Date {
        Date::from_ymd(2030, 1, 1).unwrap()
    }
}

#[test]
fn missing_curve_does_not_calibrate_against_fabricated_rates() {
    let helpers = [CoterminalSwaptionHelper {
        expiry_years: 1.0,
        tail_years: 4.0,
        atm_normal_vol_bps: 100.0,
    }];
    assert!(calibrate_hw1f_sigma(&UnavailableCurve, 0.03, &helpers).is_err());
}

#[test]
fn invalid_helper_bounds_fail_before_curve_evaluation() {
    for (expiry, tail, vol) in [
        (f64::NAN, 4.0, 100.0),
        (1.0, 0.0, 100.0),
        (1.0, 4.0, 0.0),
        (1.0, 151.0, 100.0),
    ] {
        let helpers = [CoterminalSwaptionHelper {
            expiry_years: expiry,
            tail_years: tail,
            atm_normal_vol_bps: vol,
        }];
        let result = calibrate_hw1f_sigma(&UnavailableCurve, 0.03, &helpers);
        assert!(matches!(
            result,
            Err(convex_analytics::AnalyticsError::InvalidInput(_))
        ));
    }
}
