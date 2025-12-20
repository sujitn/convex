//! Domain-specific curve wrappers.
//!
//! These wrappers provide semantic operations on top of any `TermStructure`:
//!
//! - [`RateCurve`]: Interest rate operations (discount, zero, forward)
//! - [`CreditCurve`]: Credit operations (survival, hazard, spread)
//! - [`InflationCurve`]: Inflation operations (index ratio, real rate)
//! - [`FxCurve`]: FX operations (forward rate, forward points)

mod rate_curve;
mod credit_curve;

pub use rate_curve::RateCurve;
pub use credit_curve::CreditCurve;

// TODO: Implement these wrappers
// mod inflation_curve;
// mod fx_curve;
// pub use inflation_curve::InflationCurve;
// pub use fx_curve::FxCurve;
