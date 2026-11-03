# Shared cashflow primitives

`convex-core` provides the building blocks for coupon schedules, dated coupon
and principal profiles, fixing history and overnight compounding. They are
reachable through `convex::cashflows` and `convex::fixings`, and the bonds and
analytics crates re-export the same types. Rates and credit legs can use core
directly. There is one schedule generator.

Each function validates its input and returns an error instead of guessing.
None of these calculations uses a server, repository, clock, vendor feed,
database or message queue.

Run the standalone example from the repository root:

```sh
cargo run --locked -p convex --example cashflow_primitives
cargo run --locked -p convex --example cashflow_primitives --no-default-features
cargo run --locked -p convex --example cashflow_primitives --features schemars
cargo test --locked -p convex-core --test cashflow_primitives
cargo test --locked -p convex-bonds --test shared_primitives
cargo test --locked -p convex-analytics --test shared_primitives
```

The example produces:

- coupons on stepped outstanding principal;
- expected principal repayments;
- a missing-history failure that is reported before any forecast;
- a compounded known-plus-forecast result with provenance.

Its inputs are fixtures, and it prints `"certified":false`.

## API overview

| API | Responsibility |
|---|---|
| `CouponSchedule::generate` | Anchored monthly grid, short/long front/back stubs, explicit first/penultimate coupon, distinct date schedules and retained notional reference periods |
| `DateRules` / `DateOffset` / `settlement_date` | Independent accrual adjustment, delayed payment, term fixing, observation, ex-coupon and settlement dates; a signed offset also serves option notice dates |
| `CouponPeriod::year_fraction` | Validated subperiod accrual; splits long ICMA stubs across each regular notional denominator; validated 30E/360 ISDA |
| `DatedProfile::new` | Independent immutable contiguous coupon, spread, gearing or notional intervals with bounds and explicit boundary sides |
| `CouponProfiles` | Expected coupon and accrued amounts, independently dated steps, explicit principal basis, negative-coupon and rounding policy |
| `principal_cashflows` | Expected pre/post principal changes and final residual redemption; effective dates and cash payment dates stay separate |
| `FixingSeries::new` / `resolve` | Immutable revision history; complete missing list; publication and information cutoff before any projection |
| `OvernightSchedule` | Observation dates and weights, lookback versus shift, lockout, holiday/weekend weighting, validated product and per-observation provenance |

Validated schedule, profile and history objects have private fields and cannot
be deserialized directly into an unchecked state. Deserialize the input terms,
then call the validating constructors. Input structs reject unknown fields.

These are Rust building-block APIs. The product JSON/FFI schema does not yet
carry all of these economics.

## Dates, stubs and reference periods

`ScheduleSpec` requires start and end dates, a frequency, an EOM choice and a
stub rule. Each regular date is computed from the original anchor and an integer
period count. A February clamp therefore cannot drift later dates from the 30th
to the 29th. EOM keeps the calendar month end when the regular anchor is a month
end.

Automatic short or long front stubs are generated backward from the end date,
and back stubs forward from the start. A long stub removes the adjacent coupon
boundary but keeps every regular notional reference period for accrual.

Explicit first and penultimate dates define the first and last coupons. The
interior must fit the regular grid; conflicting, outside or inconsistent dates
fail. `StubType::None` on the shared API requires an exact interior grid. Zero
frequency has one principal period and rejects coupon-stub terms. ICMA requires
a nonzero coupon frequency.

Every period keeps:

- the unadjusted contractual boundaries;
- independently adjusted accrual boundaries;
- the term fixing date;
- optional observation boundaries;
- the payment date;
- an optional ex-coupon boundary.

Business-day adjustment does not rewrite all of these dates. Payment delay is a
nonnegative offset from the unadjusted coupon end, followed by the payment
adjustment you pass in. Fixing and observation offsets count from the accrual
boundaries. Reset, accrual and payment schedules can be generated independently.
Putting them together into a full product leg is left to the product code.

ICMA fractions intersect the requested subperiod with each retained regular
reference period and sum `actual_days / (frequency * reference_days)`. This
supports short and long first and last coupons, including settlement partway
through a long stub. There is no ISDA fallback and no single long-stub
denominator.

