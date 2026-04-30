# Reconciliation Findings

Convex ↔ QuantLib 1.40 fixed-income book. Valuation date **2025-12-31**.
Target: bit-for-bit match (tolerances in `reconcile.py`) across 14
instruments and 4–14 metrics each.

| M | Result | Scope |
|---|---|---|
| 2 | 17 / 72 | 9 vanilla bonds; exposed the ICMA discount bug |
| 3 | 72 / 72 | ICMA + short-stub fixes |
| 4 | 97 / 97 | Callable bonds via workout-bullet proxies |
| 5 | 113 / 113 | + TIPS real-yield, FRN flat-forward, calendar/EOM fixes |
| post-M5 | 117 / 117 | + real UK/EU/JP curves + TIPS nominal pricing (Tier 2.2) |
| post-M5 | 117 / 117 | FRN flipped to real ACT/360; day-count-aware coupon/accrued/PV (Tier 2.1) |
| post-M5 | 121 / 121 | + corporate SOFR FRN on SOFR OIS zero curve (Tier 2.3) |
| A2 | 149 / 149 | + UST 52-week Bill (real CUSIP 912797TC1) via ZeroCouponBond |
| A1 | 157 / 157 | + plain sinker; surfaced two bugs in factor-adjusted pricing |
| A3 | 161 / 161 | + Ford Credit make-whole redemption (verified +35bp spread) |
| A4 | 174 / 174 | + Bermudan puttable; YTP/YTB workout-bullet path |
| A4 cleanup | 172 / 172 | trimmed 4→2 MW scenarios; refactored 8-metric duplication |
| A polish | 172 / 172 | A1.1, A3.1, A3.2, A4.1 — no new bond shapes, removes Excel-side gaps |
| B1 | 178 / 178 | + callable SOFR FRN (synthetic); workout-bullet DM-to-first-call |
| B1 polish | 188 / 188 | re-anchor to 2024-11-15, ARRC × workout-bullet on both snapshots, per-call DM + DM-to-worst |

Workspace lib + integration tests pass, 0 fail. Clippy clean.

## M2 — ICMA year-fraction in Convex

ACT/ACT ICMA bonds (UST 10Y/30Y/5Y, Gilt, Bund, JGB) all mispriced with
an identical sign pattern. `ActActIcma::year_fraction(start, end)` used
`days / (freq · round(365/freq))` — semi-annual gives `days / 364`, not
ICMA. ICMA is period-based: nominal period is exactly `1/freq`, partial
accrual is `accrued_days / period_days`. The file had the correct
`year_fraction_with_period` method, but `YieldSolver::pv_at_yield` and
the analytics duration/convexity loops all called the bare trait method
with no period context.

YTM round-trip and accrued matched exactly — schedule alignment and
internal consistency were fine; the bug was isolated to year-fraction.

## M3 — three fixes, book green

1. **Period-aware PV.** Added `project_discount_fractions` in
   `convex-bonds::pricing::yield_solver`, reads `accrual_start`/`accrual_end`
   from each cashflow and computes the ISMA `v / t_i` formula. Using the
   day-count's own `day_count()` for both accrued and period length is
   what keeps 30/360 and ACT/ACT in sync — earlier drafts that used
   calendar days broke 30/360 by the d1=31→30 rule's one-day overlap.
   Callers updated: `YieldSolver::solve`, `YieldSolver::dirty_price_from_yield`,
   `analytics::functions::{macaulay_duration, convexity}`.

2. **Short-stub coupon amount.** `FixedRateBond::cash_flows` prorated
   stub coupons using the buggy ICMA year fraction. Fix: `coupon_per_period
   * actual_days / nominal_period_days` using day-count native days.
   JGB #380 (the only stub bond) went from 3/8 → 8/8.

3. **Book fix.** UST_30Y was missing `dated_date`; fallback to
   `issue_date` (Nov 17) didn't align with the Nov-15 coupon anchor.

## M4 — callables without a tree

YTC at call date D with call price K = YTM of a hypothetical bullet
with the same coupons up to D and redemption K. YTW = min over
YTC-at-each-call plus YTM-at-maturity. Both libraries expressed this
directly via `.redemption_value(K)` / QL's `redemption=` arg. No
Hull-White, no OAS.

Ford Credit (par-call): YTC = 3.42%, YTW = 3.42% on 2028-10-07 (issuer
has every incentive to call at par when trading 8pt above).
SYNTH_HY_STEPDOWN_01: YTC = −26.99% on 2026-04-15 (4 months to call, 11pt
below market). Both sides handle the negative-yield regime and match
bit-for-bit.

## M5 — TIPS + FRN + scheduling edge cases

1. **Business-day convention.** Convex's builder defaulted to a non-null
   calendar, shifted Saturday coupons (Jan 31 2026) to Monday. QL with
   `Unadjusted` doesn't. Vanilla bonds didn't hit this; the quarterly
   FRN made it obvious. Bench now passes `CalendarId::new("")` +
   `BusinessDayConvention::Unadjusted` explicitly, matching QL's
   `NullCalendar + Unadjusted`.

