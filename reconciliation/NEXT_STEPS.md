# Next Steps

Work queue for picking up this branch in a fresh session.

## Status

Branch `reconcile/milestone-1-book`. Reconciliation **121 / 121**, zero delta.
Workspace `cargo test --all-targets` **1672 / 0**. Clippy clean under
`-D warnings`. Excel add-in builds. CI has a reconciliation gate
(`.github/workflows/reconcile.yml`).

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
  T-Bill path. Followup `2.3.1` below if the ARRC shortcut ever becomes
  a blocker.

### 2.3.1 Live FloatingRateNote × FloatingRateBond reconciliation (deferred)

Wire Convex `FloatingRateNote` and QL `FloatingRateBond(sofr_index)` to
the same curve with live historical fixings and reconcile past-period
accrual. Needs the two ARRC compound-in-arrears implementations
verified calendar-identical first (observation shift, lookback
business-day rules, publication lag). 4–6 hours of research + impl.

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

### 5.2 OAS / tree models for callables

Current reconciliation uses deterministic YTC/YTW on workout-bullet
proxies. Real OAS against Hull-White / BK isn't tested. Needs a shared
model choice first.