For 30E/360 ISDA the shared path keeps an actual final February day; a final
31st is still day 30. `Thirty360EIsda::checked_year_fraction` exposes that path
directly. The existing `DayCount` method keeps its historical termination
exception and its pinned tests, and is not the validated shared contract. Other
supported day counts reuse the existing kernels on validated, ordered dates.

Calendars are immutable inputs from the caller. The caller is responsible for
holiday coverage and provenance, and the built-in calendar range does not make
an arbitrary market contract valid. Joint calendars use the existing
all-components business-day rule. `calendars::checked` bounds the searches and
business-day offsets. `Calendar::adjust` now returns an error if no business day
is reachable. Custom trait or callback implementations must terminate and must
not panic.

## Dated terms and cash

Each `DatedValue` covers `[start, end)`. Entries must be sorted, contiguous,
non-overlapping and non-empty. The four profiles have equal full coverage, but
their step dates may differ. A calculation splits at the combined step dates and
uses the declared day count for each segment. ICMA reference denominators stay
those of the original schedule, including when a profile steps. This is a
piecewise-constant contractual profile, not an averaged coupon.

Coupon rate = deterministic coupon + gearing × resolved index rate + decimal
spread. Rates and spreads are annualized decimals, not percent or bp. Principal
is in absolute contract cash units; the enclosing product owns the currency and
the signed position.

The term index rate is constant within one coupon calculation. A full floating
or overnight product whose index or profile changes within the period needs its
own calculation. Do not feed an averaged index into this utility.

`BoundarySide::Before` and `After` make step inclusion explicit. The principal
used for a coupon is a separate choice: accrue each segment on its outstanding
principal, or use the principal before or after the **contractual coupon
boundary**. Payment lag never chooses the principal.

At maturity the principal after redemption is zero. A reduction creates
before-minus-after cash, and an increase creates a signed advance. Internal
repayments are at par. An explicit 0..10 redemption multiplier applies to the
final residual principal only.

All additions, multiplications, divisions and compound products on these paths
use checked decimal arithmetic. Decimal has at most 28 digits after the decimal
point, so repeating fractions are approximations. Cash rounding is explicit:

- unrounded;
- nearest even;
- midpoint away from zero;
- truncation toward zero.

The scale is 0..28. Coupon segments are summed before a single cash rounding.
The ex-coupon subtraction uses the unrounded full coupon before the final accrued
rounding. The negative coupon policy either keeps signed cash or rejects the
contract. No floor or option model is inferred.

Coupon entitlement and principal entitlement are independent. Ex-coupon rules
specify an offset, a contractual or payment anchor, and `OnDate` versus
`AfterDate`. In the pinned gilt record-date example, settlement on the record
date keeps coupon entitlement, and later settlement loses it. Accrued can
subtract the full coupon (rebate) or follow an explicitly selected zero-accrued
contract. Whether cash due on the payment date is included is a separate
pre/post-payment choice that the caller supplies.

Accrued interest for an unpaid period is zero before accrual starts and on or
after its payment date. During a payment delay it stays fully accrued, subject to
the ex-coupon policy. A leg can sum the outstanding period accruals.

Principal effects and coupon projections **do not assert that a payment
occurred**. Realized cash, lifecycle state and settlement history are handled
separately in the product and application code.

## Fixing observations and overnight projection

`FixingObservation` carries:

- index and date;
- annualized decimal rate;
- source;
- positive revision;
- publication instant;
- receipt (known-at) instant;
- preliminary or final status.

History construction rejects duplicate revisions, competing sources for an
index and date, receipt before publication, publication before the observation
date, backward revision times and a final-to-preliminary regression. Input order
does not matter, and revisions are kept immutably. The application must choose a
source before building the history.

Each requirement has an expected publication instant that the caller supplies.
Administrator calendars, local publication times, DST, exceptional delayed
publication and a final-only policy are all passed in explicitly. Resolution
picks the highest revision whose publication and known-at instants are both on
or before the cutoff. A later revision never changes an earlier result.

Future observation dates stay projected even when they appear in a restated
history. A historical rate that is already due but missing at the cutoff goes
into the full missing list. It cannot become zero or a forecast.

A historical observation that is not yet due is labelled `AwaitingPublication`.
The caller must choose either to forecast it or to require publication. Future
observations have the origin `FutureObservation`. Preliminary values need an
explicit policy. `FixingResolution::project` checks the whole missing list before
it calls any forecast callback, and callback failures propagate.

