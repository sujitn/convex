# Performance baselines

Local-machine release-build numbers for `cargo bench -p convex-analytics`.
Treat these as a rolling baseline: future PRs that touch the listed code paths
should not regress these numbers by more than 5%. Re-run on the same machine
before/after large refactors and update this file in the same PR.

## Hedge advisor (`benches/hedge_advisor.rs`)

Recorded on Windows 11 / convex-analytics 0.12.1, criterion default settings.

| Bench | Median | Notes |
| --- | --- | --- |
| `risk_profile_apple_10y` | ~22 µs | One `price_from_mark` + one `BondRiskCalculator` + 4-tenor KRD profile (4 ZSpreadCalculator reprices). |
| `propose_five_strategies` | ~1.79 ms | Each futures-based strategy prices a 2-deliverable basket via `select_ctd`, picks min-net-basis CTD, then prices the chosen CTD with full KRD bumps. |
| `end_to_end` | ~1.75 ms | `risk_profile + propose_five + compare + narrate`. |

The propose path is dominated by per-leg basket pricing — CTD selection on
2 deliverables × 4 futures legs (DurationFutures + Barbell ×2 + KeyRate ×4)
runs ~10 `compute_position_risk` calls. Cheaper alternatives (e.g.
amortizing `KeyRateBump` curve construction across strategy legs) would
recover some of this, but 1.8 ms end-to-end is well under interactive
thresholds and the current code is simple to follow.

## Existing benches — untouched

The hedge advisor adds modules under `risk::profile` and `risk::hedging::*`
and does not modify any existing pricing/curve/bond code path. Existing
benches are expected to be unaffected:

- `convex-analytics/benches/spread_pv_kernel.rs`
- `convex-bonds/benches/trinomial_tree.rs`
- `convex-engine/benches/pricing_benchmarks.rs`

If a future PR touches `risk::dv01`, `risk::duration`, `risk::convexity`,
`pricing.rs`, `spreads/zspread.rs`, or `curves::bumping`, re-run those
benches and update the table.
