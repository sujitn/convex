//! WebAssembly bindings for Convex fixed income analytics.
//!
//! This crate provides WASM bindings for the Convex library, enabling
//! Bloomberg YAS-equivalent bond analytics in web browsers. The public
//! `#[wasm_bindgen]` surface is split across submodules by responsibility:
//!
//! - [`analyze`] — `analyze_bond`, `get_cash_flows`, `calculate_accrued`,
//!   `calculate_simple_metrics`
//! - [`pricing`] — `price_from_yield`, `price_from_spread`,
//!   `price_from_g_spread`, `price_from_benchmark_spread`
//! - [`conventions`] — `get_convention_options`, `get_default_conventions`
//!
//! The non-public modules ([`dto`], [`convert`], [`bond`]) hold the wire
//! types, parser/formatter helpers, and shared bond/curve construction.

use wasm_bindgen::prelude::*;

mod analyze;
mod bond;
mod conventions;
mod convert;
mod dto;
mod pricing;

pub use analyze::{analyze_bond, calculate_accrued, calculate_simple_metrics, get_cash_flows};
pub use conventions::{get_convention_options, get_default_conventions};
pub use dto::{
    AnalysisResult, BondParams, CallScheduleEntry, CashFlowEntry, ConventionOption,
    ConventionOptions, CurvePoint, DefaultConventions, PriceFromYieldResult,
};
pub use pricing::{
    price_from_benchmark_spread, price_from_g_spread, price_from_spread, price_from_yield,
};

/// Initialize the WASM module (sets up panic hook for better error messages).
#[wasm_bindgen(start)]
pub fn init() {
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}
