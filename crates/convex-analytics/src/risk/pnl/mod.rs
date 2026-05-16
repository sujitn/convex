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

// Submodules are private to `pnl` (their items are re-exported below).
// Keeping them non-`pub` avoids prelude glob collisions (e.g. with
// `hedging::types`) while staying reachable as `crate::risk::pnl::<m>` from
// sibling `pnl` modules.
mod decompose;
mod types;

pub use decompose::{decompose_curve_move, CurveComponent, CurveDecomposition};
pub use types::{
    Attribution, AttributionConfig, AttributionProvenance, CurveBreakdown, FactorPnl,
    InterestRateSwapPnlSpec, PnlFactor, PositionAttribution, DEFAULT_PIVOT_TENOR_YEARS,
    FACTOR_MODEL_NAME,
};
