//! PnL attribution.
//!
//! Decomposes a book's `t0 → t1` value change into carry, roll-down, curve
//! (parallel / slope / curvature / residual), spread (per benchmark), and a
//! closing residual, then narrates the result with a deterministic template.
//!
//! Extends the hedge advisor (`risk::profile`, `risk::hedging`) with the same
//! patterns: schema-derived wire types, full-revaluation analytics, a
//! deterministic narrator, and provenance on every output. It is a **sibling**
//! of `risk::hedging`, not a child — attribution is not hedging — mirroring
//! the decision that put `compute_position_risk` in `risk::profile`.
//!
//! The pricing core is reused unchanged: `price_from_mark` already takes the
//! valuation date (`settlement`), so two-date repricing is pure orchestration.

// `types` is private to `pnl` (its items are re-exported below). Keeping it
// non-`pub` avoids a prelude glob collision with `hedging::types` while still
// being reachable as `crate::risk::pnl::types` from sibling `pnl` modules.
mod types;

pub use types::{
    Attribution, AttributionConfig, AttributionProvenance, CurveBreakdown, FactorPnl,
    InterestRateSwapPnlSpec, PnlFactor, PositionAttribution, DEFAULT_PIVOT_TENOR_YEARS,
    FACTOR_MODEL_NAME,
};
