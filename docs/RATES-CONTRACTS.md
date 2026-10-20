# Supplied IRS and OIS contracts

CVX-018 adds the pure `convex-rates` crate, exposed as `convex::rates`.
`SwapTerms::build` validates supplied economics and caches actual fixed/floating
coupon schedules, term-reset slices or overnight observation weights.
`SwapContract::cashflows` returns expected signed coupon cash with fixing and
projection provenance. This is construction and contractual cash engineering
evidence. [CVX-019 valuation](SWAP-VALUATION.md) adds native discounted PV,
fair rate/spread, accrued and multi-curve calibration candidates. Its external
comparator gate is pending. Risk remains CVX-057/061; lifecycle and host dispatch
remain CVX-062/063.

No engine, server, DB, MQ, provider, global evaluation date or clock is needed.
The existing tenor-only `convex_curves::Swap/Ois` helpers and hedge advisor
`InterestRateSwap` retain their separate compatibility contracts; they are not
aliases for these complete dated cash contracts.

## Construction and units

`SwapTerms` retains schema version 1, trade ID/version, cash currency,
pay-fixed/receive-fixed direction, explicit unadjusted effective/maturity dates,
independent leg economics and collateral mapping. It is a pure economic input,
not an authoritative book/ledger/counterparty record or trade event.

| Input | Meaning and admission |
|---|---|
| Fixed leg | Independent frequency, stub/EOM, first/penultimate dates, day count, accrual/payment rules, dated rate and notional profiles. |
| Term floating leg | Independent coupon and reset grids, index ID and 1–120 month tenor, reset and fixing calendars, nonpositive fixing offset, fixing adjustment, explicit index quotation interval, separate spread/gearing/notional profiles. |
| Overnight leg | Exact index, observation calendar, ACT/360 or ACT/365 Fixed, explicit business-day lookback, observation shift and lockout, simple spread and compounded-interest gearing. |
| Calendar pins | Each resource has a provider-neutral `ReferenceDataId` and positive immutable `Version`; `CalendarBinding` must match exactly. The caller binds actual calendar content to its pin. A label check is not content attestation. |
| Curves | Separate exact `CurveKey { id, version }` projection and collateral discount dependencies. Forecasts must identify the requested projection version. Actual curve role/currency/query validation belongs to the subsequent pricer, not an inferred index preset. |
| Collateral | Exact agreement pin, cash/collateral currency and explicit discount curve. This route supports same-currency collateral only. |
| Rates/spreads | Annualized decimals: 0.05 is 5%, 0.001 is 10 bp. Index rates and coupon/spread inputs are within ±10. |
| Gearing | Dimensionless multiplier with magnitude ≤1e6. |
| Notional | Absolute unsigned principal in 0..1e18; initial amount must be positive. Trade direction is separate. Increases/reductions change interest exposure and create no principal exchanges. |
| Rounding | Explicit core `CashRounding`, applied once after all coupon slices; unrounded amounts are also returned. |

Every dated profile must be sorted, contiguous and cover exactly that leg's
actual adjusted accrual span. Internal dates may differ across rate, spread,
gearing and notional. A value takes effect at its inclusive start; coupon
interest uses the actual outstanding principal in each half-open segment.
Fixed and floating leg notionals may differ explicitly. No flat average replaces
a step. Negative rates and cash are preserved under `Allow`; `Reject` returns
an error if an effective coupon segment is negative. No cap/floor is inferred.

Raw economic terms implement strict native Serde. All profile Decimal values
are **strings**, retaining exact precision with workspace serde-float enabled.
Unknown payoff fields, floating kinds and excess input fields fail. Deserialize
terms, then rebuild with actual pinned calendars. `SwapContract` cannot be
deserialized into unchecked cached plans. Generated JSON schemas and v1
JSON/C ABI/Java/Excel/MCP/WASM construction or dispatch are not added here.

## Dates and coupon mathematics

