# Shared cashflow primitives

CVX-054 supplies checked native building blocks in `convex-core`, reachable
through `convex::cashflows` and `convex::fixings`. Bonds and analytics re-export
the same types. Future rates and credit legs can use core directly. There is
one schedule generator; no server, repository, clock, vendor, DB, MQ or new
dependency participates in these calculations.

Run the standalone example from the repository root:

```sh
cargo run --locked -p convex --example cashflow_primitives
cargo run --locked -p convex --example cashflow_primitives --no-default-features
cargo run --locked -p convex --example cashflow_primitives --features schemars
cargo test --locked -p convex-core --test cashflow_primitives
cargo test --locked -p convex-bonds --test shared_primitives
cargo test --locked -p convex-analytics --test shared_primitives
```

The example produces coupons on stepped outstanding principal, expected
principal repayments, an explicit missing-history failure before forecasting,
and a compounded known/forecast result with provenance. Inputs are supplied
fixtures. Its `certified: false` remains explicit.

## Native contract

| API | Responsibility |
|---|---|
| `CouponSchedule::generate` | Anchored monthly grid, short/long front/back stubs, explicit first/penultimate coupon, distinct date schedules and retained notional reference periods |
| `DateRules` / `DateOffset` / `settlement_date` | Independent accrual adjustment, delayed payment, term fixing, observation, ex-coupon and settlement dates; signed offset also serves option notice dates |
| `CouponPeriod::year_fraction` | Checked subperiod accrual; split long ICMA stubs across each regular notional denominator; contract-aware checked 30E/360 ISDA |
| `DatedProfile::new` | Independent immutable contiguous coupon, spread, gearing or notional intervals with bounds and explicit boundary sides |
| `CouponProfiles` | Checked expected coupon and accrued amounts, independently dated steps, explicit principal basis, negative-coupon and rounding policy |
| `principal_cashflows` | Expected pre/post principal changes and final residual redemption; effective dates and cash payment dates remain separate |
| `FixingSeries::new` / `resolve` | Immutable revision history; complete missing list; publication and information cutoff before any projection |
| `OvernightSchedule` | Actual observation dates/weights, lookback versus shift, lockout, holiday/weekend weighting, checked product and per-observation provenance |

Validated schedule/profile/history objects have private fields and cannot be
deserialized directly into an unchecked state. Deserialize typed input terms,
then call their checked constructors. Input structs reject unknown fields.
These inputs are native building-block APIs; the existing product JSON/FFI
schema does not yet carry all these economics. CVX-063 owns that integration.

## Dates, stubs and reference periods

`ScheduleSpec` requires start/end, frequency, EOM choice and a stub rule. Each
regular date is computed from the original anchor and an integer period count.
A February clamp therefore cannot drift subsequent dates from the 30th to the
29th. EOM preserves calendar month end when the regular anchor is month end.
Automatic short/long front generation works backward; back generation works
forward. A long stub removes the adjacent coupon boundary, while retaining
every regular notional reference period for accrual.

Explicit first and penultimate dates define contractual first/last coupons.
The interior must fit the regular grid; conflicting, outside or inconsistent
dates fail. `StubType::None` on the shared API requires an exact interior grid.
Zero frequency has one principal period and rejects coupon-stub terms; ICMA
requires a nonzero coupon frequency.

Every period retains unadjusted contractual boundaries, independently adjusted
accrual boundaries, term fixing date, optional observation boundaries,
payment date and optional ex-coupon boundary. Business-day adjustment does not
rewrite all these dates. Payment delay is a nonnegative offset from unadjusted
coupon end followed by the supplied payment adjustment. Fixing and observation
offsets use accrual boundaries. Different reset/accrual/payment schedules can
be generated independently; full product leg composition remains its owner.

ICMA fractions intersect the requested subperiod with each retained regular
reference period and sum `actual_days / (frequency * reference_days)`. This
supports short/long first and last coupons, including settlement partway
through a long stub. No ISDA fallback or single long-stub denominator is used.
For 30E/360 ISDA the shared checked path retains an actual final February day;
a final 31st is still day 30. `Thirty360EIsda::checked_year_fraction` exposes
that path directly. The existing `DayCount` compatibility method retains its
historical all-month termination exception and its pinned tests; it is not the
checked shared contract. Other supported day counts reuse the existing kernels
under validated ordered canonical dates.

