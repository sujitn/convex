# Accrued interest for constant-coupon bonds

`convex_bonds::cashflows::AccruedInterestCalculator` computes accrued interest
for constant-coupon bonds. Its `standard`, `ex_dividend`, `irregular_period`
and `using_year_fraction` methods validate their input and return
`BondResult<Decimal>` instead of guessing, so propagate errors with `?`.

The result is in the cash units of the face amount you pass in. The helpers do
not normalize to per-100, and they do not infer recorded payments or
entitlement. Signed and zero annual coupon rates are valid. Face must be
positive. Dates must be ordered within the period and within the supported date
and work bounds. Decimal overflow, and loss of a nonzero amount through
underflow, return errors. The explicit `Frequency::Zero` no-coupon case still
accrues zero.

## Day counts

ACT/360, ACT/365 Fixed, ACT/ACT ISDA, 30/360 US and 30E/360 use their annual
day fractions. ISDA splits the actual interval across calendar years.

ACT/365L uses the coupon frequency and the complete coupon period to choose its
denominator. Full `CouponPeriod` accrual uses the same code. The old two-date
`Act365Leap` diagnostic does not have that coupon context.

ICMA needs an actual regular reference period and a frequency. The irregular
helper needs its single reference period to cover the whole accrual period.
Long reference meshes and maturity-dependent conventions use the full contract
methods, because they cannot be guessed from these shorter signatures.

## Ex-dividend

`ex_dividend` takes `&dyn Calendar`. It counts the business-day offset you pass
in backwards from the coupon end date, excluding that end date. Before the
cutoff it reports interest earned from the period start. At and after the
cutoff it reports negative interest for the remaining period.

This rule does not establish a market's payment delay, rebate, record date or
paid state. Use the full `BondTerms` and lifecycle state for those.

## Engine and shared operation

The `YieldEngine::accrued_interest` methods in both `convex-bonds` and
`convex-analytics` now return `Result`. `Standard` delegates to the calculator.
An explicit `None` validates its inputs before returning zero. The old engine
signature cannot carry ex-coupon calendars or record-date entitlement, so those
requests return errors. Use the calendar-taking calculator or the full contract
route instead.

The shared `accrued_interest` operation has `standard`, `ex_dividend`,
`irregular_period` and `year_fraction` actions. Decimal amounts and rates are
exact strings. Ex-coupon actions bind a calendar in `LibraryInputs` by exact ID
and revision. The response carries `unit: "input_face_cash"` and `amount`.

The ambiguous `Act365` and `Thirty360` type aliases have been removed. Use
`Act365Fixed` and `Thirty360US`. Other day-count parsing and context shortcuts
still need migrating.

```sh
cargo test --locked -p convex-analytics --test accrued_methods
cargo run --locked -p convex --example accrued_methods
```

The arithmetic checks in these tests are independent of the implementation.
They have not been approved as convention packs, and the Windows and Excel
paths have not been run.
