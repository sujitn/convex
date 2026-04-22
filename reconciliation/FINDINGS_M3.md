# Milestone 3 — first fixes, book fully green

Starting state (end of M2): **17 pass / 55 fail** out of 72 metrics across 9
vanilla fixed-rate bonds.

Ending state: **72 pass / 0 fail**.

Three fixes got us there.

## Fix 1 — period-aware ICMA in PV

`convex-core::daycounts::actact::ActActIcma::year_fraction(start, end)` used a
`days / (freq · round(365/freq))` approximation because the DayCount trait
doesn't know about coupon periods. `YieldSolver::pv_at_yield` and the inline
duration/convexity loops in `convex-analytics::functions` were calling this in
the hot path of every ACT/ACT ICMA bond.

**Approach.** Instead of changing the DayCount trait, I added a
`project_discount_fractions` helper in `convex-analytics::yields::solver` that
takes the full cash-flow list (which already carries `accrual_start` and
`accrual_end`) and computes ISMA-style period-based year fractions:

```text
v     = day_count.day_count(period_start, settlement)
        / day_count.day_count(period_start, period_end)
t_i   = ((i + 1) − v) / freq                      [in years, i = 0, 1, …]
```

Using the day-count's own day-count function for *both* the accrued days and
the period length is what lets the same formula match QuantLib on ACT/ACT
(where `accrued + remaining = period`) *and* on 30/360 US (where the
d1=31→30 rule breaks that identity by one day). Earlier iterations that used
`remaining / period_days` matched ACT/ACT but drifted ~1 bp on 30/360 — traced
to exactly that one-day overlap.

Callsite changes:
* `YieldSolver::solve` and `YieldSolver::dirty_price_from_yield` now route
  through the helper.
* `analytics::functions::{macaulay_duration, convexity}` ditto — they had
  their own inline PV loops with the same bug.

Left for the cleanup branch: the same fix in `convex-bonds::pricing`. The
reconciliation bench doesn't call that path (it goes through analytics), but
both paths should eventually use the helper.

## Fix 2 — short-stub coupon amount

For bonds where settlement falls inside a short first stub (issue after the
nominal period start), `FixedRateBond::cash_flows` was prorating the first
coupon as

```rust
face * coupon_rate * day_count.year_fraction(accrual_start, accrual_end)
```

With the buggy ICMA year fraction, the stub coupon was off by ~0.003 per 100
face. Fix: use the actual ISMA prorata directly,

```rust
coupon_per_period * actual_days / nominal_period_days
```

using the day-count's own day-count for both numerator and denominator. Again,
this keeps 30/360 and ACT/ACT in sync.

JGB #380 was the only bond in the book with a short stub — it went from 3/8
pass to 8/8 after this fix.

## Fix 3 — add missing `dated_date` to UST_30Y in the book

A book-content fix, not a library fix. Without `dated_date`, the harness fell
back to `issue_date` (Nov 17), which doesn't align with the Nov-15 coupons the
schedule implies, causing the schedule-anchor mismatch the earlier commit
already fixed for UST_10Y.

The original sovereign research captured `dated_date` for UST_10Y and
UST_TIPS_10Y but missed it for the 30Y — standard UST convention pins the
dated date to the matching day-of-month of maturity (Nov 15 here).

## What's green now

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

All eight metrics — clean price, dirty price, accrued, YTM, Macaulay
duration, modified duration, convexity, DV01 — reconcile within the
reconciliation report's tolerances (1e-6 on prices, 1e-4 on durations, 1e-7
on DV01 per 100, etc.).

Workspace tests still pass (1560). 

## Still deferred (Milestone 4 scope)

* UST_FRN_2Y — coupon projection from historical SOFR + forward curve
* UST_TIPS_10Y — real coupon + CPI-U index ratio on valuation date
* F_6_798_2028 Ford Credit callable — make-whole + par-call stub pricing
* SYNTH_HY_STEPDOWN_01 — multi-date step-down call schedule
* UK / EU / JP real discount curves (currently coupon-rate placeholders —
  the DE_BUND and UK_GILT reconciliations above are still numerically
  consistent because both sides use the same input, but the *yield* isn't
  market-representative)
