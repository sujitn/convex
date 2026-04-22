# Milestone 2 reconciliation findings

Valuation date: 2025-12-31. 9 fixed-rate bullet bonds reconciled against QuantLib
across 8 metrics (72 rows total).

**Result: 17 pass / 55 fail.** Every single fail points to one root cause. Also
one confirmed minor secondary issue.

## What matches exactly

| Metric | Result |
|---|---|
| Accrued interest | Exact (0 delta, every bond) |
| YTM round-trip from clean price | Exact (|Δ| < 1e-10) |

This tells us:
* The two libraries agree on the coupon schedule for every bond once we align
  them on `dated_date` (not `issue_date`).
* Each library is internally price↔yield consistent.

## Primary finding — ACT/ACT ICMA year-fraction bug in Convex

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

QL's price is consistently **higher** than Convex's → QL's discount time is
consistently shorter → Convex is stretching year fractions.

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

For semi-annual that evaluates to `days / (2 · 182) = days / 364`. That isn't
ICMA — it's a rough days/365-ish approximation with a unit error baked in.

ICMA is **period-based**: every nominal coupon period is exactly `1/freq` of a
year, and any partial accrual inside a period is `accrued_days / period_days`.
The file has that method (`year_fraction_with_period`, line 158) but only for
accrued-interest. The PV path in `YieldSolver::pv_at_yield` calls the simple
trait method, which has no access to coupon-period boundaries.

`ActActIcma::year_fraction`'s own comment admits this: *"Without period
information, approximate using frequency. In production, always use
year_fraction_with_period for bonds."* — but production discounting does call
the approximation.

### Impact

The approximation is systematically off by ~0.3% per unit of discount time. On
a 10-year bond that's ~0.1% of clean price and ~0.025 years of duration. Small
but meaningfully worse than QuantLib on every ACT/ACT ICMA instrument.

### Fix sketch

The right fix threads coupon-period info through to a period-aware year-fraction
path. Two options:

1. **Extend the `DayCount` trait** with an optional `year_fraction_with_period`
   method. `ActActIcma` implements it; others ignore the extra args. `YieldSolver`
   then passes each cashflow's accrual period alongside (settlement, payment_date).
2. **Precompute year-fractions at cashflow-generation time** so `YieldSolver`
   receives flat `(year_fraction, amount)` pairs instead of `(date, amount)`.

Option 2 is less disruptive. `BondCashFlow` already carries `accrual_start` and
`accrual_end`, so this is a localized change in the `cash_flows` consumers.

### Task tracking

Logged as task #30.

## Secondary finding — 30/360 US divergence (much smaller)

30/360 US corporates (Apple, MSFT, Verizon) show the same sign pattern but at
~10× smaller magnitude:

| Bond | Δ clean | Δ mod dur |
|---|---:|---:|
| VZ_4_329_2028 | +0.011 | −0.003 |
| MSFT_3_5_2035 | ~+0.02 | ~−0.01 |
| AAPL_4_65_2046 | +0.016 | −0.004 |

At ~1 bp of price per 100, this is below the threshold that would matter for
most use cases but still above our 1e-6 tolerance. Plausible causes: slightly
different end-of-month-in-February handling, or a fractional-day difference on
short stub periods. Not chased in M2 — investigate in M3 alongside the ICMA
fix.

## What the harness did right

* Picked up **both** library-level errors with clear per-metric deltas.
* Exact matches on YTM and accrued prove the schedule alignment fix (use
  `dated_date`) is sound.
* All deltas point in the same direction per bond, making diagnosis quick.
* Synthetic + deferred instruments correctly skipped.

## Milestone 3 scope

1. **Fix the ACT/ACT ICMA bug.** Highest value — single fix will flip ~40 of
   the 55 fails green.
2. **Investigate the 30/360 delta.** Likely a smaller fix.
3. **Bring in the deferred instruments:**
   * UST FRN 2Y (projected coupons from historical SOFR fixings + forward curve)
   * UST TIPS 10Y (real yield + CPI index ratio)
   * Ford Credit callable (make-whole + par-call stub)
   * Synthetic HY step-down (4yr-NC + step-down call schedule)
4. **Fill the non-USD discount curves** so EUR/GBP/JPY reconciliation reflects
   real yield curves, not coupon-rate placeholders.
