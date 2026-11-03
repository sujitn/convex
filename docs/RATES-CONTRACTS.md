# IRS and OIS contracts

The `convex-rates` crate, exposed as `convex::rates`, describes interest rate
swaps and overnight index swaps as dated contracts. `SwapTerms::build` validates
the economics you pass in and caches the fixed and floating coupon schedules,
term-reset slices and overnight observation weights. `SwapContract::cashflows`
returns the expected signed coupon cash with fixing and projection provenance.

Valuation is in [swap valuation](SWAP-VALUATION.md): discounted PV, fair rate or
spread, accrued, and multi-curve calibration. This document covers contract
construction and expected cash only.

Each function validates its input and returns an error instead of guessing. No
engine, server, database, message queue, provider, global evaluation date or
clock is needed.

The existing tenor-only `convex_curves::Swap/Ois` helpers and the hedge advisor's
`InterestRateSwap` keep their own contracts. They are not aliases for these dated
cash contracts.

## Construction and units

`SwapTerms` keeps schema version 1, the trade ID and version, the cash currency,
the pay-fixed or receive-fixed direction, unadjusted effective and maturity dates,
independent leg economics and the collateral mapping. It is an economic input. It
is not a book, ledger, counterparty record or trade event.

| Input | Meaning and validation |
|---|---|
| Fixed leg | Independent frequency, stub/EOM, first/penultimate dates, day count, accrual/payment rules, dated rate and notional profiles. |
| Term floating leg | Independent coupon and reset grids, index ID and 1-120 month tenor, reset and fixing calendars, nonpositive fixing offset, fixing adjustment, explicit index quotation interval, separate spread/gearing/notional profiles. |
| Overnight leg | Index, observation calendar, ACT/360 or ACT/365 Fixed, explicit business-day lookback, observation shift and lockout, simple spread and compounded-interest gearing. |
| Calendar pins | Each resource has a provider-neutral `ReferenceDataId` and a positive immutable `Version`; `CalendarBinding` must match exactly. The caller binds the calendar content to its pin. Matching labels does not prove the content is right. |
| Curves | Separate exact `CurveKey { id, version }` projection and collateral discount dependencies. Forecasts must identify the requested projection version. The pricer, not an index preset, validates curve role, currency and query. |
| Collateral | Agreement pin, cash/collateral currency and explicit discount curve. Only same-currency collateral is supported. |
| Rates/spreads | Annualized decimals: 0.05 is 5%, 0.001 is 10 bp. Index rates and coupon/spread inputs are within ±10. |
| Gearing | Dimensionless multiplier with magnitude ≤1e6. |
| Notional | Absolute unsigned principal in 0..1e18; the initial amount must be positive. Trade direction is separate. Increases and reductions change interest exposure and create no principal exchanges. |
| Rounding | Explicit core `CashRounding`, applied once after all coupon slices; unrounded amounts are also returned. |

Every dated profile must be sorted, contiguous and cover exactly that leg's
adjusted accrual span. Internal dates may differ across rate, spread, gearing and
notional. A value takes effect at its inclusive start, and coupon interest uses
the outstanding principal in each half-open segment. Fixed and floating leg
notionals may differ. No flat average replaces a step. Negative rates and cash
are kept under `Allow`, and `Reject` returns an error if an effective coupon
segment is negative. No cap or floor is inferred.

Raw economic terms use strict Serde. All profile Decimal values are **strings**,
which keeps exact precision with the workspace serde-float feature enabled.
Unknown payoff fields, floating kinds and excess input fields fail. Deserialize
the terms, then rebuild with the pinned calendars. `SwapContract` cannot be
deserialized into unchecked cached plans. This crate adds no generated JSON
schema and no v1 JSON/C ABI/Java/Excel/MCP/WASM construction or dispatch.

## Dates and coupon mathematics

The core generator has `generate_with_calendars`, which takes distinct accrual,
payment, fixing, observation and ex-date calendars. `generate` delegates to it
with its single calendar and keeps its behavior. Swap coupon terms forbid bond
ex-coupon fields and implicit fixing or observation offsets, because floating
terms own those independent dates.

Accrual adjustments affect accrual boundaries only. Payment delay counts from the
**unadjusted coupon end**, excluding the starting date for business-day lags.
The payment adjustment is then applied on the payment calendar. A swap payment
must be on or after the accrual end, and payments must not run backward.

This lag anchor is not every market contract's rule. Other anchors need a
separately represented contract. Stubs keep the core regular reference periods,
and ICMA long stubs split at those periods. Contractual 30E/360 ISDA uses the
adjusted leg termination date in the shared core.

Term resets cover the whole floating coupon accrual span. Each intersection keeps
the coupon-slice start and end, the fixing date, the full index interval and the
quotation day count. The fixing offset counts from the adjusted reset start on
the fixing calendar, followed by the fixing adjustment. An observation after the
reset start is unsupported without an in-arrears or convexity model.

`TermRatePeriod::ResetInterval` contracts a rate over the reset interval,
including a stub. Index-tenor-only settings must be neutral (false/Unadjusted).
`IndexTenor` uses the full calendar-month tenor you supply, from the adjusted
reset start, with explicit EOM and end adjustment on the reset calendar. Coupon
accrual may be shorter than that quotation period. No interpolation between two
index tenors is invented. Contextual ICMA and 30E ISDA **index quotation** are
unsupported. Coupon day counts still use the shared context.

Cash is computed as follows:

- Fixed cash sums `N * fixed_rate * coupon_year_fraction` on each profile segment.
- Term cash sums `N * (gearing * resolved_index + spread) * coupon_year_fraction`
  on every reset and profile slice, then rounds once.
- Multiple resets use simple accrued interest. There is no implicit compounding
  or averaged reset rate.
