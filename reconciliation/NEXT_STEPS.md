# Next Steps — Deferred and Outstanding Items

A pick-up-and-execute plan for anyone continuing this work in a fresh session.

## Where things stand

* Branch `reconcile/milestone-1-book`, Tier 1 + Tier 3 + Tier 4.1 applied on top of commit `324447d`.
* **Reconciliation: 113 / 113 pass, zero delta**. 13 instruments across 8–14 metrics each (UST 10Y/30Y/5Y/FRN/TIPS, UK Gilt, Bund, JGB, Apple/MSFT/Verizon bullets, Ford Credit callable, synthetic HY step-down).
* **Workspace `cargo test --all-targets`: 1736 pass, 0 fail.** Doc tests: 4 pass. Clippy (`-D warnings`): clean.
* Excel add-in builds clean (0 errors, 0 warnings).
* QuantLib 1.40 + Python 3.13 confirmed working.
* **Tier 1 complete**: real UK/EU/JP curves pulled into `curves.json` (1.1); `ActActIcma::year_fraction` trait fallback fixed (1.2); `FINDINGS_M*.md` consolidated into `FINDINGS.md` (1.3).
* **Tier 3 complete** (validation): clippy clean, test suite green, doc tests green, FRED pull retried (blocked in env, other sources work), release script dry-run surfaced a version-bump blocker (see 3.6).
* **Tier 4.1 complete**: `.gitattributes` expanded.

Everything below is safe to start after checking out the branch and running:

```bash
cargo test --workspace --lib                             # should see 1560 pass
cargo run -p reconcile_bench
python reconciliation/ql_bench.py
python reconciliation/reconcile.py                       # should exit 0
```

---

## Tier 1 — Quick wins (≤ 1 hr combined)

These are safe, isolated, and improve either realism or library hygiene without touching the reconciliation pass count.

### 1.1  Real UK / EU / JP discount curves in `curves.json`

*Status.* **Done.** `curves.json` now carries `UK_GILT_CURVE` (BoE nominal spot, 2025-12-31, 8 tenors), `DE_BUND_CURVE` (ECB AAA SDMX `SR_<tenor>`, 2025-12-31, 9 tenors), and `JP_JGB_CURVE` (MOF `jgbcme_all.csv`, 2025-12-30 — Japan markets closed 12-31). Both `reconcile_bench` and `ql_bench.py` dispatch by currency. 113/113 still passes. Details: "Post-M5 quick wins" block in `FINDINGS.md`.

*Original motivation (kept for posterity).* Previously those three sovereign reconciliations used `coupon_rate`-as-reference-yield placeholders. Cross-library consistency was perfect either way, but the reported numbers weren't market-realistic.

*How.*

* UK Gilt yield curve: https://www.bankofengland.co.uk/statistics/yield-curves — "Nominal spot curve" CSV for 2025-12-31.
* German Bund curve: ECB AAA euro-area government bond yield curve, https://www.ecb.europa.eu/stats/financial_markets_and_interest_rates/euro_area_yield_curves/html/index.en.html
* JGB curve: MoF Japan https://www.mof.go.jp/english/policy/jgbs/reference/interest_rate/

Pull a handful of tenors (1Y, 2Y, 5Y, 10Y, 20Y, 30Y where available), add them to `reconciliation/curves.json` as `UK_GILT_CURVE` / `DE_BUND_CURVE` / `JP_JGB_CURVE`, and update the `reference_yield` helper in both benches to interpolate those for the matching currencies instead of falling through to the coupon-rate placeholder.

*Expected outcome.* 113/113 still passes; report shows real market yields (e.g. ~4.5% UK, ~2.3% Bund, ~1.5% JGB on 2025-12-31 instead of the coupon-rate fallbacks).

*Effort.* ~30 min.

### 1.2  Fix `ActActIcma::year_fraction` in `convex-core`

*Status.* **Done.** `convex-core/src/daycounts/actact.rs` — the trait `year_fraction(start, end)` for `ActActIcma` now delegates to `ActActIsda` (calendar-year split) when period bounds are unknown, instead of the off-by-~0.3% `days / (freq · round(365/freq))` approximation. For accrual and period-aware PV, callers still use `year_fraction_with_period` (or `project_discount_fractions` in `convex-bonds`). Workspace lib tests pass (1538+); reconciliation still 113/113.

