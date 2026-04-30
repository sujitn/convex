# Next Steps

Reconciliation **188 / 188** across 2025-12-31 and 2025-06-30. Excel add-in
builds; CI's `.github/workflows/reconcile.yml` runs both snapshots.

Smoke test:

```bash
cargo test --workspace --lib
python reconciliation/ql_bench.py
cargo run -p reconcile_bench
python reconciliation/reconcile.py    # exit 0
```

## Open

### 5.2.3 Real CME swaption vol ingest

Replace `swaptions_20251231.csv` with real ATM USD SOFR normal vols. Blocked:
no clean free programmatic feed. Unblock when a CSV is dropped manually or a
free feed appears.

### 5.2.5 Piecewise-constant σ(t)

Extend HW1F to piecewise-σ on call dates (matches QL's `Gsr`). Only worth
doing once a long-dated callable (NC10 30y) is in the book.

### A.x Replace synthetics with real CUSIPs

Four reconciled instruments are clearly-labelled synthetics
(`SYNTH_HY_STEPDOWN_01`, `SYNTH_PLAIN_SINKER_10Y`,
`SYNTH_PUTTABLE_5Y_BERMUDAN`, `SYNTH_CALLABLE_SOFR_FRN`). Each is a
placeholder for a real bond whose schedule sits behind a feed gate.
Replace once a primary-source schedule is digitized.

### B1.x Callable FRN — FFI / Excel surface

`CallableFloatingRateNote` is library-only; no `BondSpec::CallableFrn`
variant, no dedicated RPC, no Excel UDF. Wire after a real CUSIP lands.
