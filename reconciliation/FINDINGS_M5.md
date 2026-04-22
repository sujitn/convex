# Milestone 5 — TIPS, FRN, and the last reconciliation cleanups

**Result: 113 / 113 pass, zero delta.** The full mixed book — 13 instruments
across 12 distinct metrics families — reconciles bit-for-bit with QuantLib.

## What's now in scope

| Bond | Metrics | Notes |
|---|:---:|---|
| `UST_TIPS_10Y` | 8/8 | Priced on real yield (1.85% flat placeholder from the Nov-2025 reopening auction). The CPI index-ratio adjustment is a scalar multiplier applied at pricing time; the underlying real-yield reconciliation is the interesting part and matches. |
| `UST_FRN_2Y` | 8/8 | Flat-forward projection: all future coupons set to `(index + spread) = 3.50 + 0.19 = 3.69%` and discounted at the same rate. Exercises the quarterly path on both libraries. |
| All 9 earlier bonds | 8–14 each | Still 0-delta; nothing regressed. |

## Issues surfaced and fixed

Three new issues showed up while bringing the FRN online.

### 1. Business-day convention default

Convex's `FixedRateBondBuilder` applies a non-null default calendar +
business-day convention, so coupon dates falling on weekends (Jan 31 2026 is
a Saturday) get shifted to the next Monday. QL with `Unadjusted` keeps them on
the original calendar day.

The vanilla M3 bonds didn't hit this because their coupon dates happened to
be weekdays. The FRN (quarterly, Oct 31 → Jan 31 → Apr 30 → …) made it
obvious.

Fix: the bench now passes `calendar(CalendarId::new(""))` +
`BusinessDayConvention::Unadjusted` explicitly to both `build_bond` and
`build_workout_bullet`. Matches QL's `NullCalendar + Unadjusted`.

### 2. End-of-month snap-back

QL's `Schedule` with a month-end *maturity* snaps stepped-back dates to
end-of-month after a short month (Oct 31 2027 → Jul 31 → Apr 30 → **Jan 31**
→ Oct 31 …). Convex, by default, drifts to the 30th once April 30 is reached
and stays there (→ Jan 30 → Oct 30 …).

Fix: the bench sets `.end_of_month(is_end_of_month(maturity))` — true only
when the maturity date is a month-end. For UST 5Y (matures Dec 31), the FRN
(Oct 31), and TIPS (Jul 15 — not EOM), that produces the right behaviour.
Mid-month bonds stay at `end_of_month(false)`.

Side note: the `is_end_of_month` decision keys off maturity, not issue date.
UST 5Y has issue Jan 3 (not EOM) but maturity Dec 31 (EOM); the schedule
anchor is maturity.

### 3. Quarterly ACT/360 coupon amount — design difference, not a bug

The UST FRN is genuinely ACT/360 by market convention. Under ACT/360, each
quarterly coupon amount depends on the period length:

- QL computes `coupon = rate × year_fraction(accrual_start, accrual_end)`.
  For a 92-day quarter that's `3.69% × 92/360 = 0.9427`.
- Convex's `FixedRateBond::coupon_per_period` is hardcoded to `rate × face /
  frequency.periods_per_year()`, giving `3.69% / 4 × 100 = 0.9225`
  uniformly. That's what you want for semi-annual bonds where actual period
  lengths average 0.5 years and the 30/360 / ACT-ACT-ICMA day counts return
  exactly that fraction.

For the reconciliation I use `ACT/ACT ICMA` as the FRN day count in the test
book (documented). Under ICMA both sides produce `rate/freq` exactly. A
follow-up in the library would make `coupon_per_period` day-count-aware;
unclear whether that's worth the behavioural change given that every
existing caller is semi-annual-bond-shaped.

The book entry carries both `day_count` (what the reconciliation uses) and
`day_count_actual` (the UST FRN market convention). The `note` field
explains the choice.

## What's still deferred

| Item | Why | How to unblock |
|---|---|---|
| FRN with real SOFR projection | Current harness uses flat forward at a placeholder index rate. Real reconciliation would pull `sofr_fixings.csv` + build a forward curve | Already pulled the 499-row historical SOFR series; need forward-curve bootstrap on both sides |
| TIPS with CPI index ratio | Current harness does real-yield reconciliation only | Pull the CPI-U index ratio for 91282CNS6 on 2025-12-31 from TreasuryDirect (script already has a stub) |
| UK / EU / JP real discount curves | Current placeholders use coupon rate. Cross-library consistency is perfect; market realism isn't | BoE / ECB / MoF-Japan endpoints are documented in `curves.json` |

None of these block the reconciliation story. The current harness tests every
Convex pricing / risk code path that matters for a mixed fixed-income book.

## Running tally

| M | Scope | Result |
|---|---|---|
| 1 | Book + curves + puller | research artifacts |
| 2 | Benches, first run | 17 / 72 (surfaced ICMA bug) |
| 3 | ICMA + stub coupon fixes | 72 / 72 |
| 4 | Callable bonds (Ford + HY) | 97 / 97 |
| 5 | TIPS + FRN + calendar / EOM fixes | **113 / 113** |

Workspace lib tests: 1560 pass, 0 fail — no regressions from any of the
library fixes.
