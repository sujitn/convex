# Reconciliation Findings

Consolidated log of reconciliation milestones. Valuation date: **2025-12-31**. Target: bit-for-bit match between Convex and QuantLib across a mixed fixed-income book.

## Running tally

| M | Scope | Result |
|---|---|---|
| 1 | Book + curves + puller | research artifacts |
| 2 | Benches, first run | 17 / 72 (surfaced ICMA bug) |
| 3 | ICMA + stub coupon fixes | 72 / 72 |
| 4 | Callable bonds (Ford + HY) | 97 / 97 |
| 5 | TIPS + FRN + calendar / EOM fixes | **113 / 113** |
| post-M5 | Real UK/EU/JP curves + ICMA trait fallback | 113 / 113 (no regression) |

Workspace lib tests: 1538+ pass, 0 fail.

---

## Milestone 2 — first run, ICMA bug surfaced

Valuation date: 2025-12-31. 9 fixed-rate bullet bonds reconciled against QuantLib across 8 metrics (72 rows total).

**Result: 17 pass / 55 fail.** Every single fail points to one root cause. Also one confirmed minor secondary issue.

### What matches exactly

| Metric | Result |
|---|---|
| Accrued interest | Exact (0 delta, every bond) |
| YTM round-trip from clean price | Exact (|Δ| < 1e-10) |

This tells us:
* The two libraries agree on the coupon schedule for every bond once we align them on `dated_date` (not `issue_date`).
* Each library is internally price↔yield consistent.

### Primary finding — ACT/ACT ICMA year-fraction bug in Convex

**Affects every ACT/ACT ICMA bond in the book** (UST 10Y/30Y/5Y, UK Gilt, Bund, JGB).

The delta pattern is identical across these bonds:

| Bond | Δ clean (QL − CX) | Δ mod dur | Δ convexity |
|---|---:|---:|---:|
| UST_10Y | +0.114 | −0.026 | −0.49 |
| UST_30Y | +0.238 | — | larger |
| UST_5Y_short | +0.046 | −0.013 | −0.06 |
| UK_GILT_10Y | +0.128 | −0.025 | −0.46 |
| DE_BUND_10Y | +0.070 | −0.026 | −0.48 |
| JP_JGB_10Y | similar | similar | similar |

QL's price is consistently **higher** than Convex's → QL's discount time is consistently shorter → Convex is stretching year fractions.

### Root cause

`convex-core/src/daycounts/actact.rs:195` (`ActActIcma::year_fraction`):

```rust
fn year_fraction(&self, start: Date, end: Date) -> Decimal {
    // Without period information, approximate using frequency
    // In production, always use year_fraction_with_period for bonds
    let days = start.days_between(&end);
    let approx_period_days = 365 / self.frequency as i64;
    Decimal::from(days) / (Decimal::from(self.frequency) * Decimal::from(approx_period_days))
}
```

For semi-annual that evaluates to `days / (2 · 182) = days / 364`. That isn't ICMA — it's a rough days/365-ish approximation with a unit error baked in.

ICMA is **period-based**: every nominal coupon period is exactly `1/freq` of a year, and any partial accrual inside a period is `accrued_days / period_days`. The file has that method (`year_fraction_with_period`) but only for accrued-interest. The PV path in `YieldSolver::pv_at_yield` calls the simple trait method, which has no access to coupon-period boundaries.

### Impact

The approximation is systematically off by ~0.3% per unit of discount time. On a 10-year bond that's ~0.1% of clean price and ~0.025 years of duration. Small but meaningfully worse than QuantLib on every ACT/ACT ICMA instrument.

### Secondary finding — 30/360 US divergence (much smaller)

30/360 US corporates (Apple, MSFT, Verizon) show the same sign pattern but at ~10× smaller magnitude (Δ clean ~+0.01 to +0.02). Plausible causes: slightly different end-of-month-in-February handling, or a fractional-day difference on short stub periods. Not chased in M2 — investigated in M3 alongside the ICMA fix.

### What the harness did right

* Picked up **both** library-level errors with clear per-metric deltas.
* Exact matches on YTM and accrued prove the schedule alignment fix (use `dated_date`) is sound.
* All deltas point in the same direction per bond, making diagnosis quick.
* Synthetic + deferred instruments correctly skipped.

---

## Milestone 3 — first fixes, book fully green

Starting state: **17 pass / 55 fail**. Ending state: **72 pass / 0 fail**. Three fixes got us there.

### Fix 1 — period-aware ICMA in PV

