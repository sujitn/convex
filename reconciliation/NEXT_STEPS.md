# Next Steps

Work queue for picking up this branch in a fresh session.

## Status

Branch `reconcile/milestone-1-book`. Reconciliation **117 / 117**, zero delta.
Workspace `cargo test --all-targets` **1736 / 0**. Clippy clean under
`-D warnings`. Doc tests 4 / 0. Excel add-in builds. CI has a
reconciliation gate (`.github/workflows/reconcile.yml`).

Smoke test:

```bash
cargo test --workspace --lib
cargo run -p reconcile_bench
python reconciliation/ql_bench.py
python reconciliation/reconcile.py    # exit 0
```

Session history: all of Tier 1 (real UK/EU/JP curves + `ActActIcma` trait
fallback + consolidated findings), Tier 3 (clippy / tests / docs / FRED
retry / release-dry-run), Tier 4.1 (`.gitattributes`), Tier 4.3 (CI gate),
plus a pivot to internal-only (dropped crates.io publishing, removed
`scripts/release.sh`, stripped version constraints from
`[workspace.dependencies]`).

---

## Tier 2 — Real library work

### 2.1 Day-count-aware `FixedRateBond::coupon_per_period` — blocked on 5.1

Under ACT/360 quarterly, QL computes `coupon = rate × year_fraction(start, end)`
(0.9225–0.9427 depending on 89–92-day quarter); Convex returns `rate × face
/ freq` (flat 0.9225). For semi-annual 30/360 / ACT-ACT ICMA these agree.
UST FRN in `book.json` uses ACT/ACT ICMA as a documented workaround.

Cleanest fix: leave `coupon_per_period` as a nominal value, but have
`FixedRateBond::cash_flows` scale each coupon by
`day_count.year_fraction(accrual_start, accrual_end)` when the day count
is ACT/360 or ACT/365. The stub-coupon path already does this — generalize.

Validation: swap UST FRN back to ACT/360, rerun reconciliation; should
stay at 113/113. ~1 hr.

### 2.2 TIPS nominal pricing — done

`pull_tips_index_ratio` walks TreasuryDirect `/xml/CPI_*.xml` and captures
the (CUSIP 91282CNS6, 2025-12-31) row: `index_ratio = 1.01395`. Both
benches emit 4 nominal metrics (`cpi_index_ratio`, `nominal_clean_price_pct`,
`nominal_dirty_price_pct`, `nominal_accrued`); reconciliation went 113 → 117.
Follow-up: bake the ratio into `book.json` as `cpi_index_ratio_on_valuation`
once we're comfortable pinning the snapshot, OR let the puller stay the
single source of truth.

### 2.3 UST FRN with real SOFR forward projection

Current FRN uses flat-forward. Real discount margin needs a projected
SOFR forward curve.

1. Bootstrap USD SOFR forward curve from `sofr_fixings.csv`.
2. Wire `OvernightIndex` (QL) and `FloatingRateBond` (Convex) to the
   same curve with historical fixings.
3. Reconcile discount margin + price.

Largest item here; pair with 2.1 since both affect FRN pricing. 4–6 hr.

---

## Tier 3 — Remaining validation

### 3.5 Excel UDF runtime smoke test

SafeCall refactor compiled but never exercised at runtime. Load
`Convex.Excel64.xll`, call `CX.BOND.TSY(...)` + `CX.PRICE(...)`, trigger
an error in one, confirm the error string reaches the cell.

### 3.7 BondPricer numerical regression against a reference book

`BondPricer::yield_to_maturity` now delegates to `YieldSolver` — that
shifted numbers for non-semi-annual bonds priced through `BondPricer`.
Before tagging, run an internal pricing book through both the old
approach (git history) and the current one and confirm the new numbers
are the expected direction (closer to QL).

---

## Tier 4 — Housekeeping

### 4.2 Merge PR #77

Branch clean-merges on top of main. CI green.

### 4.4 INDEX.md / OVERVIEW.md cleanup

Original review flagged both as emoji-heavy / duplicative with README
and CLAUDE.md. Decide delete vs. consolidate.

---

## Tier 5 — Design calls (don't start without input)

### 5.1 `FixedRateBond::coupon_per_period` — match QL or keep Convex house?

Current "coupon = rate / freq" is correct for UST-style equal-length
schedules. Under ACT/360 quarterly it differs from QL by ~0.02 per 100
per quarter. Tier 2.1 assumes Convex should match QL; a house convention
may disagree. Load-bearing for whether to proceed with 2.1.

### 5.2 OAS / tree models for callables

Current reconciliation uses deterministic YTC/YTW on workout-bullet
proxies. Real OAS against a short-rate model (Hull-White / BK) isn't
tested. Medium-to-large scope; requires picking a shared model.

### 5.3 Deprecate `BondPricer::yield_to_maturity`?

Now a thin delegate to `YieldSolver`; duplicates `FixedRateBond::yield_to_maturity`.
Low urgency.
