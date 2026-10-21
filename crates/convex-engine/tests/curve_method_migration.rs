#[path = "../../convex-ports/tests/support/snapshot_fixtures.rs"]
mod fixture;

use convex_curves::{ExtrapolationMethod, RateCurveDyn};
use convex_engine::curve_builder::BuiltCurve;
use convex_ports::{snapshot::ZeroExtrapolation, TraitError};

#[test]
fn snapshot_tail_policy_is_retained_or_rejected_without_relabeling() {
    let mut record = fixture::curve();
    assert_eq!(record.extrapolation, ZeroExtrapolation::FlatZero);
    let reconstructed = BuiltCurve::from_snapshot(&record).unwrap();
    assert_eq!(reconstructed.extrapolation, ExtrapolationMethod::Flat);
    assert_eq!(reconstructed.discount_factor(0.0).unwrap(), 1.0);
    record.extrapolation = ZeroExtrapolation::FlatForward;
    assert!(matches!(
        BuiltCurve::from_snapshot(&record),
        Err(TraitError::InvalidInput(message)) if message.contains("complete curve-method contract")
    ));
}