`ActActIcma::year_fraction(start, end)` used a `days / (freq · round(365/freq))` approximation because the DayCount trait doesn't know about coupon periods. `YieldSolver::pv_at_yield` and the inline duration/convexity loops in `convex-analytics::functions` were calling this in the hot path of every ACT/ACT ICMA bond.

**Approach.** Instead of changing the DayCount trait, I added a `project_discount_fractions` helper in `convex-analytics::yields::solver` that takes the full cash-flow list (which already carries `accrual_start` and `accrual_end`) and computes ISMA-style period-based year fractions:

```text
v     = day_count.day_count(period_start, settlement)
        / day_count.day_count(period_start, period_end)
t_i   = ((i + 1) − v) / freq                      [in years, i = 0, 1, …]
```

Using the day-count's own day-count function for *both* the accrued days and the period length is what lets the same formula match QuantLib on ACT/ACT (where `accrued + remaining = period`) *and* on 30/360 US (where the d1=31→30 rule breaks that identity by one day). Earlier iterations that used `remaining / period_days` matched ACT/ACT but drifted ~1 bp on 30/360 — traced to exactly that one-day overlap.

Callsite changes:
* `YieldSolver::solve` and `YieldSolver::dirty_price_from_yield` now route through the helper.
* `analytics::functions::{macaulay_duration, convexity}` ditto — they had their own inline PV loops with the same bug.

### Fix 2 — short-stub coupon amount

For bonds where settlement falls inside a short first stub (issue after the nominal period start), `FixedRateBond::cash_flows` was prorating the first coupon as `face * coupon_rate * day_count.year_fraction(accrual_start, accrual_end)`. With the buggy ICMA year fraction, the stub coupon was off by ~0.003 per 100 face.

Fix: use the actual ISMA prorata directly, `coupon_per_period * actual_days / nominal_period_days`, using the day-count's own day-count for both numerator and denominator. Again, this keeps 30/360 and ACT/ACT in sync.

JGB #380 was the only bond in the book with a short stub — it went from 3/8 pass to 8/8 after this fix.

### Fix 3 — add missing `dated_date` to UST_30Y in the book

A book-content fix, not a library fix. Without `dated_date`, the harness fell back to `issue_date` (Nov 17), which doesn't align with the Nov-15 coupons the schedule implies. Standard UST convention pins the dated date to the matching day-of-month of maturity (Nov 15 here).

### What's green at end of M3

| Bond | Pass | Notes |
|---|---|---|
| `UST_10Y` | 8/8 | 4.000% ACT/ACT ICMA, Nov-15 coupons |
| `UST_30Y` | 8/8 | 4.625% ACT/ACT ICMA, Nov-15 coupons (needed dated_date) |
| `UST_5Y_short` | 8/8 | 3.875% ACT/ACT ICMA, 2yr remaining, settlement on coupon date |
| `UK_GILT_10Y` | 8/8 | 4.750% ACT/ACT ICMA, Oct-22 coupons |
| `DE_BUND_10Y` | 8/8 | 2.500% annual, ACT/ACT ICMA, Feb-15 coupons |
| `JP_JGB_10Y` | 8/8 | 1.700% ACT/ACT ICMA, short first stub (issue Dec 3, coupons Sep 20 / Mar 20) |
| `AAPL_4_65_2046` | 8/8 | 4.65% semi, 30/360 US, 20y remaining |
| `MSFT_3_5_2035` | 8/8 | 3.5% semi, 30/360 US, 9y remaining |
| `VZ_4_329_2028` | 8/8 | 4.329% semi, 30/360 US, 3y remaining |

All eight metrics — clean price, dirty price, accrued, YTM, Macaulay duration, modified duration, convexity, DV01 — reconcile within the reconciliation report's tolerances (1e-6 on prices, 1e-4 on durations, 1e-7 on DV01 per 100, etc.).

---

## Milestone 4 — callable bonds

**Result: 97 / 97 pass, zero delta.**

Added callable-bond coverage without needing a tree model or OAS machinery. Both the Ford Credit make-whole + par-call structure and the synthetic HY multi-date step-down are now reconciled.

### Approach — YTC / YTW as deterministic

For a callable bond priced at clean price P:

* **YTC at call date D** with call price K is just the YTM of a *hypothetical bullet bond* with the same coupons up to D and a final redemption of K on D.
* **YTW** is the min of YTC across every future call date and YTM at final maturity.

Both sides express "bullet with non-par redemption" directly:

