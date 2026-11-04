# Credit curve calibration and risk

`convex::credit_calibration` fits a survival curve to CDS quotes you pass in, and
`convex::credit_risk` computes risk on that curve. Each function validates its
input and returns an error instead of guessing. Analytics combines the single-name
CDS pricer, the immutable curve contracts and the shared bounded solvers.
`convex-curves` stores the survival grid and the repricing records;
`convex-engine::curve_builder::build_supplied_credit_curves` delegates to this
workflow. The calculation needs no clock, provider, database, message queue or
external engine.

## Example

```sh
cargo run --locked -p convex --example credit_calibration
cargo run --locked -p convex --example credit_calibration --no-default-features
cargo run --locked -p convex --example credit_calibration --features schemars
cargo test --locked -p convex-analytics --test credit_calibration
```

The example uses six dated quotes and a published USD SOFR discount grid. It
prints the fitted curve, the residuals in the quote units, the trade PV and the
signed running-quote CS01. The inputs are test data, not live CDS market quotes.
Serde roundtrips and the facade's schema feature do not add REST, C ABI, Excel,
Java, MCP or WASM dispatch for these functions.

## Calibration contract

Each `CdsCalibrationHelper` keeps the complete `CdsTerms`, an explicit valuation
and cash context, the unadjusted protection maturity, the quote revision you
select, the quote unit and an absolute tolerance. Bind the premium and settlement
calendars separately, in the same order as the helpers. Helpers on one curve must
have the same entity, seniority, document, restructuring, recovery, currency and
CDS model. The discount dependencies you pass in must cover their cash dates.
Quote revisions never become hypothetical market observations.

| Quote kind | Input unit | Fitted native output |
|---|---|---|
| `RunningSpread` | DecimalRate, PercentRate or BasisPoints | CLEAN par running spread |
| `CleanUpfront` | UpfrontFraction | CLEAN buyer-to-seller fraction at the retained coupon |
| `DirtyUpfront` | UpfrontFraction | CASH buyer-to-seller fraction including accrued refund |

`CdsCurveRecipe` supplies increasing ACT/365F intensity interval end dates and an
ACT/365F log-linear survival curve without extrapolation.

- Piecewise mode needs one unique helper per interval end, an explicit closed
  nonnegative hazard bracket and an iteration limit. It runs Brent against the
  complete pricer in increasing maturity order.
- `CreditGlobal` passes initial forward hazards, closed bounds, quote scales and
  weights (in helper order) and convergence settings to the shared bounded
  least-squares solver. Its Jacobian must identify every parameter.

Scales affect the fit; they never waive a quote's own tolerance, including for
zero-weight quotes.

Every accepted output reprices all helpers through `price_cds`. It keeps the full
helper recipe, the complete leg valuations, the solver work and rank, and the
residuals in helper order. Survival starts at one, stays positive and
nonincreasing, and integrates nonnegative piecewise hazards. These cases return an
error with no partial curve:

- underflow or a negative hazard;
- a missing dependency or calendar;
- an unbracketed root or a fit that does not identify every parameter;
- a residual outside the quote tolerance;
- an exhausted budget.

The curves you pass in and the last good curves are immutable. Recovery is an
input; recovery and hazard are not identified jointly.

The atomic batch accepts at most 128 helpers and parameters in total, 8,192
single-helper valuations, and 256 curves including those you pass in. Hazards lie
in [0,5] per ACT/365F year, horizons within 150 years and solver iterations in
1..10,000, subject to the stricter valuation budget. The stricter curve, schedule,
label and calendar bounds inherited from the lower crates also apply. Calendar
callbacks must terminate without panicking. These ceilings are resource limits,
not throughput or latency guarantees.

## Risk, recovery and default

