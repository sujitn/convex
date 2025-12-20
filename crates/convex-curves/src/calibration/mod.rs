//! Curve calibration engine.
//!
//! This module provides infrastructure for calibrating curves from
//! market instruments using global optimization.
//!
//! # Approach
//!
//! Unlike sequential bootstrapping, we use a global solver that:
//! - Fits all instruments simultaneously
//! - Handles interdependencies between instruments
//! - Provides better fit quality and stability
//!
//! # Supported Instruments
//!
//! - [`Deposit`]: Money market deposit rates
//! - [`Fra`]: Forward rate agreements
//! - [`Future`]: Interest rate futures (with convexity adjustment)
//! - [`Swap`]: Fixed-for-floating interest rate swaps
//! - [`Ois`]: Overnight index swaps
//!
//! # Example
//!
//! ```rust,ignore
//! use convex_curves::calibration::{GlobalFitter, Deposit, Swap, InstrumentSet};
//! use convex_core::daycounts::DayCountConvention;
//! use convex_core::types::{Date, Frequency};
//!
//! let today = Date::from_ymd(2024, 1, 2).unwrap();
//! let dc = DayCountConvention::Act360;
//!
//! // Build instrument set
//! let instruments = InstrumentSet::new()
//!     .with(Deposit::from_tenor(today, 0.25, 0.04, dc))
//!     .with(Deposit::from_tenor(today, 0.5, 0.042, dc))
//!     .with(Swap::from_tenor(today, 2.0, 0.045, Frequency::SemiAnnual, DayCountConvention::Thirty360US))
//!     .with(Swap::from_tenor(today, 5.0, 0.048, Frequency::SemiAnnual, DayCountConvention::Thirty360US));
//!
//! // Calibrate
//! let fitter = GlobalFitter::new();
//! let result = fitter.fit(today, &instruments).unwrap();
//!
//! println!("{}", result.summary());
//! println!("5Y zero rate: {:.4}%", result.curve.value_at(5.0) * 100.0);
//! ```

mod global_fit;
mod instruments;

pub use global_fit::{CalibrationResult, FitterConfig, GlobalFitter, SequentialBootstrapper};
pub use instruments::{
    CalibrationInstrument, Deposit, Fra, Future, InstrumentSet, Ois, Swap,
};
