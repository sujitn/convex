//! Partial holiday interest-weight prefixes over already resolved observations.
use convex_core::{
    calendars::Calendar,
    cashflows::{CashflowError, OvernightConvention, OvernightSchedule},
    contracts::{IndexId, UtcTimestamp},
    daycounts::DayCountConvention,
    fixings::{FixingContext, FixingSeries, UnpublishedFixingPolicy},
    Date,
};
use rust_decimal::{prelude::ToPrimitive, Decimal};
use rust_decimal_macros::dec;
fn d(s: &str) -> Date {
    Date::parse(s).unwrap()
}
struct Holiday;
impl Calendar for Holiday {
    fn name(&self) -> &'static str {
        "pinned holiday"
    }
    fn is_business_day(&self, at: Date) -> bool {
        !at.is_weekend() && at != d("2024-04-08")
    }
}
fn coupon(shift: bool) -> convex_core::cashflows::CompoundedCoupon {
    let schedule = OvernightSchedule::generate(
        d("2024-04-04"),
        d("2024-04-11"),
        DayCountConvention::Act360,
        &Holiday,
        OvernightConvention {
            lookback_business_days: if shift { 2 } else { 0 },
            observation_shift: shift,
            lockout_business_days: 0,
        },
    )
    .unwrap();
    schedule
        .compound(
            &IndexId::new("USD.ON").unwrap(),
            &FixingSeries::new(vec![]).unwrap(),
            FixingContext {
                valuation_date: d("2024-04-01"),
                known_at: UtcTimestamp::parse("2024-04-01T23:59:59Z").unwrap(),
                unpublished: UnpublishedFixingPolicy::Forecast,
                final_only: true,
            },
            |at| Ok(UtcTimestamp::parse(&format!("{at}T08:00:00Z")).unwrap()),
            |_, _| Ok(dec!(0.05)),
        )
        .unwrap()
}
#[test]
fn actual_interest_window_prefix_and_partial_holiday_days_compound_once() {
    for shift in [false, true] {
        let coupon = coupon(shift);
        let first = coupon.observations[0].dates.weight_start;
        assert_eq!(
            coupon
                .factor_through(first, DayCountConvention::Act360)
                .unwrap(),
            Decimal::ONE
        );
        assert_eq!(
            coupon
                .factor_through(d("2024-04-20"), DayCountConvention::Act360)
                .unwrap(),
            coupon.period_factor
        );
        let daily: f64 = 1.0 + 0.05 / 360.0;
        let partial = 1.0 + 0.05 * 2.0 / 360.0;
        let expected = if shift {
            daily.powi(3) * partial
        } else {
            daily * partial
        };
        let actual = coupon
            .factor_through(d("2024-04-07"), DayCountConvention::Act360)
            .unwrap()
            .to_f64()
            .unwrap();
        assert!((actual - expected).abs() < 1e-15);
    }
}
#[test]
fn invalid_mutated_prefix_inputs_fail_instead_of_returning_an_empty_factor() {
    let original = coupon(false);
    let mut broken = original.clone();
    broken.observations[1].dates.weight_start = d("2024-04-06");
    assert!(broken
        .factor_through(d("2024-04-01"), DayCountConvention::Act360)
        .is_err());
    let mut broken = original.clone();
    broken.observations[0].year_fraction = dec!(1);
    assert!(broken
        .factor_through(d("2024-04-07"), DayCountConvention::Act360)
        .is_err());
    assert!(original
        .factor_through(d("2024-04-07"), DayCountConvention::Thirty360E)
        .is_err());
    let mut broken = original;
    broken.observations.clear();
    assert!(matches!(
        broken.factor_through(d("2024-04-07"), DayCountConvention::Act360),
        Err(CashflowError::BoundExceeded(_))
    ));
}
