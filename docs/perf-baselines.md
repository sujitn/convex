# Performance baselines

Local-machine release-build numbers for `cargo bench -p convex-analytics`.
Treat these as a rolling baseline: future PRs that touch the listed code paths
should not regress these numbers by more than 5%. Re-run on the same machine
before/after large refactors and update this file in the same PR.

## Hedge advisor (`benches/hedge_advisor.rs`)

Recorded on Windows 11 / convex-analytics 0.12.1, criterion default settings.

| Bench | Median | Notes |
| --- | --- | --- |
| `risk_profile_apple_10y` | ~21 µs | One `price_from_mark` + one `BondRiskCalculator` + 4-tenor KRD profile (4 ZSpreadCalculator reprices). |
| `propose_five_strategies` | ~513 µs | Five strategies → 9 `compute_position_risk` calls (1 single-future CTD, 2 barbell-future CTDs, 4 key-rate-future CTDs, 1 unit-notional swap leg, 1 unit-face cash bond) + one 4×4 LU solve. |
| `end_to_end` | ~538 µs | `risk_profile + propose_five + compare + narrate`. |

History: v1 release (commit `62d2073`) recorded 22 / 286 / 309 µs for
`risk_profile / propose_four / end_to_end`. After collapsing post-review
cleanup the 4-strategy baseline settled at 24 / 300 / 328 µs. Adding
`KeyRateFutures` (a 4-leg N×N hedge) brought the propose/end-to-end benches
from ~300 µs to ~513 µs — the new strategy makes 4 extra `compute_position_risk`
calls (one per CTD leg) and a small LU solve. Single-position risk profile
is unchanged.

A v2 optimization could amortize `KeyRateBump` setup across strategies
(build the bumped curves once, share with all legs) — not pursued.

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
