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
| `propose_five_strategies` | ~1.44 ms | Five strategies — each futures-based one now does CTD selection (price every deliverable in the basket on the spot curve, compute fair forward, select min net basis), then prices the chosen CTD via `compute_position_risk`. |
| `end_to_end` | ~1.45 ms | `risk_profile + propose_five + compare + narrate`. |

History:
- v1 release (commit `62d2073`): 22 / 286 / 309 µs for
  `risk_profile / propose_four / end_to_end`.
- 4-strategy baseline after post-review cleanup: 24 / 300 / 328 µs.
- Adding `KeyRateFutures` (4-leg N×N hedge): 21 / 513 / 538 µs.
- **CTD optimization (this commit):** 22 / 1440 / 1450 µs. Single-position
  risk profile is unchanged. The propose/end-to-end benches got ~2.8× slower
  because every futures-based strategy now exercises the full CTD path
  (basket pricing + fair-forward + net-basis selection + CTD spot price)
  instead of a single hardcoded synthetic 6% bond. With 1 deliverable per
  basket the CTD path adds ≈3 extra `compute_position_risk` calls per
  futures leg (curve-pricing the deliverable for fair-forward, then for
  selection, then for risk).

Two cheap optimizations available if needed:
1. Cache `(spot, coupons)` in `select_ctd_by_net_basis` so the basket
   isn't priced twice (once in `fair_futures_price`, once in selection).
   ~30% saving on the futures path.
2. Single-deliverable shortcut: skip the basket loop when
   `deliverable_basket.len() == 1`. The default strategy baskets are
   single-deliverable, so this would recover most of the regression for
   advisor consumers.

Neither is pursued in this commit — net-basis-driven CTD is a substantive
correctness improvement and the absolute numbers (1.5 ms end-to-end) are
still well under interactive UI thresholds.

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