- The coupon day count and the index quotation day count stay distinct.

For OIS, the core calculates `factor = product(1 + rate_i * weight_i)`.
Non-business days have an explicit multi-day simple weight within one term. They
do not create fictitious weekend observations or repeated daily compounding.
Lookback without shift keeps the coupon's accrual weights while moving the rate
observations, and observation shift moves the weight window too. Lockout reuses
the last preceding observation and its full rate interval across the final
business-day weights. Each unique rate is resolved once. Shift requires
business-day endpoints, and unshifted non-business starts keep explicit partial
weights.

OIS gearing applies to compounded interest once. The spread accrues simply on the
unshifted coupon dates. For constant coupon principal and gearing, cash is
`N * gearing * (factor - 1) + simple_spread_cash`, computed through the core
compounded annualized-rate and coupon primitives. Decimal operations are checked
and finite.

Changes of principal or gearing within a coupon would need segmented compounding
and fail. Spread steps and changes at coupon boundaries are supported. Spread
compounding, rate averaging, caps, floors and options are not implicit variants.

## Fixing history and expected cash

Call `fixing_requirements(context, publication)` to get the unique outstanding
observations in chronological order. The publication function you pass in
receives the index and date and returns the administrator's publication
timestamp, including its holiday, time zone and DST rules. It is called once per
unique date. It must not infer UTC business time from a ticker.

`CashflowContext` supplies the valuation date, the known-at cutoff, eligibility
and the same-day `PaymentDateEntitlement`. Its unpublished and final-only policy
must match the floating contract. Before any forecast runs, the whole outstanding
leg is resolved in bounded batches. All missing published fixings are returned
together, and no partial cash result is returned. There is no forecast, zero, par
or discount-to-projection fallback for missing due history.

Both publication and receipt must be on or before known-at. The highest eligible
revision wins. Late corrections and future-date observations cannot leak into the
run. A past observation awaiting publication may be projected only under
`Forecast`, with the `AwaitingPublication` origin. A future observation has the
`FutureObservation` origin. Preliminary observations fail when final-only is
true. Published resets are copied unchanged when a future forecast changes.

The forecast callback receives a `ProjectionRequest` with the quotation dates and
day count, the index tenor where applicable, the publication requirement and the
projection curve key. Return `ForecastRate` with the exact source key requested.
Errors and out-of-domain rates propagate. The callback is a boundary you supply.
It is not a provider or repository interface or an automatic curve lookup. It and
any custom calendars must terminate without panic and must not read a hidden
mutable clock.

The output includes:

- the terms and pins, and the context;
- signed fixed and floating cash;
- dates and year fractions;
- unrounded and rounded amounts;
- all reset slices or compound observations;
- the unique published revisions and the forecast source lineage.

Pay-fixed makes fixed cash negative and floating cash positive, and negative
coupons can reverse that direction. Identical receive-fixed cash offsets exactly.

Cash before the valuation date is outside this projection. Same-day cash uses the
explicit include or exclude state. During a payment delay the fully accrued
coupon stays outstanding. After the last payment, both legs and the rate
requirements are empty. Notional becomes zero after accrual maturity even if
payment is delayed. Projecting or excluding a coupon never records that cash was
paid. Unpaid arrears and authoritative payment or default state belong to the
lifecycle and application ledger routes.

## Bounds and tests

- Each coupon, reset or profile grid has at most 4,096 entries.
- Years are 0001-9999, the contract span is at most 150×366 days, and offsets are
  at most 366 days.
- Checked calendar scans take at most 3,660 steps.
- A swap has at most 65,536 reset slices, daily observations or unique
  requirements.
- Core fixing resolution and compound batches have at most 4,096 entries, and a
  history at most 65,536 revisions.
- Profile-work validation counts the reset joins, twice the fixed-coupon profile
  scans and the floating slice scans, including the overnight principal and
  gearing scans, against a ceiling of 16,777,216.

Excess work and Decimal overflow fail. These are resource ceilings, not measured
latency or capacity.

Run from the repository root:

```sh
cargo test --locked -p convex-rates
cargo run --locked -p convex --example rates_contracts
cargo run --locked -p convex --example rates_contracts --no-default-features
cargo run --locked -p convex --example rates_contracts --features schemars
```

The [contract tests](../crates/convex-rates/tests/contracts.rs) use
[15 pinned fixtures](../crates/convex-rates/tests/fixtures/swaps.json)
and 128 deterministic date variations. The independent Python reference uses
hand-pinned date arrays, `datetime` and calendar business-day arithmetic, and
60-digit Decimal cash and factor calculations. It does not call Convex or an
external pricing engine.

Dates, signs and revisions must match exactly. The comparison bounds are 1e-18
absolute for cash at a fixture notional of 1e6, and 1e-24 absolute with zero
relative for factors. The stored reference cash and factors are rounded to 20 and
26 decimal places for representability. Existing quantitative artifacts and
tolerances are unchanged.

The holiday-weight formula is grounded in the
[NY Fed methodology](https://www.newyorkfed.org/markets/reference-rates/additional-information-about-reference-rates),
inspected 2026-10-06. The synthetic partial-weekend contract defines its own
weights; it is not a SOFR Index interpolation. The
[QuantLib v1.40 schedule source](https://raw.githubusercontent.com/lballabio/QuantLib/v1.40/ql/time/schedule.cpp)
was a design reference for anchored stubs and EOM. No external code is copied.
No QuantLib or ORE runtime and no market data pack is validated here.

Not yet covered: shared-host price and inverse, risk, lifecycle, and market packs
for USD, EUR and GBP. See [library coverage](internal/LIBRARY-COVERAGE.md) and
[assurance](internal/ASSURANCE.md).
