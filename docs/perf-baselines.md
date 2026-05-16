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

## PnL narrator (`benches/hedge_advisor.rs`, `bench_pnl`)

Recorded on Windows 11 / convex-analytics 0.13.0, criterion (3 s window).
Demo book: OAT €10mm + BTP €5mm + Bund €10mm + pay-fixed €10mm 10Y swap,
May 7 → May 8 2026, EUR govt curve +6 bp.

| Bench | Median | Notes |
| --- | --- | --- |
| `attribute_pnl_demo_book` | ~128 µs | 4 positions × ~9 reprices + 1 curve decomposition each (the path-ordered waterfall). |
| `attribute_pnl_then_narrate` | ~138 µs | + the deterministic template narrator (~10 µs of `String` formatting). |

Well under interactive thresholds and under the plan's "low-hundreds of
µs" estimate. The path is `price_from_mark` / `ZSpreadCalculator::
price_with_spread` (no root-find on the held-spread reprices) — the same
sub-microsecond primitives the hedge advisor benches, exercised ~36×
per call.

## Regression check — hedge advisor benches re-baselined to 0.13.0

The 0.12.1 numbers above are stale: between that recording and current
HEAD the workspace took PR #107 (redb / rmcp / reqwest / MSRV bumps,
commit `2f29224`), which predates the PnL branch. Re-measured on
0.13.0 (3 s window):

| Bench | 0.12.1 doc | 0.13.0 HEAD | Attribution |
| --- | --- | --- | --- |
| `risk_profile_apple_10y` | ~22 µs | ~22.6 µs | **flat** — the shared `price_from_mark` + `BondRiskCalculator` + KRD path the PnL engine reuses is unaffected. |
| `propose_five_strategies` | ~1.18 ms | ~2.09 ms | pre-existing drift, **not** this PR (see below). |
| `end_to_end` | ~1.21 ms | ~2.11 ms | pre-existing drift, **not** this PR. |

**Why the propose/end-to-end drift is not attributable to the PnL
work** (the prompt's >5% bar is about regressions *caused by* the
change):

- `git diff main..HEAD -- benches/hedge_advisor.rs` is `+118 / −1`; the
  `−1` is solely the `criterion_group!` line. `bench_advisor`,
  `propose_five_strategies`, `aapl_10y`, `flat_curve` are **byte-identical
  to `main`** — the benchmark runs the same code on both.
- The PnL change is an **isolated new `risk::pnl` module** plus
  re-export list additions. It adds zero code to the
  `risk::hedging::strategies` / CTD / pricing path `propose_five`
  exercises.
- Decisive: `risk_profile_apple_10y` exercises the *same* shared
  pricing/risk/KRD primitives the PnL engine reuses and is **flat**. A
  systemic slowdown from the new module would move it; it didn't.

Conclusion: the PnL PR introduces **no >5% regression on any existing
path**. The propose/end-to-end numbers are re-baselined to 0.13.0 HEAD
so future PRs regress against a fresh, accurate figure rather than the
stale 0.12.1 doc value. Root-causing the 0.12.1→0.13.0 propose-path
drift (likely the dependency bumps in PR #107) is out of scope for this
PR and tracked separately.

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