*Original motivation (kept for posterity).* `convex-core/src/daycounts/actact.rs:195` used the `days / (freq · round(365/freq))` approximation. PV code no longer calls it (we routed everything through `project_discount_fractions`), but external library users of the `DayCount` trait hit it. The method's own comment admitted it wasn't production-grade.

*How.* Two options:

1. Add a `Default` for `ActActIcma` that uses semi-annual, and document `year_fraction` as "informational only — use `year_fraction_with_period` for accrual or the `project_discount_fractions` helper for PV." Keep the approximation.
2. Replace the trait method with `year_fraction_with_period(start, end, start, end)` — treating the whole span as one nominal period. For a span equal to `1/freq` that returns `1/freq` correctly.

Option 2 is better if we can determine the period bounds. Without them, option 1 with a clear deprecation note is honest.

*Expected outcome.* Trait method no longer ambushes future callers.

*Effort.* ~30 min.

### 1.3  Consolidate `FINDINGS_M*.md` into a single running report

*Status.* **Done.** See `FINDINGS.md` — timeline header plus per-milestone sections plus a "Post-M5 quick wins" block for 1.1 / 1.2 / 1.3.

---

## Tier 2 — Real library work (1–2 hr each)

Each of these surfaces a genuine design question or adds new test coverage that *could* reveal bugs.

### 2.1  Day-count-aware `FixedRateBond::coupon_per_period`

*Why.* Under ACT/360 (real UST FRN convention), each quarterly coupon is `rate × year_fraction(start, end) = rate × actual_days / 360`. Under QL this varies 0.9225–0.9427 per coupon depending on the 89–92 day quarter. Convex currently uses `rate / freq = 0.9225` uniformly. For semi-annual 30/360 and ACT/ACT ICMA the two approaches converge; for quarterly ACT/360 they don't. Today the FRN in the reconciliation book is kept at `ACT/ACT ICMA` as a workaround (see the Milestone 5 section of `FINDINGS.md`).

*How.* In `convex-bonds/src/instruments/fixed_rate.rs`, change `coupon_per_period()` to:

```rust
pub fn coupon_per_period(&self) -> Decimal {
    // For conventions that produce year_fraction = 1/freq for a regular
    // period (semi-annual 30/360, ACT/ACT ICMA) this is unchanged. For
    // ACT/360 quarterly it returns the actual per-quarter amount.
    let dc = self.day_count.to_day_count();
    let yf = dc.year_fraction(..., ...);  // but we don't have period bounds here
    self.face_value * self.coupon_rate * yf
}
```

The complication: `coupon_per_period` is currently a scalar method that doesn't know which coupon period. The refactor needs to thread period bounds through.

Cleaner path: leave `coupon_per_period` alone as a "nominal" value, but have `FixedRateBond::cash_flows` compute per-period coupons using `day_count.year_fraction(accrual_start, accrual_end)` for day counts where that matters (ACT/360, ACT/365). The stub-coupon path already does this; generalize it to all periods when day_count is one of those.

*Validation.* Swap the UST FRN's `day_count` back to `"ACT/360"` in `book.json` and set the Python `ql_bench` to use `ql.Actual360()`. Rerun reconciliation. Should flip to 113/113 under ACT/360 directly; if it doesn't, the fix needs more work.

*Expected outcome.* FRN reconciles under its actual market convention without the ICMA workaround. Confirms Convex's FRN pricing matches QL on a real UST FRN.

*Effort.* ~1 hr to implement + test.

*Risk.* Changes `coupon_per_period` behavior for any existing caller using ACT/360. Need to grep for callers in the bonds crate, FFI, wasm bindings. Likely small surface; run workspace tests.

### 2.2  TIPS nominal pricing with live CPI index ratio

*Why.* Real-yield TIPS reconciliation already passes 8/8. Nominal pricing (the number a trader actually quotes) requires scaling principal + coupons by the CPI index ratio at the valuation date. Adds a new dimension of test coverage that could surface subtle ratio-handling bugs.

*How.*

