# Next Steps

Work queue for picking up this branch in a fresh session.

## Status

Branch `reconcile/milestone-1-book`. Reconciliation **141 / 141** under
calibrated HW1F params (137 prior + 4 new calibration-parity rows from Tier
5.2.4), across two snapshots (2025-12-31 full mixed book + 2025-06-30
FRN-focused mid-period mini-book). 16 of 141 are HW1F trinomial OAS or
calibration metrics on the two callables. Per-bond σ is calibrated on
*both* sides independently (QL: `JamshidianSwaptionEngine` + LM; Rust:
`payer_swaption_hw1f` + golden-section), against the same ATM USD SOFR
co-terminal swaption strip in `swaptions_20251231.csv` with `a=0.03`
fixed. Worst Rust-vs-QL σ residual: 1.6e-5 absolute on Ford (sparse
2-helper strip); SYNTH_HY agrees to 4.8e-6. Workspace `cargo test --lib`
clean. Clippy clean under `-D warnings`. Excel add-in builds. CI has a
reconciliation gate (`.github/workflows/reconcile.yml`) that runs both
snapshots.

Smoke test:

```bash
cargo test --workspace --lib
cargo run -p reconcile_bench
python reconciliation/ql_bench.py
python reconciliation/reconcile.py    # exit 0
```

## Tier 2 — Real library work

- **2.1** Day-count-aware coupon / accrued / discount time — **done** (`d353c99`)
- **2.2** TIPS nominal pricing — **done** (`8cbe181`)
- **2.3** Corporate SOFR FRN with real SOFR curve projection — **done**
  (`b5de93d`, refined `ac4f1b9`). UST FRN stays on its flat-forward
  T-Bill path.
- **2.3.1** Live FloatingRateNote × FloatingRateBond reconciliation
  (narrow scope) — **done**.
  - `convex_bonds::arrc::compound_in_arrears` implements ARRC
    compound-in-arrears with observation shift, lookback, lockout, and
    spread-additive convention matching QL `OvernightIndexedCoupon`
    (`applyObservationShift=true, lookbackDays=2, lockoutDays=0`).
  - `convex_bonds::fixings::OvernightFixings` registers daily SOFR
    fixings from `reconciliation/sofr_fixings.csv`.
  - Both reconcile_bench and `ql_bench.py` price the in-progress coupon
    with real fixings + curve forwards; future periods use deterministic
    curve projection on both sides. Settlement 2025-12-31 → all four
    FRN metrics agree to 1e-10 (clean/dirty/accrued/DM).
  - Fixed a longstanding bug along the way: `CalendarId::us_government()`
    was silently dispatching to `WeekendCalendar` because the const value
    `"USGov"` wasn't in the dispatch table — so US holidays were ignored
    in any logic that walked business days. Calendar adjustments on
    `Unadjusted`-BDC schedules masked it from the prior 121/121.

- **2.3.2** Mid-period FRN snapshot — **done**.
  - New 2025-06-30 snapshot: `book_20250630.json` + `curves_20250630.json`
    (FRN-focused mini-book; CORP_SOFR_FRN sits inside its 2025-05-08 →
    2025-08-08 coupon period, with two prior coupons paid).
  - `reconcile_bench` and `ql_bench.py` both iterate a `SNAPSHOTS` list;
    output goes to `convex_<label>.csv` / `ql_<label>.csv`. Default
    snapshot keeps `convex.csv` / `ql.csv` for backward compat.
  - `reconcile.py` aggregates across snapshots — full report shows one
    section per snapshot, single pass/fail count rolled up.
  - **Total reconciliation: 125 / 125, zero delta.** (121 from 2025-12-31
    + 4 FRN metrics from 2025-06-30.)
  - Library change: added `as_of: Option<Date>` to
    `compound_in_arrears`. Avoids look-ahead bias when the fixings
    registry contains rates published after the valuation date — for
    obs days strictly after `as_of` the pricer falls through to the
    projection-curve forward, matching QL's
    `OvernightIndex::fixing(d > evaluationDate) → forecastFixing(d)`
    behaviour.
  - CI artifact list extended to include the new snapshot CSVs.

## Tier 3 — Remaining validation

- **3.5** Excel UDF runtime smoke test — **done**. Protocol at
  `excel/SMOKE_TEST.md` (`c67846a`, confirmed pass `aaa6a90`).
- **3.7** BondPricer numerical regression — **done**. Probe at
  `tools/reconcile_bench/examples/bondpricer_regression.rs` (`4ec1e03`).
  OLD path drifts +13.2 bp on quarterly, −2.4 bp on annual; NEW
  round-trips to 1e-10. Refactor direction confirmed correct.

## Tier 4 — Housekeeping

### 4.2 Merge PR #77

Branch clean-merges on top of main. CI green.

- **4.4** Delete stale INDEX.md / OVERVIEW.md — **done** (`74090c4`).

## Tier 5 — Design calls (don't start without input)

- **5.1** `CashFlowGenerator` match QL coupon-by-day-count — **done**
  (`3b23dc8`). Superseded by the 5.3 purge below, which deleted
  `CashFlowGenerator` entirely.
