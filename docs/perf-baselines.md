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
| `propose_five_strategies` | ~1.18 ms | Five strategies; each futures-based one prices the basket once (single-pass `select_ctd_with_market_or_fair_price`), selects min-net-basis CTD, then prices the chosen CTD via `compute_position_risk`. |
| `end_to_end` | ~1.20 ms | `risk_profile + propose_five + compare + narrate`. |

History:
- v1 release (commit `62d2073`): 22 / 286 / 309 µs for
  `risk_profile / propose_four / end_to_end`.
- 4-strategy baseline after post-review cleanup: 24 / 300 / 328 µs.
- Adding `KeyRateFutures` (4-leg N×N hedge): 21 / 513 / 538 µs.
- CTD optimization landed: 22 / 1440 / 1450 µs (~2.8× regression because
  every futures-based strategy now prices a real deliverable basket
  instead of a hardcoded synthetic 6% bond).
- **CTD perf shortcut (this commit):** 22 / 1180 / 1200 µs. Single-pass
  basket pricing dedup (`select_ctd_with_market_or_fair_price`) saves
  ~17% on propose / ~19% on end-to-end vs the two-pass implementation.
  The remaining slowdown vs the pre-CTD baseline (~1.2 ms vs ~538 µs) is
  structural — the CTD path is genuinely more work than pricing one
  synthetic — but is well under interactive thresholds.

Further optimization is possible but not pursued: amortize `KeyRateBump`
setup across strategies (build the bumped curves once, share with all
legs).

A further optimization could amortize `KeyRateBump` setup across strategies
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
