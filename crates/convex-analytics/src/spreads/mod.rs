//! Spread calculations for fixed income securities.
//!
//! This module provides various spread measures used in fixed income analysis:
//!
//! - **Z-Spread**: Zero-volatility spread over the spot curve
//! - **G-Spread**: Spread over government/treasury benchmark
//! - **I-Spread**: Spread over swap curve (interpolated)
//! - **OAS**: Option-adjusted spread for callable bonds
//! - **Discount Margin**: Spread for floating rate notes
//! - **ASW**: Asset swap spreads (par-par and proceeds)
//!
//! # Overview
//!
//! Each spread measure answers a different question:
//!
//! | Spread | Question | Use Case |
//! |--------|----------|----------|
//! | Z-Spread | What constant spread over spot rates prices the bond? | Relative value analysis |
//! | G-Spread | How much over government bonds? | Credit risk assessment |
//! | I-Spread | How much over swaps at maturity? | Quick benchmark |
//! | OAS | What spread after adjusting for embedded options? | Callable bond analysis |
//! | DM | What spread for floating rate notes? | FRN valuation |
//! | ASW | What spread in an asset swap package? | Swap-based hedging |

mod benchmark;
mod discount_margin;
mod government_curve;
mod gspread;
mod ispread;
mod oas;
mod sovereign;
mod zspread;

pub mod asw;

// Re-export main types and functions
pub use benchmark::{BenchmarkSpec, SecurityId};
pub use discount_margin::{simple_margin, z_discount_margin, DiscountMarginCalculator};
pub use government_curve::{GovernmentBenchmark, GovernmentCurve};
pub use gspread::{g_spread, g_spread_with_benchmark, GSpreadCalculator};
pub use ispread::{i_spread, ISpreadCalculator};
pub use oas::OASCalculator;
pub use sovereign::{Sovereign, SupranationalIssuer};
pub use zspread::{z_spread, z_spread_from_curve, ZSpreadCalculator};

// Re-export ASW types
pub use asw::{ASWType, ParParAssetSwap, ProceedsAssetSwap};
