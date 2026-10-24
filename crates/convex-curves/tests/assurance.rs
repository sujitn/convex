//! Curve construction remains a standalone pure operation.
use convex_core::Date;
use convex_curves::calibration::PiecewiseBootstrapper;
use convex_curves::calibration::{
    CalibrationInstrument, FitterConfig, GlobalFitter, InstrumentSet,
};
use convex_curves::{CurveError, CurveResult, DiscreteCurve, RateCurve};

#[derive(Debug)]
struct FailedHelper {
    tenor: f64,
    nonfinite: bool,
}
impl CalibrationInstrument for FailedHelper {
    fn reference_date(&self) -> Date {
        Date::from_ymd(2026, 10, 4).unwrap()
    }
    fn maturity(&self) -> Date {
        Date::from_ymd(2027, 10, 4).unwrap()
    }
    fn tenor(&self) -> f64 {
        self.tenor
    }
    fn quote(&self) -> f64 {
        0.03
    }
    fn pv(&self, curve: &RateCurve<DiscreteCurve>) -> CurveResult<f64> {
        self.pricing_error(curve)
    }
    fn pricing_error(&self, _: &RateCurve<DiscreteCurve>) -> CurveResult<f64> {
        if self.nonfinite {
            Ok(f64::NAN)
        } else {
            Err(CurveError::invalid_value("helper unavailable"))
        }
    }
    fn dv01(&self, _: &RateCurve<DiscreteCurve>) -> CurveResult<f64> {
        Ok(0.0)
    }
    fn instrument_type(&self) -> &'static str {
        "fault fixture"
    }
}

fn helpers(tenor: f64, nonfinite: bool) -> InstrumentSet {
    let mut set = InstrumentSet::new();
    set.add(FailedHelper { tenor, nonfinite });
    set.add(FailedHelper {
        tenor: 2.0,
        nonfinite,
    });
    set
}

#[test]
fn helper_failure_and_nonfinite_residual_do_not_become_zero_error() {
    let date = Date::from_ymd(2026, 10, 4).unwrap();
    for nonfinite in [false, true] {
        assert!(GlobalFitter::default()
            .fit(date, &helpers(1.0, nonfinite))
            .is_err());
        assert!(PiecewiseBootstrapper::default()
            .bootstrap(date, &helpers(1.0, nonfinite))
            .is_err());
    }
}

#[test]
fn invalid_tenor_and_budget_fail_without_panicking() {
    let date = Date::from_ymd(2026, 10, 4).unwrap();
    assert!(GlobalFitter::default()
        .fit(date, &helpers(f64::NAN, false))
        .is_err());
    let config = FitterConfig::default().with_max_iterations(usize::MAX);
    assert!(GlobalFitter::with_config(config)
        .fit(date, &helpers(1.0, false))
        .is_err());
}

#[test]
fn curve_thread_safety_is_derived_from_fields() {
    fn requires_send_sync<T: Send + Sync>() {}
    requires_send_sync::<DiscreteCurve>();
    requires_send_sync::<convex_curves::curves::SegmentedCurve>();
}