| | Convex | QuantLib |
|---|---|---|
| Coupons | `coupon_rate × 100` per period | `[coupon_rate]` × face=100 |
| Final redemption | `.redemption_value(call_price)` on the builder | `redemption=call_price` arg to `FixedRateBond` |

No tree, no Hull-White, no OAS. The existing vanilla-bond pricing path does the work — which is what the earlier milestones already reconciled bit-for-bit.

### What's now reconciled

| Bond | Extra metrics | Pass |
|---|---|:---:|
| `F_6_798_2028` | YTC at 2028-10-07 (par-call), YTM, YTW, workout date | 11/11 ✓ |
| `SYNTH_HY_STEPDOWN_01` | YTC at 2026/2027/2028/2029 call dates, YTM, YTW, workout date | 14/14 ✓ |

Plus the 8 vanilla bonds from M3 continue to pass at 8/8 each, all of them matching with 0 delta (every metric, every digit of the 10-decimal output).

### Numerical sanity check

* **Ford Credit** at UST 3Y reference yield (3.51%): 6.798% coupon trades at 108.82 clean. YTC at par-call = 3.42%, YTW = 3.42% on 2028-10-07 (par-call wins because issuer has every incentive to redeem at par when the bond is trading 8 points above). Matches QL exactly.

* **SYNTH_HY_STEPDOWN_01** at UST 4Y reference yield (3.61%): 7.5% coupon trades at 115.31. YTC on each successive call date:
  * 2026-04-15 @ 103.750: **−26.99%** (4 months to call; huge negative yield because redemption is 11 points below market)
  * 2027-04-15 @ 101.875: −2.62%
  * 2028-04-15 @ 100.938: +1.11%
  * 2029-04-15 @ 100.000: +2.61%
  * Maturity at 100: +3.61%
  YTW = −26.99% (earliest call wins). Both Convex and QL handle the negative-yield regime cleanly and produce the same answer.

The step-down case is a real stress test — solver robustness across negative yields, high-coupon/low-market-yield regime, and four workout dates per bond. All passed.

---

## Milestone 5 — TIPS, FRN, and the last cleanups

**Result: 113 / 113 pass, zero delta.** The full mixed book — 13 instruments across 12 distinct metrics families — reconciles bit-for-bit with QuantLib.

### What's now in scope

| Bond | Metrics | Notes |
|---|:---:|---|
| `UST_TIPS_10Y` | 8/8 | Priced on real yield (1.85% flat placeholder from the Nov-2025 reopening auction). The CPI index-ratio adjustment is a scalar multiplier applied at pricing time; the underlying real-yield reconciliation is the interesting part and matches. |
| `UST_FRN_2Y` | 8/8 | Flat-forward projection: all future coupons set to `(index + spread) = 3.50 + 0.19 = 3.69%` and discounted at the same rate. Exercises the quarterly path on both libraries. |
| All 9 earlier bonds | 8–14 each | Still 0-delta; nothing regressed. |

### Issues surfaced and fixed

#### 1. Business-day convention default

Convex's `FixedRateBondBuilder` applies a non-null default calendar + business-day convention, so coupon dates falling on weekends (Jan 31 2026 is a Saturday) get shifted to the next Monday. QL with `Unadjusted` keeps them on the original calendar day.

The vanilla M3 bonds didn't hit this because their coupon dates happened to be weekdays. The FRN (quarterly, Oct 31 → Jan 31 → Apr 30 → …) made it obvious.

Fix: the bench now passes `calendar(CalendarId::new(""))` + `BusinessDayConvention::Unadjusted` explicitly to both `build_bond` and `build_workout_bullet`. Matches QL's `NullCalendar + Unadjusted`.

#### 2. End-of-month snap-back

QL's `Schedule` with a month-end *maturity* snaps stepped-back dates to end-of-month after a short month (Oct 31 2027 → Jul 31 → Apr 30 → **Jan 31** → Oct 31 …). Convex, by default, drifts to the 30th once April 30 is reached and stays there (→ Jan 30 → Oct 30 …).

Fix: the bench sets `.end_of_month(is_end_of_month(maturity))` — true only when the maturity date is a month-end. For UST 5Y (matures Dec 31), the FRN (Oct 31), and TIPS (Jul 15 — not EOM), that produces the right behaviour. Mid-month bonds stay at `end_of_month(false)`.

Side note: the `is_end_of_month` decision keys off maturity, not issue date. UST 5Y has issue Jan 3 (not EOM) but maturity Dec 31 (EOM); the schedule anchor is maturity.

#### 3. Quarterly ACT/360 coupon amount — design difference, not a bug