Overnight compounding keeps each rate interval and weight. Lookback without
shift uses the accrual-day weights. Observation shift uses the shifted-day
weights. Shift currently requires business-day endpoints, and shifted
non-business endpoints fail.

Without shift, an initial partial weekend uses the previous business day's
observation and the remaining calendar days. The final interval is clipped to
the end date you pass in. Lockout freezes the final N observation business days
at the last preceding observation and requires that preceding rate. It does not
duplicate requirements or forecasts. Combining these rules is explicit,
contract-specific behavior, not an ISDA fallback or an approved market
convention.

The supported overnight bases are ACT/360 and ACT/365 Fixed. Forecast callbacks
receive the full rate observation interval, which can differ from the weighted
accrual interval. Every product factor must be positive, and arithmetic overflow
fails. The result carries the unshifted period fraction, the annualized compound
rate and the origin of every actual or projected rate. Spread, gearing, caps and
floors, and full FRN/OIS product integration are not part of this layer.

## Bounds and compatibility

- Dates use years 0001..9999, and a contract span is at most 150×366 days.
- A schedule or profile has at most 4,096 periods or entries. An overnight
  schedule has at most 4,096 terms.
- Offsets, lookback and lockout are bounded by 366 days, and checked calendar
  scans by 3,660 calendar days.
- A history has at most 65,536 revisions, and one resolution has at most 4,096
  unique requirements.
- Annualized input rates and spreads are within ±10, gearing magnitude is at most
  1e6, and principal is 0..1e18.

These are resource and domain limits, not capacity or latency measurements.

The existing bond `Schedule` and analytics `Schedule` paths remain available.
`StubType` is now the same shared type. The bond wrapper keeps its historical
`None` short-front default (or short back after an explicit first coupon), its
adjusted date and period accessors, and its zero-frequency behavior. It exposes
the shared reference periods with `coupon_periods`, and calendars you pass in
with `generate_with_calendar`.

`try_to_calendar` resolves known and bounded joint names, and unknown names
fail. The established empty calendar spelling remains an explicit weekend alias.
USGov/SIFMA aliases keep their established mapping. The checked UK and Japan
names now resolve to their built-in calendars.

`FixedRateBondBuilder::build` and Rust deserialization validate and cache the
schedule before accepting it, and `try_schedule` returns a typed error to direct
callers. The older infallible `with_conventions`, the bond trait schedule access,
the calendar convenience searches, the date-only `OvernightFixings`/ARRC
function and the older accrued utilities keep their documented preconditions.

`CouponSchedule::generate_with_calendars` takes distinct accrual, payment,
fixing, observation and ex-date calendars. `generate` delegates to it with its
original single calendar, so existing date and reference behavior is unchanged.
Binding calendar content and version to a contract is the job of the product
that uses it. The [rates contract guide](RATES-CONTRACTS.md) documents IRS and
OIS use of these primitives, including explicit reset quotations and expected
signed cash.

## Evidence

The [pinned fixtures](../crates/convex-core/tests/fixtures/cashflow-primitives.json)
describe synthetic contracts and the analytic units and bounds. They use the DMO
reference-period and record-date examples, the NY Fed daily index and weight
rules, ARRC observation choices and ISDA day-count references. The fixture
details are derived checks, not copied provider data or a live administrator
integration.

The NY Fed index example is checked at its published eight-decimal precision.
Other Decimal comparisons use absolute 1e-24 and relative zero. Existing
quantitative fixture bytes and tolerances are unchanged.

Primary references: [DMO convention paper](https://www.dmo.gov.uk/media/cp1linix/convention_changes.pdf),
[DMO record-date glossary](https://www.dmo.gov.uk/help/glossary/),
[NY Fed reference-rate methodology](https://www.newyorkfed.org/markets/reference-rates/additional-information-about-reference-rates),
[ARRC FRN matrix](https://www.newyorkfed.org/medialibrary/Microsites/arrc/files/2019/ARRC_SOFR_FRN_Conventions_Matrix.pdf),
[ISDA 30/360 examples](https://www.isda.org/2008/12/22/30-360-day-count-conventions/).

These checks cover the building blocks only. They do not validate full bond,
IRS, CDS or futures prices, curves, lifecycle, host parity, market data packs or
performance. See [library coverage](internal/LIBRARY-COVERAGE.md) and
[assurance](internal/ASSURANCE.md).