The same core generator now has `generate_with_calendars` accepting distinct
accrual, payment, fixing, observation and ex-date calendars. Existing `generate`
delegates with its original single supplied calendar and retains behavior.
Swap coupon terms forbid bond ex-coupon fields and implicit fixing/observation
offsets: floating terms own those independent dates.

Accrual adjustments affect accrual boundaries only. Payment delay counts from
the **unadjusted coupon end**, excluding the starting date for business-day
lags, then payment adjustment is applied on the payment calendar. A swap
payment must be on/after actual accrual end and payments must not run backward.
This explicit lag anchor is not advertised as every market contract's rule.
Alternative anchors require a separately represented and reviewed contract.
Stubs preserve the core regular reference periods; ICMA long stubs split at
those reference periods. Contractual 30E/360 ISDA uses the actual adjusted leg
termination date in the shared core.

Term resets cover the entire floating coupon accrual span. Each intersection
retains coupon-slice start/end, fixing date, full index interval and quotation
day count. The fixing offset counts from adjusted reset start on the fixing
calendar, followed by fixing adjustment. A resulting observation after reset
start is unsupported without an explicit in-arrears/convexity model.

`TermRatePeriod::ResetInterval` explicitly contracts a rate over the actual
reset interval, including a stub; index-tenor-only settings must be neutral (false/Unadjusted).
`IndexTenor` uses the full supplied calendar-month tenor from adjusted reset
start, with explicit EOM and end adjustment on the reset calendar. Coupon
accrual may be shorter than that quotation period. No interpolation between
two index tenors is invented. Contextual ICMA/30E ISDA **index quotation** is
unsupported; coupon day counts still use the shared retained context.

Fixed cash sums `N * fixed_rate * coupon_year_fraction` on each profile segment.
Term cash sums `N * (gearing * resolved_index + spread) * coupon_year_fraction`
on every actual reset/profile slice, then rounds once. Multiple resets use
simple accrued interest; there is no implicit compound or averaged reset rate.
Coupon day count and index quotation day count remain distinct.

For OIS, the core calculates `factor = product(1 + rate_i * weight_i)`.
Non-business days have an explicit multi-day simple weight within one term,
not fictitious weekend observations or repeated daily compounding. Lookback
without shift keeps the coupon's accrual weights while moving rate observations;
observation shift moves the weight window too. Lockout reuses the last preceding
observation and full rate interval across its final business-day weights. Each
unique rate is resolved once. Shift requires business-day endpoints; unshifted
non-business starts retain explicit partial weights.

OIS gearing applies to compounded interest once, spread accrues simply on the
unshifted coupon dates. Equivalently cash is
`N * gearing * (factor - 1) + simple_spread_cash` for constant coupon principal/
gearing, through the core compounded annualized-rate/coupon primitives.
Decimal operations retain their checked finite precision. Intracoupon changes
in principal/gearing require explicit segmented-compounding economics and
fail; spread steps and changes at coupon boundaries are supported. Spread
compounding, rate averaging, caps/floors and options are not implicit variants.

## Supplied history and expected cash

Call `fixing_requirements(context, publication)` for unique outstanding
observations in chronological order. The supplied publication function receives
the actual index/date and returns an administrator publication timestamp,
including its holiday/timezone/DST rules. It is called once per unique date.
The publication function must not infer UTC business time from a ticker.

`CashflowContext` supplies valuation date, known-at cutoff, eligibility and
same-day `PaymentDateEntitlement`. Its unpublished/final-only policy must match
the floating contract. Before any forecast runs, the complete outstanding leg
is resolved in bounded batches. All missing required published fixings are
returned together and no partial cash result is returned. There is no forecast,
zero, par or discount-to-projection fallback for missing due history.

