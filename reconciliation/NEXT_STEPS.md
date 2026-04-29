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

### A1.1 Off-cycle sinker dates

`project_discount_fractions` advances the ICMA period index on each unique
CF date. That works when sink dates align with coupon dates (current
synthetic). For real FHLB / agency sinkers where the paydown date sits
between two coupon dates, the period index will be wrong. Fix: derive the
period index from `cf.date` directly via month arithmetic, not from
date-change detection. Pin with a regression test.

### A3.1 Wire make-whole through the FFI surface

`CallableBond::make_whole_call_price` exists and is reconciled to QL,
but the function isn't exposed through the JSON-RPC FFI. So `convex_price`
on a make-whole-callable returns bullet pricing — Excel sheets pricing
AAPL or Ford Credit silently ignore the MW provision. Wiring needs:

1. `MakeWholeRequest { bond, call_date, treasury_rate }` and response DTO
   in `convex_analytics::dto`.
2. New `convex_make_whole` C symbol in `convex_ffi::ffi`.
3. Dispatch arm in `convex_ffi::dispatch` with a `with_callable_bond!` macro.
4. MCP tool registration.
5. Excel UDF `CX.MW(bond, call_date, ust_rate)`.

### A3.2 MW convention is ACT/365F, not bond day-count

`CallableBond::make_whole_call_price` discounts at ACT/365F time × bond
frequency. Real US-corp 424B2 prospectuses typically specify the bond's
own day-count (30/360 US for Apple, MSFT, Verizon, Ford). The two
conventions disagree by basis points on long-dated MW. Fix: read the
day-count from the underlying bond and use it for MW discount time.

### A4.1 Put-only `CallableBond` is fragile

The bench builds a put-only bond by passing an empty `American` call
schedule to `CallableBond::new(...)` then attaching a put schedule. This
relies on `CallSchedule::is_callable_on(date) → false` for empty
entries — an invariant a future contributor could break with a
"no entries means perpetual call" optimization. Two clean fixes:

(a) Extract `PutableBond` as a sibling of `CallableBond`.
(b) Make `call_schedule: Option<CallSchedule>` on `CallableBond`.

(b) is less code and reuses the existing tree pricer.

### A.x Replace synthetics with real CUSIPs

Three reconciled instruments are clearly-labelled synthetics
(`SYNTH_HY_STEPDOWN_01`, `SYNTH_PLAIN_SINKER_10Y`,
`SYNTH_PUTTABLE_5Y_BERMUDAN`). Each is a placeholder for a real bond
whose schedule sits behind a feed gate (FHLB search form, HY indenture
403s). When a primary-source schedule is digitized, swap the entry —
reconciliation tolerances stay unchanged.