`reprice_cds_helpers` checks one complete `CdsCurveBuildInput` against immutable
curves you pass in, without fitting them. It reuses the recipe and contract
validation and the Standard183 price and quote functions, and keeps the terms,
quote revisions, full valuations and signed residuals in helper order. Finite
off-market diagnostics carry `within_tolerance:false`; they are never an accepted
fit. See the [shared API](LIBRARY-API.md#direct-cds-helper-repricing-diagnostics)
for inputs and bounds.

Prepare `PreparedCreditRisk` from the full recipe, the trade, calendars, curve set,
context and a risk model you choose. Preparation performs the baseline fit and
price. `CreditScenarioRequest` carries exactly one axis and distinct hypothetical
curve versions that you assign:

| Shock | Inputs changed | Required response |
|---|---|---|
| `DiscountZero` | One exact collateral DF grid; continuous-zero decimal shift | Hold survival or rebuild at original CDS quotes |
| `Quotes` | Explicit helper ID, original observation revision and native-unit delta | Rebuild all helpers |
| `Recovery` | Fractional recovery on the credit role and full terms | Hold survival or rebuild at original quotes |

Frozen survival outputs are not refitted to the quotes. Full recovery is valid
when survival is frozen; it cannot identify a quote-recalibrated hazard curve.
Quote scenarios keep rates and recovery fixed; recovery scenarios keep rates and
quotes fixed. Nonlinear PV changes are scenario effects, not additive P&L
attribution or realized ledger cash.

`credit_sensitivity` evaluates both sides of a central bump you supply. Rate PV01
and running CDS CS01 are signed currency for a +1 bp move; protection buyer and
seller have opposite signs. Upfront and recovery outputs are monetary derivatives
per unit fraction and cannot be inserted into a bp risk bucket. Each factor must
match the currency, target, dated tenor, quote type, basis, convention and risk
model. Quote bumps must exceed 32 times the residual tolerance. A failed
repricing or an out-of-domain central bump never produces a zero sensitivity.
`validate_credit_risk_vector` rejects duplicate canonical factors, including
frozen and rebuilt variants; a job must select one response per factor.

`immediate_jump_to_default` is an immediate conditional scenario at the step-in
day you supply. It requires the exact entity and seniority, an explicit settlement
date and the fraction of default-day premium accrual. Unknown auction recovery or
source returns `Pending` without a monetary value. With recovery supplied, it
produces signed protection and accrued premium cash, discounted to valuation, and
the post-default PV minus the surviving PV. Zero and full auction recovery are
valid. Pending upfront cash, later default paths and realized payments need
lifecycle handling; this API does not record an auction or a payment.

## Bond/CDS hedge input

`admit_bond_cds_hedge` requires an explicit `BondCdsMapping` over a resolved
`BondCreditIdentity`, a full CDS reference, a credit version and a separate
`BondBasisInput`. The instrument, reference, entity, seniority, currency, recovery,
document, restructuring, basis and liquidity revisions must all match. Dated
residual continuous-zero basis inputs must cover both maturities without a tail
proxy. Bond terms contain no issuer identity; none is inferred from a ticker.

The result is validated input that can be reused. It does not calculate a bond
credit price, calibrate the residual basis, produce a hedge ratio or declare an
executable hedge. Product valuation, liquidity and eligibility checks,
optimization and execution happen elsewhere.

## Comparison with the ISDA C code

The `credit-isda-1.8.3` pack captures the output of the unmodified ISDA 1.8.3 C
code on six deterministic CDS spreads and all 36 USD SOFR discount knots. Twenty-
three base, rate, quote and recovery scenarios give 276 clean, dirty and PV
valuations. Running, mixed running/clean/dirty and global fits plus ten central
sensitivities give 1,150 numeric checks. The absolute bounds are hazard 1e-9,
survival 1e-10, upfront 1e-10 and PV 1e-3 currency; risk uses 0.1 currency plus
1e-6 relative. The quote tolerances and the bounds of the earlier 7,164-case grid
are unchanged.

```sh
python3 tests/scripts/isda_credit_compare.py check
cargo build --locked -p convex-analytics --example credit_comparison
python3 tests/scripts/isda_credit_compare.py offline --candidate-binary target/debug/examples/credit_comparison --report /tmp/credit-comparison.json
python3 -m unittest discover -s tests/scripts -p test_isda_credit_compare.py
```

Use a fresh report path. Offline mode runs the binary you pass, checks the pinned
source, input and capture bytes, the lineage, units and residuals and every
required value, and keeps the raw stdout and stderr even on failure. The capture
from the Rust code is a replay for testing, not an independent price oracle.
Capturing from the C code needs the checksum-verified publisher source archive. No
external model is installed or linked into the library. Attribution and the
[ISDA license](../licenses/ISDA-CDS-Standard-Model-Public-License-1.0.txt) apply
as described in [CDS valuation](CDS-VALUATION.md).

Not yet covered: full market and convention packs, RFR discount curve
calibration, lifecycle handling, and most host bindings. None of this has had an
independent quantitative review, and production, library and latency
certification are still open.

Capture of a full product graph, comparison of fitted outputs, solver and helper
diagnostics, and replay of quote risk are described in
[the schema-3 replay contract](CALIBRATION-SNAPSHOT-REPLAY.md). The calculations
do not depend on a provider, storage, message queue or clock. Schema 1 and 2
identities and the existing numerical controls are unchanged.
