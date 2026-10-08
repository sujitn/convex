//! A standalone consumer of existing public APIs and three observed gap controls.
//! Run with `cargo run --locked -p convex --example inventory_probe`.
//! This is reachability/invariant evidence, never an LB0/LB1 certificate.

#![forbid(unsafe_code)]
#![deny(
    unused_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use convex::{
    clean_price_from_yield, convexity, dirty_price_from_yield, dv01, modified_duration,
    yield_to_maturity, Bond, Compounding, Date, DayCountConvention, Deposit, DiscreteCurve,
    FixedRateBond, Frequency, GlobalFitter, InstrumentSet, InterpolationMethod, RateCurve,
    ValueType,
};
use convex_bonds::arrc::{compound_in_arrears, ArrcConfig};
use convex_bonds::cashflows::{Schedule, ScheduleConfig};
use convex_bonds::fixings::OvernightFixings;
use convex_bonds::instruments::CallableFloatingRateNote;
use convex_bonds::types::{CallSchedule, CallType};
use convex_core::calendars::WeekendCalendar;
use convex_curves::calibration::PiecewiseBootstrapper;
use convex_curves::TermStructure;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn close(actual: f64, expected: f64, tolerance: f64) -> Result<()> {
    if !actual.is_finite() || (actual - expected).abs() > tolerance {
        return Err(
            format!("invariant failed: {actual} versus {expected}, bound {tolerance}").into(),
        );
    }
    Ok(())
}