Both actual publication and receipt must be on/before known-at. The highest
eligible revision wins; late corrections and future-date observations cannot
leak into the run. A past observation awaiting publication may be projected
only under `Forecast`, with `AwaitingPublication` origin. A future observation
has `FutureObservation` origin. Preliminary observations fail when final-only
is true. Published resets are copied unchanged when a future forecast changes.

The pure forecast callback receives an actual `ProjectionRequest`, including
full quotation dates/day count, index tenor where applicable, publication
requirement and projection curve key. Return `ForecastRate` with the exact
requested source key. Errors and out-of-domain rates propagate. It is a
supplied calculation boundary, not a provider/repository interface or automatic
curve lookup. Callbacks and custom calendars must terminate without panic and
must not consult a hidden mutable clock.

Output includes original exact terms/pins, context, signed fixed/floating cash,
actual dates/year fractions, unrounded/rounded amounts, all reset slices or
compound observations, and unique published revisions/forecast source lineage.
Pay-fixed signs fixed cash negative and floating positive; negative coupons can
reverse that cash direction. Identical receive-fixed cash offsets exactly.

Cash before valuation is outside this future projection; same-day cash uses
the explicit include/exclude state. During payment delay, the fully accrued
coupon remains outstanding. After the last actual payment, both legs and rate
requirements are empty. Notional becomes zero after accrual maturity even if
payment is delayed. Projecting or excluding a coupon never records that cash
was paid; unpaid arrears and authoritative payment/default state need the pure
lifecycle and application ledger routes.

## Bounds and standalone evidence

Each coupon/reset/profile grid has at most 4,096 entries, years 0001–9999,
contract span ≤150×366 days, offsets ≤366 days, checked calendar scans ≤3,660
steps. A swap has at most 65,536 reset slices/daily observations or unique
requirements. Individual core fixing resolution/compound batches are ≤4,096;
history has ≤65,536 revisions. Profile-work admission conservatively counts
reset joins and twice the fixed-coupon profile scans plus floating slice scans,
including overnight principal/gearing admission scans, with ceiling 16,777,216. Excess work and checked Decimal overflow fail.
These are resource ceilings, not measured low-latency or capacity certificates.

Run from the repository root:

```sh
cargo test --locked -p convex-rates
cargo run --locked -p convex --example rates_contracts
cargo run --locked -p convex --example rates_contracts --no-default-features
cargo run --locked -p convex --example rates_contracts --features schemars
```

[Contract tests](../crates/convex-rates/tests/contracts.rs) consume
[15 pinned supplied-contract fixtures](../crates/convex-rates/tests/fixtures/swaps.json)
and 128 deterministic date variations. The independent Python oracle uses
hand-pinned contractual date arrays, `datetime`/calendar business-day arithmetic
and 60-digit Decimal cash/factor calculations, without calling Convex or an
external pricing engine. Dates/signs/revisions are exact. New comparison bounds
are cash 1e-18 absolute at fixture 1e6 notional, factor 1e-24 absolute, relative
zero; stored oracle cash/factors are rounded only to 20/26 decimal places for
representability. Existing quantitative artifacts/tolerances remain unchanged.

The holiday-weight formula is grounded in the
[NY Fed methodology](https://www.newyorkfed.org/markets/reference-rates/additional-information-about-reference-rates),
inspected 2026-10-06. The synthetic partial-weekend contract explicitly defines
its weights; it is not an unqualified SOFR Index interpolation. The
[QuantLib v1.40 schedule source](https://raw.githubusercontent.com/lballabio/QuantLib/v1.40/ql/time/schedule.cpp)
provides a design reference for anchored stubs/EOM. No external source code is
copied; no QuantLib/ORE runtime or actual market pack is certified here.

Full shared-host price/inverse, risk, lifecycle, exact
USD/EUR/GBP market-pack admission and independent LB1/production/colocation
certification remain owning work. See [library coverage](LIBRARY-COVERAGE.md)
and [assurance](ASSURANCE.md). Finish the CVX-019 external-engine comparison
before accepting its native valuation/calibration implementation.
