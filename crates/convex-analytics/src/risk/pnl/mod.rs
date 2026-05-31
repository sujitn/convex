//! PnL attribution: decompose a book's `t0 → t1` value change into carry,
//! roll-down, curve (parallel/slope/curvature/residual), spread (per
//! benchmark) and a closing residual, then narrate it deterministically.
//! Design rationale is in `docs/pnl-narrator-*.md`.

// Submodules are private to `pnl` (their items are re-exported below).
// Keeping them non-`pub` avoids prelude glob collisions (e.g. with
// `hedging::types`) while staying reachable as `crate::risk::pnl::<m>` from
// sibling `pnl` modules.
mod decompose;
mod engine;
mod narrate;
mod types;

pub use decompose::{decompose_curve_move, CurveComponent, CurveDecomposition};
pub use engine::{attribute_pnl, ResolvedBook, ResolvedPosition};
pub use narrate::narrate_attribution;
pub use types::{
    Attribution, AttributionConfig, AttributionProvenance, CurveBreakdown, FactorPnl,
    InterestRateSwapPnlSpec, PnlFactor, PositionAttribution, DEFAULT_PIVOT_TENOR_YEARS,
    FACTOR_MODEL_NAME,
};
