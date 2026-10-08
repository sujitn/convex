//! WebAssembly bindings for Convex fixed income analytics.
//!
//! This crate provides supplied-input library calls and scoped bond
//! conveniences for web browsers. The public
//! `#[wasm_bindgen]` surface is split across submodules by responsibility:
//!
//! - [`analyze`] — `analyze_bond`, `get_cash_flows`, `calculate_accrued`,
//!   `calculate_simple_metrics`
//! - [`pricing`] — `price_from_yield`, `price_from_spread`
//! - [`library`] — supplied-security government spreads and full native terms
//! - [`conventions`] — `get_convention_options`, `get_default_conventions`
//!
//! The non-public modules ([`dto`], [`convert`], [`bond`]) hold the wire
//! types, parser/formatter helpers, and shared bond/curve construction.

use wasm_bindgen::prelude::*;

pub mod analyze;
pub mod bond;
pub mod conventions;
pub mod convert;
pub mod dto;
pub mod pricing;

/// Complete versioned supplied-input library request. Returns the full native
/// JSON envelope as text; exact cash strings never pass through JS Number.
#[wasm_bindgen]
pub fn library(request_json: &str) -> String {
    convex::library::execute_json(request_json)
}

/// Actual generated schema for the complete request and original native terms.
#[wasm_bindgen]
pub fn library_schema() -> String {
    convex::library::request_schema().to_string()
}

pub use analyze::{analyze_bond, calculate_accrued, calculate_simple_metrics, get_cash_flows};
pub use conventions::{get_convention_options, get_default_conventions};
pub use dto::{
    AnalysisResult, BondParams, CallScheduleEntry, CashFlowEntry, ConventionOption,
    ConventionOptions, CurvePoint, DefaultConventions, PriceFromYieldResult,
};
pub use pricing::{price_from_spread, price_from_yield};

/// Initialize the WASM module (sets up panic hook for better error messages).
#[wasm_bindgen(start)]
pub fn init() {
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}
