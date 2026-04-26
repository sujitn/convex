# Next Steps

Work queue for picking up this branch in a fresh session.

## Status

Reconciliation **141 / 141** across 2025-12-31 and 2025-06-30 snapshots
(`cargo test --workspace --lib`, `cargo clippy --workspace --tests --examples
-- -D warnings`, and `python reconciliation/reconcile.py` all clean). 16 of
141 are HW1F trinomial-tree OAS or calibration metrics on the two callables;
σ is calibrated independently on each side against the same ATM SOFR
co-terminal strip with `a=0.03` fixed. Excel add-in builds; CI's
`.github/workflows/reconcile.yml` runs both snapshots.

Smoke test:

```bash
cargo test --workspace --lib
python reconciliation/ql_bench.py
cargo run -p reconcile_bench
python reconciliation/reconcile.py    # exit 0
```

## Done

- **2.1** Day-count-aware coupon / accrued / discount time (`d353c99`).
- **2.2** TIPS nominal pricing (`8cbe181`).
- **2.3** Corporate SOFR FRN with real SOFR projection (`b5de93d`, `ac4f1b9`).
- **2.3.1** Live `FloatingRateNote` × `FloatingRateBond` reconciliation
  (also fixed `CalendarId::us_government()` silently dispatching to
  `WeekendCalendar`).
- **2.3.2** 2025-06-30 mid-period FRN snapshot + multi-snapshot reconcile.
- **3.5** Excel UDF runtime smoke test (`c67846a`, `aaa6a90`).
- **3.7** BondPricer numerical regression probe (`4ec1e03`).
- **4.4** Stale INDEX.md / OVERVIEW.md cleanup (`74090c4`).
- **5.1** `CashFlowGenerator` coupon-by-day-count (`3b23dc8`, then deleted in 5.3).
- **5.2** HW1F trinomial-tree OAS, reconciled against
  `ql.TreeCallableFixedRateBondEngine`.
- **5.2.1** Event-aligned trinomial `TimeGrid` + coupon-on-call convention
  (`1668ce7`).
- **5.2.2** Per-bond ATM swaption-strip σ calibration on the QL side, with
  `a=0.03` held fixed (`5405c5e`).
- **5.2.4** Native Rust HW1F calibrator (Jamshidian closed-form +
  golden-section), reconciled against QL σ to ~1e-5 (`5405c5e`).
- **5.3** Removed legacy `FixedBond` / `BondPricer` / `CashFlowGenerator` /
  `GovernmentCouponBond` island.

## Open

### 5.2.3 Real CME swaption vol ingest

Replace `swaptions_20251231.csv` with real ATM USD SOFR normal vols. Blocked:
no clean free programmatic feed (CME publishes settlement data via website
UI only; DataMine and MDP are licensed). Unblock when (a) a CSV is dropped
into the repo manually or (b) a free feed appears.

### 5.2.5 Piecewise-constant σ(t)

Extend HW1F to piecewise-σ on call dates (matches QL's `Gsr`). Only worth
doing once a long-dated callable (NC10 30y) is in the book — current 5y/4y
residuals don't move under term structure of vol.