2. **End-of-month snap-back.** QL's `Schedule` with a month-end maturity
   snaps stepped-back dates to month-end after short months (Oct 31 →
   Jul 31 → Apr 30 → Jan 31 …). Convex defaults to drifting to the 30th.
   Fix: `.end_of_month(is_end_of_month(maturity))` — true only for
   month-end maturities.

3. **Quarterly ACT/360 coupon amount — design difference, not a bug.**
   QL: `coupon = rate × year_fraction(accrual_start, accrual_end)` — for
   a 92-day quarter, `3.69% × 92/360 = 0.9427`. Convex's
   `coupon_per_period` is `rate × face / freq`, giving `0.9225` flat.
   For semi-annual 30/360 / ACT-ACT ICMA the two agree; for quarterly
   ACT/360 they don't. UST FRN book entry uses ACT/ACT ICMA as a
   documented workaround. See NEXT_STEPS 2.1 for the design question.

## Post-M5 quick wins

* **Real sovereign curves** (UK/EU/JP). `curves.json` + `SOURCES.md` + the
  three pullers in `pull_market_data.py`. Previously used coupon-rate as
  reference yield — cross-library consistent but not market-realistic.
  Reconciliation stayed at 113/113; 10Y reference yields now read UK
  4.62%, Bund 2.97%, JGB 2.07%.

* **`ActActIcma::year_fraction` trait fallback.** Was `days / (freq·round(365/freq))`
  — off by ~0.3% per year unit. Now delegates to `ActActIsda`: exactly 1
  for a full year, calendar-year split for multi-year. Period-aware
  path (`year_fraction_with_period`) unchanged.

* **Clippy clean under `-D warnings`.** Two library simplifications
  (zero_coupon.rs double-comparison; key_rate.rs collapsible match); a
  bench that had drifted from its struct (`PricingInput.bid_ask_config`);
  `&format!` and `i as i32` cleanups in integration tests.

## A2 — Zero-coupon via the Bond trait

Real instrument: UST 52-week Bill (CUSIP 912797TC1, 2025-12-26 → 2026-12-24).

Architectural finding: the analytics surface is `&dyn Bond`, not
`&dyn FixedCouponBond`. `ZeroCouponBond::cash_flows(settle)` returns a
single principal CF; `project_discount_fractions` falls through its raw
`day_count.year_fraction(settle, maturity)` branch, and the pricing
math reduces to `face / (1 + y/m)^(years × m)` — exactly the closed
form. A `FixedCouponBond` impl would have been wrong (zeros have no
coupon); a parity test in `zero_coupon.rs` pins the equivalence.

## A1 — Sinking-fund bond, two real bugs

Synthetic 10Y plain sinker (5 annual paydowns of 20%) reconciled
against `ql.AmortizingFixedRateBond`. Two bugs surfaced and fixed.

1. **`SinkingFundBond::cash_flows_to_date` used post-paydown factor.**
   Coupon at sink date `D` accrued on the factor *after* the paydown
   instead of the principal outstanding during the prior period.
   Result: every December coupon was understated by 20% of the period
   coupon, and the maturity coupon (factor=0 after final sink) was
   silently dropped. Fix: snapshot `pre_paydown_factor` before applying
   the sink. Regression test
   `test_sinker_coupon_uses_pre_paydown_factor`.

2. **`project_discount_fractions` used iterator-position period index.**
   The ICMA period-aware time-to-CF formula `(i+1 − v) / freq` was
   indexed by enumerator position. For sinkers (and any shape with
   multiple CFs on the same date), the second CF on a shared date got
   shifted forward by one full period. Fix: advance the period index
   only on date change. For one-CF-per-period bonds the formula is
   unchanged. Off-cycle sink dates (between coupon dates) are not yet
   handled — flagged with TODO and listed in NEXT_STEPS.

## A3 — Make-whole call redemption

Ford Credit 6.798% '28 (verified +35bp spread from 424B2). Reconciled
the MW PV formula at `(call_date, treasury_rate)` scenarios on both
sides: ITM (UST 3% → MW well above par) and near-ATM (UST 5% ≈
coupon → MW close to par). Convention: discount at `treasury + spread`
using ACT/365F time × bond frequency, floored at the first call entry
price. ACT/365F is what `CallableBond::make_whole_call_price` already
uses; mirrored on the QL side hand-rolled. Note: real US-corp 424B
typically uses the bond's own day-count for MW discount — flagged as
a convention gap in NEXT_STEPS.

The library function (`CallableBond::make_whole_call_price`) is not
exposed through the `convex_price` FFI RPC or any Excel UDF. So Excel
sheets pricing Ford Credit today still ignore make-whole. Listed as a
production gap in NEXT_STEPS.

## A4 — Bermudan puttable

Synthetic 5Y annual-put bond. The library already had `PutType`,
`PutEntry`, `PutSchedule`, and `CallableBond.with_put_schedule(...)`.
The bench composes a `CallableBond` with empty call schedule + put
schedule — fragile (relies on "no entries → not callable" being a
permanent invariant). Tracked in NEXT_STEPS as a refactor candidate
(either `PutableBond` or `Optional<CallSchedule>` on `CallableBond`).

