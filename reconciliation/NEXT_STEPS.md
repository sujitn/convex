# Next Steps

Work queue for picking up this branch in a fresh session.

## Status

Branch `reconcile/milestone-1-book`. Reconciliation **121 / 121**, zero delta.
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

### 2.1 Day-count-aware coupon / accrued / discount time — done

ACT/360 and ACT/365* now behave correctly on three fronts (leaving the
semi-annual 30/360 / ACT-ACT paths untouched since they already return
1/freq for a regular period):

* `FixedRateBond::cash_flows` uses `annual_coupon × year_fraction(start, end)`
  so each quarterly coupon varies with the 89–92-day period length.
* `AccruedInterestCalculator::standard` uses `face × rate × year_fraction(last, settle)`
  (prorata-in-period is still used for ICMA/30-360).
* `project_discount_fractions` in `yield_solver` short-circuits the ISMA
  `(i+1-v)/freq` formula and falls through to day-count-driven
  `year_fraction(settle, cf_date)`, matching QL's PV path.

UST FRN in `book.json` flipped back to ACT/360 (removed the
`day_count_actual` sidecar note that now duplicates `day_count`).
Reconciliation stays at 117/117 under the real market convention.

Unblocks 2.3: real SOFR forward projection on the same day-count
footing.

### 2.2 TIPS nominal pricing — done

`pull_tips_index_ratio` walks TreasuryDirect `/xml/CPI_*.xml` and captures
the (CUSIP 91282CNS6, 2025-12-31) row: `index_ratio = 1.01395`. Both
benches emit 4 nominal metrics (`cpi_index_ratio`, `nominal_clean_price_pct`,
`nominal_dirty_price_pct`, `nominal_accrued`); reconciliation went 113 → 117.
Follow-up: bake the ratio into `book.json` as `cpi_index_ratio_on_valuation`
once we're comfortable pinning the snapshot, OR let the puller stay the
single source of truth.

### 2.3 Corporate SOFR FRN with real SOFR curve projection — done

Scope intentionally narrower than the original tier sketch — see rationale
below. The UST FRN (`UST_FRN_2Y`) stays on its flat-forward T-Bill path
(it's indexed to the 13-week T-Bill, not SOFR). Added `CORP_SOFR_FRN`:
Marsh & McLennan 571748BZ4, $300M, Compounded SOFR + 65bps, matures
2027-11-08 (see `book.json::CORP_SOFR_FRN.source` for the 424B2).

Curve side:
* `SOFR_OIS_CURVE` in `curves.json` holds pre-bootstrapped continuously-
  compounded zero rates (ACT/365F) at standard OIS tenors
  (1M/3M/6M/1Y/2Y/3Y/5Y/7Y/10Y). Par-quote panel + methodology in
  `SOURCES.md`.
* Both libraries consume the zero rates directly via linear interpolation
  in zero-rate space. This sidesteps Convex-vs-QL OIS bootstrap
  divergence — that belongs in a separate tier.

Pricing convention (documented in full in
`book.json::CORP_SOFR_FRN.coupon_model_note`):
* Quarterly schedule, NullCalendar + Unadjusted, backward-generated from
  maturity — identical dates on both sides.
* Each period's projected coupon: `(DF(start)/DF(end) − 1) × 100 +
  spread × yf360 × 100`. Past dates clamp `DF = 1`.
* Accrued: `100 × current_reset_rate × yf360(last_coupon, settle)`.
* DM: closed-form `(dirty − 100 − accrued) / (spread_annuity × 100)`.

Emits `clean_price_pct`, `dirty_price_pct`, `accrued`,
`discount_margin_bps`. Reconciliation 117 → 121, zero delta
(byte-identical values on both sides).

Deliberate simplification vs. the original tier sketch — **what we did
NOT do**:
* **Live OIS bootstrap on both sides.** The `sofr_fixings.csv` history
  is not a forward-curve input by itself (you'd need SR3 futures or OIS
  swap quotes). We hand-curated the zero-rate panel and documented
  provenance; reconciling the two bootstrappers against each other is
  its own tier.
* **Convex `FloatingRateNote` + QL `FloatingRateBond` wired to an
  `OvernightIndex` with historical fixings.** The two libraries have
  subtly different Compounded-SOFR-in-arrears implementations
  (observation-shift calendar alignment, lookback business-day
  definitions). Reconciling those on real historical fixings is a
  4–6-hour research task that would dwarf this tier. Our manual
  projection convention picks up the curve-driven part cleanly and
  documents the shortcut in `book.json`.

Follow-on (optional, if the manual-projection shortcut ever becomes a
real reconciliation blocker):
* Tier 2.3.1: `FloatingRateNote::cash_flows_projected` vs QL
  `FloatingRateBond(sofr_index)` with live fixings — reconcile only the
  past-period accrual once both ARRC-compound implementations are
  verified calendar-identical.

---

## Tier 3 — Remaining validation

### 3.5 Excel UDF runtime smoke test

SafeCall refactor compiled but never exercised at runtime. Load
`Convex.Excel64.xll`, call `CX.BOND.TSY(...)` + `CX.PRICE(...)`, trigger
an error in one, confirm the error string reaches the cell.

### 3.7 BondPricer numerical regression — done

Probe at `tools/reconcile_bench/examples/bondpricer_regression.rs`.
Builds three known-answer scenarios at par (coupon-rate-at-par →
true YTM = coupon rate), prices via current `BondPricer`, re-solves
YTM via both the current `YieldSolver` (NEW) path and a `YieldSolver`
call forced to `(SemiAnnual, Act365Fixed)` — the exact math the
pre-8ae6574 body did — as the OLD reference.

```
bond                       coupon       NEW ytm       OLD ytm    Δ new    Δ old
--------------------------------------------------------------------------------
ANNUAL_BUND_LIKE          3.0000%     3.000000%     2.976167%   +0.00bp  -2.38bp
QUARTERLY_FRN_LIKE        4.0000%     4.000000%     4.131992%   -0.00bp +13.20bp
SEMI_UST_LIKE             4.0000%     4.000000%     3.997751%   -0.00bp  -0.22bp
```

NEW round-trips to the coupon rate to 1e-10 across all three frequencies.
OLD drifts −2.38 bp on annual and **+13.20 bp** on quarterly —
non-trivial for any caller that went through `BondPricer` on a non-SA
bond. Semi-annual OLD shows a residual −0.22 bp because Act365Fixed
year fraction (days/365) differs from ACT/ACT ICMA year fraction
(period-based); the refactor correctly switched to ICMA for ACT/ACT
bonds. Refactor direction confirmed correct; shipping the current path
is net-positive for every non-semi-annual caller.

Run:
```bash
cargo run -p reconcile_bench --example bondpricer_regression
```

---

## Tier 4 — Housekeeping

### 4.2 Merge PR #77

Branch clean-merges on top of main. CI green.

### 4.4 INDEX.md / OVERVIEW.md cleanup — done

Deleted both. They were setup-package docs from project inception that
had rotted past the point of salvage: referenced non-existent
`Cargo.toml.template` / `README.md.template`, described Java JNI
bindings that were never built, quoted Phase-1 / Weeks-1-2 timelines
long since irrelevant, and duplicated material now in `README.md` and
`CLAUDE.md`. No source file referenced them outside this queue.

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