fn main() -> Result<()> {
    let issue = Date::parse("2024-01-15")?;
    let settlement = Date::parse("2025-01-15")?;
    let maturity = Date::parse("2030-01-15")?;
    let bond = FixedRateBond::builder()
        .cusip_unchecked("CVX053001")
        .coupon_rate("0.05".parse()?)
        .issue_date(issue)
        .maturity(maturity)
        .frequency(Frequency::SemiAnnual)
        .day_count(DayCountConvention::Thirty360US)
        .build()?;
    let schedule = Schedule::generate(ScheduleConfig::new(issue, maturity, Frequency::SemiAnnual))?;
    let flows = bond.cash_flows(settlement);
    if schedule.is_empty() || flows.is_empty() {
        return Err("expected supplied future schedule/cashflows".into());
    }
    let clean = clean_price_from_yield(&bond, settlement, 0.045, Frequency::SemiAnnual)?;
    let dirty = dirty_price_from_yield(&bond, settlement, 0.045, Frequency::SemiAnnual)?;
    let accrued = bond
        .accrued_interest(settlement)
        .to_string()
        .parse::<f64>()?;
    close(dirty - clean, accrued, 1e-10)?;
    let ytm = yield_to_maturity(
        &bond,
        settlement,
        clean.to_string().parse()?,
        Frequency::SemiAnnual,
    )?;
    close(ytm.yield_value, 0.045, 1e-9)?;
    let duration = modified_duration(&bond, settlement, ytm.yield_value, Frequency::SemiAnnual)?;
    let convexity_value = convexity(&bond, settlement, ytm.yield_value, Frequency::SemiAnnual)?;
    let dv01_value = dv01(
        &bond,
        settlement,
        ytm.yield_value,
        dirty,
        Frequency::SemiAnnual,
    )?;
    if !duration.is_finite() || !convexity_value.is_finite() || !dv01_value.is_finite() {
        return Err("nonfinite bond risk".into());
    }
    let curve = RateCurve::new(DiscreteCurve::new(
        settlement,
        vec![0.0, 10.0],
        vec![0.04, 0.04],
        ValueType::ZeroRate {
            compounding: Compounding::Continuous,
            day_count: DayCountConvention::Act365Fixed,
        },
        InterpolationMethod::Linear,
    )?);
    close(curve.discount_factor_at_tenor(0.0)?, 1.0, 1e-14)?;
    close(
        curve.discount_factor_at_tenor(2.0)?,
        (-0.08_f64).exp(),
        1e-14,
    )?;
    close(
        curve.forward_rate_at_tenors(1.0, 2.0, Compounding::Continuous)?,
        0.04,
        1e-14,
    )?;
    let helpers = InstrumentSet::new()
        .with(Deposit::new(
            settlement,
            settlement.add_months(6)?,
            0.04,
            DayCountConvention::Act360,
        ))
        .with(Deposit::new(
            settlement,
            settlement.add_months(12)?,
            0.045,
            DayCountConvention::Act360,
        ));
    let global = GlobalFitter::new().fit(settlement, &helpers)?;
    let piecewise = PiecewiseBootstrapper::new().bootstrap(settlement, &helpers)?;
    close(global.max_error(), 0.0, 1e-8)?;
    close(piecewise.max_error(), 0.0, 1e-10)?;
    let invalid_date_rejected = Date::parse("2025-02-30").is_err();
    let empty_helpers_rejected = GlobalFitter::new()
        .fit(settlement, &InstrumentSet::new())
        .is_err();
    let duplicate_nodes_rejected = DiscreteCurve::new(
        settlement,
        vec![1.0, 1.0],
        vec![0.9, 0.8],
        ValueType::DiscountFactor,
        InterpolationMethod::Linear,
    )
    .is_err();
    if !invalid_date_rejected || !empty_helpers_rejected || !duplicate_nodes_rejected {
        return Err("a negative construction control was accepted".into());
    }

    // Check the explicit right-owned step; historic fixing gaps remain separate.
    let hazard = DiscreteCurve::new(
        settlement,
        vec![1.0, 2.0],
        vec![0.01, 0.03],
        ValueType::HazardRate,
        InterpolationMethod::PiecewiseConstant,
    )?;
    let observed_hazard_midpoint = hazard.value_at(1.5);
    close(observed_hazard_midpoint, 0.01, 1e-14)?;
    let mut historic_forecasts = 0;
    let forecast_rate = "0.04".parse()?;
    let _coupon = compound_in_arrears(
        Date::parse("2025-01-06")?,
        Date::parse("2025-01-10")?,
        DayCountConvention::Act360,
        &WeekendCalendar,
        ArrcConfig {
            observation_shift: false,
            lookback_days: 0,
            lockout_days: 0,
        },
        &OvernightFixings::new().with_as_of(settlement),
        |_| {
            historic_forecasts += 1;
            forecast_rate
        },
    );
    if historic_forecasts != 4 {
        return Err("historical fixing fallback characterization changed; review inventory".into());
    }
    // Compile native composition separately from the absent wire/model workflow.
    let frn = convex::FloatingRateNote::builder()
        .index(convex_curves::multicurve::RateIndex::Sofr)
        .issue_date(issue)
        .maturity(maturity)
        .build()?;
    let _callable_frn = CallableFloatingRateNote::new(frn, CallSchedule::new(CallType::Bermudan));
    println!("{{\"status\":\"reachability_controls_passed\",\"certified\":false,\"units\":{{\"prices\":\"per_100_face\",\"yield\":\"decimal\",\"duration\":\"years\",\"convexity\":\"years_squared\",\"dv01\":\"existing_positive_per_100_face_per_bp\"}},\"clean\":{clean},\"dirty\":{dirty},\"accrued\":{accrued},\"yield\":{},\"duration\":{duration},\"convexity\":{convexity_value},\"dv01\":{dv01_value},\"cashflows\":{},\"schedule_periods\":{},\"global_max_residual\":{},\"piecewise_max_residual\":{},\"negative_controls_passed\":3,\"step_controls\":{{\"piecewise_constant_hazard_midpoint\":{observed_hazard_midpoint}}},\"observed_gaps\":{{\"missing_historic_fixings_forecast\":{historic_forecasts}}}}}", ytm.yield_value, flows.len(), schedule.num_periods(), global.max_error(), piecewise.max_error());
    Ok(())
}