YTP per put date computed via the same workout-bullet trick used for
callable YTC pre-OAS (M4): build a hypothetical bullet maturing at the
put date with redemption = put price; YTM on that bullet = YTP. YTB
(best for holder) = max yield over YTM and all YTPs. The synthetic
trades at premium so puts are OTM (YTB = YTM); the negative YTP at the
14-day-out put still surfaces — both sides reach the same number,
exercising the workout-bullet solver in the negative-yield regime.

## A polish — close out the Tier A items

Four items called out in NEXT_STEPS were closed without new bench scope:

* **A1.1 Off-cycle sinker dates.** `project_discount_fractions` now derives
  the period index from each cf's `accrual_end` and adds a fractional
  in-period offset for CFs strictly inside their accrual window. On-cycle
  bonds collapse to the canonical `(i+1-v)/freq`, so existing
  reconciliation is unchanged. Regression test in `yield_solver.rs`.
* **A3.1 Wire MW through FFI + Excel.** New `convex_make_whole` C symbol,
  `MakeWholeRequest`/`MakeWholeResponse` DTOs, dispatcher, schema entries,
  MCP `make_whole_call_price` tool, Excel `CX.MW(bond, call_date,
  treasury_rate, [field])` UDF + `make_whole_spread_bps` field on
  `CallableSpec`. Three FFI smoke tests pin the round trip.
* **A3.2 MW discount uses bond day-count.** `CallableBond::make_whole_call_price`
  now reads the bond's own day count for the discount-time year fraction
  (matches US-corp 424B2 — 30/360 US for AAPL/MSFT/Verizon/Ford). The QL
  mirror in `make_whole_redemption_qq` was switched in lockstep, so the
  Ford MW reconciliation continues to match bit-for-bit.
* **A4.1 Optional<CallSchedule> on CallableBond.** `call_schedule` is now
  `Option<CallSchedule>`; new `CallableBond::new_putable` constructor for
  put-only bonds. The bench's puttable build no longer relies on the
  empty-call-schedule "no entries → not callable" invariant. `call_type()`
  becomes `Option<CallType>`.

## B1 — Callable FRN (workout-bullet DM)

New `CallableFloatingRateNote` (composition of FRN + CallSchedule) with
two analytic surfaces on `DiscountMarginCalculator`:

* `calculate_to_workout(frn, dirty, settlement, workout_date, call_price)`
  — solve for DM such that the truncated-cash-flow PV at curve+DM equals
  the target dirty price.
* `discount_margin_to_worst(cfrn, dirty, settlement)` — minimum DM across
  DM-to-each-call-date and DM-to-maturity; mirror of M4's YTC/YTW workout
  bullet adapted for FRNs.

Two pre-existing bugs in `DiscountMarginCalculator::price_with_dm`
surfaced and were fixed:

1. **Spread double-count.** The calculator passed `simple_fwd +
   quoted_spread` to `frn.effective_rate(...)`, which already adds the
   bond's `spread_decimal()`. Fix: pass just the projected forward index
   rate; let `effective_rate` add the spread (and apply any cap/floor).
2. **Day-count mismatch in forward projection.** `simple_forward_period`
   was called with ACT/365 spans (`days/365`) while the coupon was applied
   with the bond's own ACT/360 yf, silently inflating each projected
   coupon by `yf_360 / span_365 ≈ 1.0139`. Fix: compute the forward
   directly as `(DF(start)/DF(end) - 1) / yf_bond_dc` so the discount span
   matches the day count the rate is being applied with.
3. **Solver rounding.** DM was rounded to whole bps inside the solver
   (`(root * 1e4).round()`). Removed; callers can round if they want.

Reconciled instrument: `SYNTH_CALLABLE_SOFR_FRN` — a synthetic 5Y
quarterly SOFR + 125bps callable on annual anniversaries. The QL side
runs a hand-rolled workout-bullet PV solver (`dm_to_workout_qq`); the
Rust side drives `DiscountMarginCalculator::calculate_to_workout`.
See "B1 polish" for the in-progress ARRC mechanics.

## B1 polish

Re-anchored `SYNTH_CALLABLE_SOFR_FRN` to mid-month (2024-11-15 → 2029-11-15)
so both snapshots sit mid-period; added per-call DM rows and DM-to-worst
mirroring the puttable's YTW pattern. `DiscountMarginCalculator` grew an
optional in-progress coupon override so the bench supplies an ARRC
`compound_in_arrears` closure and reuses the library's workout-bullet PV
instead of duplicating it. +10 rows; 178 → 188.

## Deferred

See `NEXT_STEPS.md`. Biggest rocks:

* **FRN with real SOFR projection** — bootstrap forward curve on both sides.
* **TIPS nominal pricing with live CPI index ratio** (Tier 2.2).
* **Day-count-aware `coupon_per_period`** — would let FRN reconcile under
  real ACT/360 instead of the ICMA workaround.
* **OAS / tree models for callables** — current uses deterministic
  workout-bullet YTC/YTW.
