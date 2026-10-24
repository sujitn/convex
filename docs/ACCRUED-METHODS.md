# Checked constant-coupon accrued interest

`convex_bonds::cashflows::AccruedInterestCalculator` returns `BondResult<Decimal>`
from `standard`, `ex_dividend`, `irregular_period` and `using_year_fraction`.
Propagate errors with `?`. Results retain the supplied original face's cash units;
the helpers do not infer per100 normalization, recorded payments or entitlement.
Signed and zero annual coupon rates are valid. Face must be positive. Dates must
be ordered within the original period and within native date/work bounds.
Decimal overflow and loss of a nonzero amount through underflow return errors.
The existing explicit `Frequency::Zero` no-coupon case retains zero accrual.

ACT/360, ACT/365 Fixed, ACT/ACT ISDA, 30/360 US and 30E/360 use their annual
day fractions. ISDA splits the actual interval across calendar years. ACT/365L
uses the original coupon frequency and complete coupon period to choose its
denominator; the same checked owner is used by full `CouponPeriod` accrual.
The old two-date `Act365Leap` diagnostic does not supply that coupon context.
ICMA requires an actual regular reference period and frequency. The irregular
helper requires its single reference to cover the complete original accrual
period. Long reference meshes and maturity-dependent conventions use full
contract methods; they cannot be guessed through these shorter signatures.

`ex_dividend` takes `&dyn Calendar`. It counts the supplied bounded business-day
offset backwards from the original coupon end, excluding that anchor. Before
the cutoff it reports interest earned from the period start; at and after the
cutoff it reports negative remaining-period interest. This explicitly selected
law does not establish a market's payment delay, rebate, record-date or paid
state. Use full `BondTerms` and lifecycle state for those economics.

Both native and analytics `YieldEngine::accrued_interest` now return `Result`.
`Standard` delegates to the calculator; explicit `None` validates its inputs
before returning zero. The old engine signature cannot carry actual ex-coupon
calendars or record-date/cum-dividend entitlement. Those requests return errors;
use the supplied-calendar calculator or the complete contract route.

The strict shared `accrued_interest` operation has `standard`, `ex_dividend`,
`irregular_period` and `year_fraction` actions. Decimal amounts/rates are exact
strings. Ex-coupon actions bind an actual `LibraryInputs` calendar by exact ID
and revision. The response carries `unit: "input_face_cash"` and `amount`.

Ambiguous `Act365` and `Thirty360` type aliases have been removed. Use the actual
`Act365Fixed` and `Thirty360US` names. Other day-count parsing/context shortcuts
remain separate required migrations.

```sh
cargo test --locked -p convex-analytics --test accrued_methods
cargo run --locked -p convex --example accrued_methods
```

The independent arithmetic controls are engineering checks. They do not provide
independently approved convention packs or qualify unavailable Windows/Excel
execution. CVX-063 retains its other mandatory native and client requirements.
