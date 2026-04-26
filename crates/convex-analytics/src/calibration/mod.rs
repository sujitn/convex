//! Short-rate model calibration.
//!
//! Currently supports HW1F constant-`σ` calibration against an ATM
//! co-terminal swaption strip (a fixed exogenously). See
//! [`hw1f`] for details.

pub mod hw1f;

pub use hw1f::{calibrate_hw1f_sigma, CoterminalSwaptionHelper, Hw1fCalibration};