Calendars are immutable caller inputs. Their holiday coverage and provenance
remain the caller's responsibility; built-in calendar range does not certify
an arbitrary market contract. Joint calendars use the existing all-components
business-day rule. `calendars::checked` bounds searches and business-day offsets;
`Calendar::adjust` now returns a checked failure if no business day is reachable.
Custom trait/callback implementations must themselves terminate and not panic.

## Dated terms and cash

Each `DatedValue` covers `[start, end)`. Entries must arrive sorted, contiguous,
nonoverlapping and nonempty. The four profiles have equal full coverage; their
individual step dates may differ. A calculation splits at their combined step
dates and uses the declared day count for each economic segment. ICMA reference
denominators remain the original schedule's, including when a profile steps.
This is a piecewise-constant contractual profile, not an averaged coupon.

Coupon rate = deterministic coupon + gearing × supplied resolved index rate +
decimal spread. Rates and spreads are annualized decimals, not percent or bp.
Principal is absolute contract cash units; currency and signed position belong
to the enclosing product. The term index input is constant within that coupon
calculation. A full floating/overnight product with changing index/profile
composition must implement its product-specific calculation rather than feed
an averaged index into this utility.

`BoundarySide::Before` and `After` make step inclusion explicit. The coupon
principal choice is independent: accrue each segment on its outstanding
principal, or use principal before/after the **contractual coupon boundary**.
Payment lag never chooses principal. At maturity principal after redemption is
zero. A reduction creates before-minus-after cash; an increase creates a signed
advance. Internal repayments are at par, and an explicit 0..10 redemption
multiplier applies to final residual principal only.

All additions, multiplications, division and compound products on these paths
use checked decimal arithmetic. Decimal has at most 28 digits after the decimal
point; repeating fractions are representable approximations, not infinite
precision. Cash rounding is explicit: unrounded, nearest even, midpoint away
from zero or truncation toward zero, scale 0..28. Coupon segments sum before a
single cash rounding; ex-coupon subtraction uses the unrounded full coupon
before final accrued rounding. Negative coupon policy either preserves signed
cash or rejects the contract; no floor or option model is inferred.

Coupon entitlement and principal entitlement are independent. Ex-coupon rules
specify an offset, contractual/payment anchor and `OnDate` versus `AfterDate`.
For the pinned gilt record-date example, settlement on the record date retains
coupon entitlement; subsequent settlement loses it. Accrued can subtract the
full coupon (rebate) or follow an explicitly selected zero-accrued contract.
Payment-date inclusion is a separate caller-supplied pre/post payment choice.

Accrued for an unpaid period is zero before accrual and on/after its payment
date. During payment delay it remains fully accrued, subject to the ex-coupon
policy. A leg can sum outstanding period accruals. Principal effects and coupon
projections **do not assert a payment occurred**; realized cash/lifecycle state
and settlement history remain separate product/application work.

## Eligible observations and overnight projection

`FixingObservation` carries index/date, annualized decimal rate, source,
positive revision, actual publication instant, receipt/known-at instant and
preliminary/final status. History construction rejects duplicate revisions,
competing sources for an index/date, receipt before publication, publication
before the observation date, backward revision times and final-to-preliminary
regression. Input ordering is immaterial; revisions are retained immutably.
The application must select a source before constructing this history.

Each requirement has a caller-supplied expected publication instant.
Administrator calendars, local publication times, DST, exceptional delayed
publication and final-only policy are supplied explicitly. Resolution selects
the highest admissible revision with both actual publication and known-at
on/before cutoff. A known later revision never mutates an earlier result.
Future observation dates remain projected even when present in a restated
history. A historical rate already due for publication but missing at cutoff
is returned in the full missing list. It cannot become zero or a forecast.

A historical observation not yet due is separately labelled
`AwaitingPublication`. The caller must explicitly choose forecast or require
publication. Future observations have `FutureObservation` origin. Preliminary
values require explicit policy. `FixingResolution::project` checks the entire
missing list before calling any forecast callback; callback failures propagate.

