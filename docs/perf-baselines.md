# Performance baselines

Local-machine release-build numbers for `cargo bench -p convex-analytics`.
Treat these as a rolling baseline: future PRs that touch the listed code paths
should not regress these numbers by more than 5%. Re-run on the same machine
before/after large refactors and update this file in the same PR.

## Hedge advisor (`benches/hedge_advisor.rs`)

Recorded on Windows 11 / convex-analytics 0.12.1, criterion default settings.

| Bench | Median | Notes |
| --- | --- | --- |
| `risk_profile_apple_10y` | ~22 µs | One `price_from_mark` + one `BondRiskCalculator` + 4-tenor KRD profile. |
| `propose_five_strategies` | ~1.18 ms | Each futures-based strategy builds a single-deliverable BondFuture via `make_default_future`, runs CTD selection trivially (basket of 1), then prices the CTD with full KRD bumps. Callers supplying real multi-deliverable baskets pay extra basket-pricing per leg. |
| `end_to_end` | ~1.21 ms | `risk_profile + propose_five + compare + narrate`. |

The propose path is dominated by KRD bumping — `KeyRateFutures` runs four
`compute_position_risk` calls (one per ladder leg) against the bumped
curve. Amortizing `KeyRateBump` curve construction across legs is the
obvious next optimization but isn't pursued; 1.2 ms is well under
interactive thresholds.

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