1. Pull the CPI index ratio for CUSIP `91282CNS6` on 2025-12-31 from TreasuryDirect. The puller stub exists in `reconciliation/pull_market_data.py` — finish the implementation (the detail page at https://www.treasurydirect.gov/auctions/announcements-data-results/tips-cpi-data/tips-cpi-detail/?cusip=91282CNS6 has a CSV download).
2. On the Convex side: check if `FixedRateBond` can express an inflation-linked bond, or whether there's a separate inflation bond type. Adapt the bench to price the TIPS with the ratio applied.
3. On the QL side: `ql.CPI` indices + `ql.ZeroCouponInflationIndex` / `ql.InflationIndex`. Simpler: manually scale coupons and compute as fixed-rate.
4. Add new metrics: `nominal_clean_price_pct`, `nominal_dirty_price_pct`, `inflation_accrued` etc.

*Expected outcome.* +4 to +6 additional rows reconciling on TIPS.

*Effort.* ~1–1.5 hr.

### 2.3  UST FRN with real SOFR forward projection

*Why.* Current FRN reconciliation uses flat-forward (constant index + spread). A real bank would project using SOFR OIS forwards. Tests both libraries' `FloatingRateBond` / `FloatingRateNote` machinery end-to-end.

*How.* Significant work:

1. Bootstrap a USD SOFR forward curve from the historical fixings CSV (already pulled: `reconciliation/sofr_fixings.csv`, 499 rows).
2. Build a SOFR `IborIndex` / `OvernightIndex` on the QL side with the historical fixings loaded.
3. Build a `FloatingRateNote` on the Convex side wired to the same curve.
4. Both sides project coupons using the forward curve.
5. Reconcile discount margin + price.

**This is the largest item here.** Probably a full session on its own. Recommend pairing it with 2.1 (day-count-aware coupons) since both affect FRN pricing.

*Effort.* 4–6 hr.

---

## Tier 3 — Pre-ship validation

### 3.1  Run `cargo clippy --workspace --all-targets -- -D warnings`

*Status.* **Done.** Fixed 6 clippy errors under `-D warnings`:
* *Library code:* `convex-curves/src/bumping/key_rate.rs` (collapsible match arms); `convex-bonds/src/instruments/zero_coupon.rs:824` (`maturity <= issue_date && maturity != issue_date` → `maturity < issue_date`).
* *Bench:* `convex-engine/benches/pricing_benchmarks.rs` — 3 `PricingInput` initializers missing the new `bid_ask_config: None` field (struct had drifted vs. bench).
* *Tool:* `tools/reconcile_bench/src/main.rs` — redundant `u32` casts in `is_end_of_month`.
* *Tests:* `convex-server/tests/{websocket,api}_integration_tests.rs` — `&format!()` → `format!()` (needless_borrows_for_generic_args), `i as i32` drops.

Workspace now passes `cargo clippy --workspace --all-targets -- -D warnings`.

### 3.2  Run `cargo test --workspace --all-targets`

*Status.* **Done.** 1736 pass, 0 fail. Covers integration tests + benches that `--lib` doesn't.

### 3.3  Run `cargo test --workspace --doc`

*Status.* **Done.** 4 pass, 0 fail, 32 ignored.

### 3.4  Re-run the FRED pull

*Status.* **Done but network-blocked.** Re-ran `reconciliation/pull_market_data.py`:
* NY Fed SOFR fixings (499 rows): works ✓
* TreasuryDirect TIPS search: works ✓
* **FRED: still read-times-out** (both root domain and `fredgraph.csv` unreachable from this environment at 90s timeout).

Not a code issue — `fred.stlouisfed.org` is specifically blocked in this session's network. The UST CMT values already in `curves.json` were pulled in an earlier session and remain valid. If re-pulling is needed from a different environment, the script will work as-is; if this environment continues to fail, swap to `requests` or fetch the data from an ALFRED mirror.

### 3.5  Load the Excel add-in and call a UDF

The SafeCall refactor compiled but was never exercised at runtime. Load `excel/Convex.Excel/bin/Release/net472/Convex.Excel64.xll` in Excel, call `CX.BOND.TSY(...)`, `CX.PRICE(...)`, cause an error in one, confirm the error string appears in the cell.

### 3.6  Run `scripts/release.sh` dry-run

*Status.* **Done — release blocker surfaced.** `cargo publish --dry-run --allow-dirty -p <crate>` for the six release crates:

| Crate | dry-run | Note |
|---|---|---|
| convex-core | ✓ | |
| convex-math | ✓ | |
| convex-curves | ✓ | |
| convex-bonds | ✗ | Uses `DayCountConvention::from_str`, absent from published `convex-core@0.11.1` |
| convex-analytics | ✗ | Uses `YieldSolver::solve_primitive` (absent from published `convex-bonds@0.11.1`) + `DayCountConvention::from_str` |
| convex-portfolio | ✓ | |

*Root cause.* Local `convex-bonds` / `convex-analytics` at `version = "0.11.1"` call APIs added on the cleanup branch. Publishing would require every crate to bump (likely to `0.12.0`) so Cargo.toml version constraints pick up the new APIs. This is pre-existing — not a regression from any Tier-1/3 work this session.

*Next action when ready to release.* Bump workspace version to `0.12.0`, re-run this script; should go clean.

### 3.7  BondPricer numerical regression against a known reference book

`BondPricer::yield_to_maturity` was rewritten on the cleanup branch to delegate to `YieldSolver` (fixing the hardcoded-semi-annual bug). That changed numbers for any non-semi-annual bond priced through `BondPricer`. Before tagging, run an internal pricing book through `BondPricer` and confirm the new numbers are what you expect (they should be closer to QL, not further).

---

## Tier 4 — Operational / housekeeping

### 4.1  Resolve CRLF/LF warnings

*Status.* **Done.** Expanded `.gitattributes` with explicit rules:
* LF for `*.rs`, `*.toml`, `*.md`, `*.json`, `*.py`, `*.sh`, `*.yml`/`*.yaml`, `*.css`/`*.js`/`*.ts`/`*.html`, `Cargo.lock`.
* CRLF for `*.cs`, `*.sln`, `*.csproj`, `*.resx`, `*.manifest`, `*.bat`, `*.cmd`, `*.ps1`.
* Binary for `*.xll`, `*.dll`, `*.pdb`, `*.exe`, image types, `*.pdf`, `*.zip`, `*.xlsx`/`*.xls`.

Existing committed files won't change until a `git add --renormalize .`; future commits won't emit CRLF/LF warnings on the listed extensions.

### 4.2  Merge PR #77 (this branch)

The reconciliation branch is clean-merged on top of main. Once any Tier 1 quick wins you want are added, merge into main. The harness + findings files then ship with the code.

### 4.3  CI integration

Add `.github/workflows/reconcile.yml` that runs `cargo run -p reconcile_bench && python reconciliation/ql_bench.py && python reconciliation/reconcile.py` and asserts `exit == 0`. Any future PR that breaks reconciliation fails CI.

### 4.4  INDEX.md / OVERVIEW.md cleanup

Original cleanup review flagged these as emoji-heavy / duplicative with README and CLAUDE.md. I explicitly skipped this because you hadn't okayed deletion. Decide whether to archive them or consolidate.

---

## Tier 5 — User-decision items (don't start without input)

### 5.1  Convex `FixedRateBond::coupon_per_period` design

The current "coupon = rate / freq" behaviour is genuinely correct for UST-style bonds where the schedule gives equal-length periods. Under ACT/360 quarterly it differs from QL by ~0.02 per 100 per quarter. **Which is canonical?** Tier 2.1 assumes Convex should match QL; a quant shop with a different house convention may disagree. Needs a design call.

### 5.2  OAS / tree models for callables

Current reconciliation handles callable bonds via YTC / YTW on workout-bullet proxies (no tree). Real OAS pricing against a Hull-White or Black-Karasinski short-rate model isn't tested. Would require picking a shared model + parameters on both sides. Medium-to-large scope.

### 5.3  Convex `convex-bonds::BondPricer::yield_to_maturity` — formal deprecation?

After the cleanup, `BondPricer::yield_to_maturity` is a thin delegate to `YieldSolver`. It duplicates `FixedRateBond::yield_to_maturity` from the `BondAnalytics` trait. Worth deprecating one of them. Low urgency.

---

## Suggested first session after break

If you have ~1 hour:

1. Tier 1.1 — pull real UK/EU/JP curves. 30 min.
2. Tier 1.2 — fix `ActActIcma::year_fraction` with the period-based form. 30 min.

If you have ~3 hours:

1. Above.
2. Tier 2.1 — day-count-aware coupon_per_period + swap FRN back to ACT/360 and verify reconciliation. 1.5 hr.
3. Tier 3.1 + 3.2 — run clippy + full test suite. 30 min.

If you want to ship the current state:

1. Tier 3.1, 3.2, 3.3 — validation.
2. Tier 4.1 — `.gitattributes` to quiet CRLF/LF noise.
3. Tier 4.2 — merge PR #77.
4. Tier 4.3 — add reconciliation as a CI gate on future PRs.