- **5.3** Delete the `FixedBond` / `BondPricer` / `CashFlowGenerator` /
  `GovernmentCouponBond` legacy island — **done**. Everything in the
  production call graph (FFI, Excel, engine, server, MCP, portfolio,
  reconcile bench) already went through `FixedRateBond` +
  `BondAnalytics`; the legacy types were only self-referenced.

### 5.2 OAS / tree models for callables — **done (HW1F)**

HW1F trinomial-tree OAS on both callables in the book, reconciled against
`ql.TreeCallableFixedRateBondEngine`. New
`convex_bonds::options::TrinomialTree` (Hagan-Brace, Arrow-Debreu α(t)
calibration); `OASCalculator` rewired to it. QL side uses
`ql.HullWhite(handle, 0.03, 0.008)` with a daily-densified
`CallabilitySchedule` and `ZeroSpreadedTermStructure` for OAS shifts.
Metrics: `price_at_oas_{25,50,100}bps`, `oas_bps_at_market`,
`effective_duration_at_oas`, `effective_convexity_at_oas`. Sub-ppm
parity on Ford; ~$1.7 / 1.6 bp residual on the coupon-aligned
SYNTH_HY callable, tracked under 5.2.1.

### 5.2.1 Event-aligned trinomial TimeGrid — **done**

* `TrinomialTree` runs on a non-uniform grid via
  `build_hull_white_on_grid`; new `build_event_grid` mirrors QL's
  `TimeGrid` (mandatory times land on layers exactly).
* `OASCalculator` builds the grid from cashflow dates + step-down
  boundaries and matches QL's coupon-on-call convention: receive at the
  first callable layer, forfeit elsewhere (the latter encoded as
  `cap - cashflow`).

Reconciliation 137 / 137. SYNTH_HY_STEPDOWN_01 collapsed from $1.7 / 1.6 bp
residuals to sub-ppm on the OAS-given prices (Δ < $2e-4) and 0.40 bps on
`oas_bps_at_market`.

### 5.2.2 ATM swaption-strip vol calibration — **done**

Per-bond σ is calibrated by `ql_bench.py` against an ATM USD SOFR co-terminal
swaption strip (`SwaptionHelper` + `JamshidianSwaptionEngine` +
`LevenbergMarquardt`, `fixParameters=[True, False]` to hold a=0.03 fixed,
matching QL's `Examples/CallableBonds/CallableBonds.cpp` and Bloomberg OAS1).
Calibrated `(a, σ)` is emitted to `hw1f_params_<snapshot>.json` and consumed
by `reconcile_bench`. The synthetic swaption surface in
`swaptions_20251231.csv` is humped (95→110 bp normal vol from 1y→7y) — typical
post-2022 USD shape. Calibration vs hardcoded params: SYNTH_HY tightened from
0.40 bp to 0.14 bp on `oas_bps_at_market`; Ford prices stay sub-cent. The
calibration step is upstream — Rust consumes parameters as inputs, not a
recalibration target (LM bit-parity across QL and a Rust optimizer is tracked
under 5.2.4 below).

### 5.2.3 Real CME swaption vol ingest

Replace the synthetic vol surface in `swaptions_20251231.csv` with daily ATM
USD SOFR normal vols pulled from CME QuikStrike (free download). Adds a
`pull_swaption_vols.py` to `pull_market_data.py`, keys quotes by snapshot
date, no other code changes needed (the calibration loop is surface-agnostic).
Light lift — half a day, mostly schema work + handling missing data.

### 5.2.4 Rust HW1F calibrator — **done**

Native Rust calibrator implemented as Jamshidian closed-form (new module
`crates/convex-bonds/src/options/swaption_hw1f.rs`) + golden-section search
on relative-price residual (`crates/convex-analytics/src/calibration/hw1f.rs`).
With `a` fixed at 0.03 the problem is 1D, so a full LM driver isn't justified
— golden-section is dependency-free and converges in ~50 iterations. Self-
consistent test: feed back HW1F-implied Bachelier vols and recover `σ` to
<1e-5 (passes). On the live SOFR strip Rust σ matches QL σ to 1.6e-5 (Ford,
sparse strip) and 4.8e-6 (SYNTH_HY). Calibration-parity rows
`hw1f_a_calibrated` and `hw1f_sigma_calibrated` now flow through the
reconcile pipeline with 1e-12 / 1e-4 absolute tolerances. Calibration is
still loaded *upstream* for OAS pricing (from
`hw1f_params_20251231.json`) — Rust calibration runs in parallel as an
independent validation, not as a replacement, so QL remains the
single source of truth for the params used in the trinomial tree.

### 5.2.5 Piecewise-constant σ(t) on event grid

Stretch goal: extend HW1F to piecewise-constant σ(t) with step dates aligned
to call dates (matches QL's `Gsr` model and the event-aligned trinomial grid
from 5.2.1). Real value-add for long-dated callables (NC10 30y) where front
vs back vol genuinely differ; skip for the current 5y / 4y book.
