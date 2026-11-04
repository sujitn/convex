//! Test-only explicit context for the existing supplied USD fixture books.
use convex_core::{Currency, Date};
use convex_portfolio::stress::{
    run_stress_scenario, RateScenario, StressOrder, StressPolicy, StressRateBasis, StressScenario,
};
use convex_portfolio::{AnalyticsConfig, Holding, Portfolio};
use rust_decimal::prelude::ToPrimitive;

pub fn policy() -> StressPolicy {
    StressPolicy {
        rate_basis: StressRateBasis::ModifiedYield,
        rate_order: StressOrder::SecondOrder,
        key_rate_tenors: Vec::new(),
        fx: Vec::new(),
    }
}
pub fn parallel_shift_impact(
    holdings: &[Holding],
    shift: f64,
    _config: &AnalyticsConfig,
) -> Option<f64> {
    let portfolio = Portfolio {
        id: "original-supplied-test-book".into(),
        name: "supplied test inputs".into(),
        base_currency: Currency::USD,
        as_of_date: Date::from_ymd(2025, 1, 15).unwrap(),
        holdings: holdings.to_vec(),
        cash: Vec::new(),
        shares_outstanding: None,
        liabilities: None,
    };
    run_stress_scenario(
        &portfolio,
        &StressScenario::new("original parallel shift")
            .with_rate_scenario(RateScenario::parallel(shift)),
        &policy(),
    )
    .expect("all original supplied rate inputs must remain valid")
    .pnl_pct?
    .to_f64()
}