Overnight compounding preserves each rate interval and weight. Lookback without
shift uses original accrual-day weights; observation shift uses shifted-day
weights. Shift currently requires business-day endpoints; unreviewed shifted
nonbusiness endpoint semantics fail. Without shift, an initial partial weekend
uses the previous business day's observation and the remaining calendar days.
The final interval is clipped to the supplied end. Lockout freezes the final N
observation business days at the last preceding observation and requires that
preceding rate; it does not duplicate requirements or forecasts. Combination
of these rules is explicit supplied-contract behavior, not a blanket ISDA
fallback or approved market convention.

The supported overnight bases are ACT/360 and ACT/365 Fixed. Forecast callbacks
receive the full rate observation interval, which can differ from the weighted
accrual interval. Every product factor must be positive; arithmetic overflow
fails. The result carries the unshifted period fraction, annualized compound
rate and every actual/projection origin. Spread/gearing/optional caps/floors and
full FRN/OIS product integration remain their own tasks.

## Bounds and compatibility

Dates use years 0001..9999; contract span ≤150×366 days. A schedule/profile has
at most 4,096 periods/entries; an overnight schedule has at most 4,096 terms.
Offsets/lookback/lockout are bounded by 366 days; checked calendar scans by
3,660 calendar days. A history has at most 65,536 revisions and one resolution
at most 4,096 unique requirements. Annualized input rates/spreads are within
±10, gearing magnitude ≤1e6, principal 0..1e18. These are resource/domain
ceilings, not capacity or latency measurements.

Existing bond `Schedule` and analytics `Schedule` paths remain available.
`StubType` is now the same shared type. The bond wrapper retains its historical
`None` short-front default (or short back after explicit first coupon), its
adjusted date/period accessors and zero-frequency behavior. It exposes shared
reference periods with `coupon_periods` and supplied calendars through
`generate_with_calendar`. `try_to_calendar` resolves known and bounded joint
names; unknown names fail. The established empty calendar spelling remains an
explicit weekend alias. USGov/SIFMA aliases retain their established mapping.
Checked UK/Japan names now resolve their real built-in calendars.

`FixedRateBondBuilder::build` and native deserialization validate/cache the
schedule before acceptance; `try_schedule` supplies typed failure to direct
callers. The older infallible `with_conventions`, bond trait schedule access,
calendar convenience searches, date-only `OvernightFixings`/ARRC function and
older accrued utilities keep their declared compatibility preconditions.
They are not upgraded publication/model certificates. Full product convention,
amount/pricing migration is CVX-017/055/018/019/020 and binding parity CVX-063.
Pure economic lifecycle remains CVX-062.

## Evidence and remaining scope

[Pinned fixtures](../crates/convex-core/tests/fixtures/cashflow-primitives.json)
name exact synthetic supplied contracts and analytic units/bounds. They use
DMO reference-period/record-date examples, NY Fed daily index/weight rules,
ARRC observation choices and ISDA day-count references. Fixture details are
derived checks, not copied provider data or a live administrator integration.
The NY Fed index example is checked at its published eight-decimal precision;
other Decimal analytic comparisons use absolute 1e-24, relative zero. Existing
quantitative fixture bytes and tolerances remain unchanged.

Primary references: [DMO convention paper](https://www.dmo.gov.uk/media/cp1linix/convention_changes.pdf),
[DMO record-date glossary](https://www.dmo.gov.uk/help/glossary/),
[NY Fed reference-rate methodology](https://www.newyorkfed.org/markets/reference-rates/additional-information-about-reference-rates),
[ARRC FRN matrix](https://www.newyorkfed.org/medialibrary/Microsites/arrc/files/2019/ARRC_SOFR_FRN_Conventions_Matrix.pdf),
[ISDA 30/360 examples](https://www.isda.org/2008/12/22/30-360-day-count-conventions/).

No full bond/IRS/CDS/futures price, curve, lifecycle, host parity, market pack,
performance or independent quantitative approval is inferred from these
building-block checks. All 30 mandatory areas and 2,154 library records remain
required; LB0/LB1 and production gates remain incomplete. See
[library coverage](LIBRARY-COVERAGE.md) and [assurance](ASSURANCE.md).


CVX-018 adds `CouponSchedule::generate_with_calendars` with distinct supplied
accrual, payment, fixing, observation and ex-date calendars. `generate` delegates
with its original single calendar, preserving existing date/reference behavior.
Calendar content/version binding remains the consuming product's responsibility.
The [rates contract guide](RATES-CONTRACTS.md) documents checked IRS/OIS use,
explicit reset quotations and expected signed cash over these same primitives.