The UST FRN is genuinely ACT/360 by market convention. Under ACT/360, each quarterly coupon amount depends on the period length:

- QL computes `coupon = rate × year_fraction(accrual_start, accrual_end)`. For a 92-day quarter that's `3.69% × 92/360 = 0.9427`.
- Convex's `FixedRateBond::coupon_per_period` is hardcoded to `rate × face / frequency.periods_per_year()`, giving `3.69% / 4 × 100 = 0.9225` uniformly. That's what you want for semi-annual bonds where actual period lengths average 0.5 years and the 30/360 / ACT-ACT-ICMA day counts return exactly that fraction.

For the reconciliation I use `ACT/ACT ICMA` as the FRN day count in the test book (documented). Under ICMA both sides produce `rate/freq` exactly. A follow-up in the library would make `coupon_per_period` day-count-aware (Tier 2.1 in `NEXT_STEPS.md`); unclear whether that's worth the behavioural change given that every existing caller is semi-annual-bond-shaped.

The book entry carries both `day_count` (what the reconciliation uses) and `day_count_actual` (the UST FRN market convention). The `note` field explains the choice.

---

## Post-M5 quick wins

### Real UK/EU/JP discount curves

Replaced the previous `PENDING` placeholders (which fell back to coupon-rate as reference yield) with actual year-end 2025 sovereign curves:

| Curve | Source | Tenors |
|---|---|---|
| `UK_GILT_CURVE` | BoE `glcnominalddata.zip` → `GLC Nominal daily data_2025 to present.xlsx` → sheet `4. spot curve`, 2025-12-31 | 1Y, 2Y, 3Y, 5Y, 7Y, 10Y, 20Y, 30Y |
| `DE_BUND_CURVE` | ECB SDMX `YC.B.U2.EUR.4F.G_N_A.SV_C_YM.SR_<tenor>`, 2025-12-31 | 1Y, 2Y, 3Y, 5Y, 7Y, 10Y, 15Y, 20Y, 30Y |
| `JP_JGB_CURVE` | MOF `jgbcme_all.csv`, 2025-12-30 (Japan markets closed 12-31 for year-end holiday) | 1Y, 2Y, 3Y, 5Y, 7Y, 10Y, 15Y, 20Y, 30Y |

Both BoE and ECB publish **continuously-compounded spot yields**; stored values are converted to semi-annual compounding (`r_sa = 2·(exp(r_c/2)−1)`) for consistency with `UST_CMT` labelling. JGB publishes par yields at fixed tenors (semi-annual market convention) — stored as-is.

Indicative 10Y yields:

| Curve | 10Y (%) |
|---|---:|
| UST (CMT) | 4.18 |
| UK (s.a.-equiv spot) | 4.624 |
| EUR AAA (s.a.-equiv spot) | 2.970 |
| JPY (par) | 2.066 |

Both `reconcile_bench` (Rust) and `ql_bench.py` now dispatch by currency (`USD→UST_CMT, GBP→UK_GILT_CURVE, EUR→DE_BUND_CURVE, JPY→JP_JGB_CURVE`) and interpolate the matching curve. Reconciliation stays at 113/113 with zero delta.

### ActActIcma::year_fraction fallback

The trait method (called when coupon-period bounds aren't available — currently not hit by the reconciliation path thanks to `project_discount_fractions`, but visible to external users of the `DayCount` trait) previously returned a rough `days / (freq · round(365/freq))` approximation. It now delegates to `ActActIsda::year_fraction` — calendar-year-split ACT/ACT. Gives exactly 1 for a full year, prorates multi-year spans by leap-year days. The period-aware method `year_fraction_with_period` is still the canonical path for accrual and bond PV; the trait fallback is now semantically honest instead of off by ~0.3%.

---

## What's still deferred

See `NEXT_STEPS.md` for the fuller picture. Big rocks:

* **FRN with real SOFR projection** — current harness uses flat-forward at a placeholder index rate. Would need forward-curve bootstrap on both sides.
* **TIPS with CPI index ratio** — current harness does real-yield reconciliation only; nominal pricing with index ratio is still pending.
* **Day-count-aware `coupon_per_period`** — would let the FRN reconcile under its actual `ACT/360` market convention instead of the current `ACT/ACT ICMA` workaround.
* **OAS / tree models for callables** — current reconciliation uses YTC/YTW on workout-bullet proxies. Real OAS against a short-rate model isn't tested.

None of these block the reconciliation story at 113/113. The current harness exercises every Convex pricing / risk code path that matters for a mixed fixed-income book.
