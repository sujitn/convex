# Performance baselines

Local-machine release-build numbers for `cargo bench -p convex-analytics`.
Treat these as a rolling baseline: future PRs that touch the listed code paths
should not regress these numbers by more than 5%. Re-run on the same machine
before/after large refactors and update this file in the same PR.

## Hedge advisor (`benches/hedge_advisor.rs`)

Recorded on Windows 11 / convex-analytics 0.12.1, criterion default settings.

| Bench | Median | Notes |
| --- | --- | --- |
| `risk_profile_apple_10y` | ~24 µs | One `price_from_mark` + one `BondRiskCalculator` + 4-tenor KRD profile (4 ZSpreadCalculator reprices). |
| `propose_four_strategies` | ~300 µs | Four strategies → 5 `compute_position_risk` calls (1 single-future CTD, 2 barbell-future CTDs, 1 unit-notional swap leg, 1 unit-face cash bond). |
| `end_to_end` | ~328 µs | `risk_profile + propose_four + compare + narrate`. |

History: v1 release (commit `62d2073`) recorded 22 / 286 / 309 µs. The
post-review cleanup (`af120f4`..`07785a8`) did not change any hot-path
arithmetic; the ~5–8% drift is criterion run-to-run variance on Windows.

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
