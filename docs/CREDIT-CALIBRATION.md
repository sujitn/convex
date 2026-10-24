# Supplied credit calibration and risk inputs

CVX-021 adds `convex::credit_calibration` and `convex::credit_risk`. Analytics
composes the checked single-name CDS pricer, immutable curve contracts and shared
bounded solvers. `convex-curves` stores the resulting survival grid and original
repricing records; `convex-engine::curve_builder::build_supplied_credit_curves`
delegates to this pure workflow. No clock, provider, database, MQ or external
engine is a calculation dependency.

## Start with the standalone consumer

```sh
cargo run --locked -p convex --example credit_calibration
cargo run --locked -p convex --example credit_calibration --no-default-features
cargo run --locked -p convex --example credit_calibration --features schemars
cargo test --locked -p convex-analytics --test credit_calibration
```

The example supplies six dated quotes and the original published USD SOFR
discount grid. It prints the actual fitted curve, native residuals, trade PV and
signed running-quote CS01. These engineering inputs are not live CDS market quotes.
Native serde roundtrips and the facade's schema feature do not add host schemas
or REST/C ABI/Excel/Java/MCP/WASM dispatch. CVX-063 owns those integrations.

## Calibration contract

Each `CdsCalibrationHelper` retains complete `CdsTerms`, an explicit surviving
valuation/cash context, actual unadjusted protection maturity, caller-selected
quote revision, native quote unit and absolute tolerance. Bind the actual premium
and settlement calendars separately in the same source order. Helpers on a curve
must have identical entity, seniority, document, restructuring, recovery, currency
and CDS model. The exact supplied discount dependencies must cover their cash
dates. Original quote revisions never become hypothetical market observations.

| Quote kind | Input unit | Fitted native output |
|---|---|---|
| `RunningSpread` | DecimalRate, PercentRate or BasisPoints | CLEAN par running spread |
| `CleanUpfront` | UpfrontFraction | CLEAN buyer-to-seller fraction at the retained coupon |
| `DirtyUpfront` | UpfrontFraction | CASH buyer-to-seller fraction including accrued refund |

`CdsCurveRecipe` supplies increasing ACT/365F intensity interval end dates and an
ACT/365F log-linear survival curve without extrapolation. Piecewise mode requires
one unique helper per interval end, an explicit closed nonnegative hazard bracket
and an iteration limit. It runs Brent against the complete pricer in increasing
maturity order. `CreditGlobal` supplies initial forward hazards, closed bounds,
source-order quote scales/weights and convergence settings to the common bounded
least-squares solver. Its actual Jacobian must identify every parameter. Scales
affect fitting; they never waive a quote's original native tolerance, including
zero-weight quotes.

Every accepted output reprices all original helpers through `price_cds`. It
retains the full original helper recipe, complete leg valuations, actual solver
work/rank and native residuals in source order. Survival starts at one, stays
positive and nonincreasing, and integrates nonnegative piecewise hazards. An
underflow, negative hazard, missing exact dependency/calendar, unbracketed root,
nonidentified fit, native residual failure or exhausted budget returns an error
with no partial accepted curve. Supplied and last-good curves are immutable.
Recovery is supplied; joint recovery/hazard identification is not inferred.

The atomic batch admits at most 128 total helpers and parameters and 8,192 actual
single-helper valuations, with at most 256 curves including supplied inputs.
Hazards lie in [0,5] per ACT/365F year, horizons in 150 years and solver iterations
in 1..10,000, subject to the stricter actual valuation budget. The stricter inherited curve/schedule/label/calendar bounds also apply.
Actual calendar callbacks must terminate without panic. These resource ceilings
are not throughput, latency or colocation certificates.

## Risk, recovery and default

The pure `reprice_cds_helpers` API diagnoses one complete `CdsCurveBuildInput`
against immutable supplied curves, without fitting them. It reuses original
recipe/contract admission and Standard183 price/quote functions, retaining all
source-order original terms, quote revisions, full valuations and signed native
residuals. Finite off-market diagnostics carry `within_tolerance:false`; they
are never an accepted fit. See the [shared API](LIBRARY-API.md#direct-cds-helper-repricing-diagnostics)
for supplied inputs, bounds and host qualification.

Prepare `PreparedCreditRisk` from the full supplied recipe, actual trade,
calendars, curve set, context and a caller-selected risk model. Preparation
performs the baseline fit and price. `CreditScenarioRequest` carries exactly one
axis and caller-assigned distinct hypothetical curve versions:

| Shock | Inputs changed | Required response |
|---|---|---|
| `DiscountZero` | One exact collateral DF grid; continuous-zero decimal shift | Hold survival or rebuild at original CDS quotes |
| `Quotes` | Explicit helper ID, original observation revision and native-unit delta | Rebuild all helpers |
| `Recovery` | Fractional recovery on the credit role and full terms | Hold survival or rebuild at original quotes |

Frozen survival outputs carry no fitted-quote certificate. Full recovery is
valid when survival is frozen; it cannot identify a quote-recalibrated hazard
curve. Quote scenarios keep rates/recovery fixed; recovery scenarios keep rates
and quotes fixed. Nonlinear PV changes are scenario effects, not additive P&L
attribution or realized ledger cash.

`credit_sensitivity` executes both sides of a supplied central bump. Rate PV01
and running CDS CS01 are signed currency for a +1 bp move; protection buyer and
seller have opposite signs. Upfront and recovery outputs are monetary derivatives
per unit fraction and cannot be inserted into a bp risk bucket. Each factor must
match the actual currency, target, dated tenor, quote type, basis, convention and
risk model. Quote bumps must exceed 32 original residual tolerances. Failed
repricing or out-of-domain central bumps never produce a zero sensitivity.
`validate_credit_risk_vector` rejects duplicate canonical factors, including
frozen and rebuilt variants; a job must select one response per factor.

`immediate_jump_to_default` is an immediate conditional scenario at the supplied
step-in day. It requires exact entity/seniority, an explicit settlement date and
fraction of default-day premium accrual. Unknown auction recovery/source returns
`Pending` without a monetary value. Supplied recovery produces signed protection
and accrued premium cash, discounted to valuation, and post-default minus
surviving PV. Zero and full auction recovery are valid. Pending original upfront
cash, later default paths and realized payments require lifecycle handling in
CVX-061/062; this API does not record an auction or payment.

## Bond/CDS hedge admission

`admit_bond_cds_hedge` requires an explicit `BondCdsMapping` over a resolved
`BondCreditIdentity`, full CDS reference, actual credit version and separate
`BondBasisInput`. Exact instrument/reference/entity/seniority/currency/recovery,
document/restructuring, basis and liquidity revisions must match. Dated residual
continuous-zero basis inputs must cover both actual maturities without a tail
proxy. Bond terms contain no issuer identity; none is inferred from a ticker.

The result is checked reusable admission input. It does not calculate a bond
credit price, calibrate the residual basis, manufacture a hedge ratio or declare
an executable hedge. Product valuation, liquidity/eligibility, optimization and
execution remain their owning tasks.

## Independent execution and regression evidence

The new `credit-isda-1.8.3` pack captures actual unmodified original ISDA 1.8.3 C
execution against six deterministic supplied CDS spreads and all 36 original
USD SOFR discount knots. Twenty-three base/rate/quote/recovery scenarios provide
276 clean/dirty/PV valuations. Running, mixed running/clean/dirty and global
native fits plus ten central sensitivities give 1,150 numeric checks. Declared
absolute bounds are hazard 1e-9, survival 1e-10, upfront 1e-10 and PV 1e-3 currency;
risk uses 0.1 currency plus 1e-6 relative. Original native quote tolerances and
the earlier 7,164-case grid's bounds remain unchanged.

```sh
python3 tests/scripts/isda_credit_compare.py check
cargo build --locked -p convex-analytics --example credit_comparison
python3 tests/scripts/isda_credit_compare.py offline --candidate-binary target/debug/examples/credit_comparison --report /tmp/credit-comparison.json
python3 -m unittest discover -s tests/scripts -p test_isda_credit_compare.py
```

Use a fresh report path. Offline mode executes the supplied binary, checks pinned
source/input/capture bytes, exact lineage/units/native residuals and every required
value, and retains raw stdout/stderr even on failure. The native control capture
is only a test-control replay, never an independent price oracle. Original C
capture requires the checksum-verified publisher source archive; no external
model is installed or linked into the production library. Attribution and the
separate [ISDA license](../licenses/ISDA-CDS-Standard-Model-Public-License-1.0.txt)
apply as described in [CDS valuation](CDS-VALUATION.md).

This is scoped engineering evidence. Full market/convention packs, native RFR
discount calibration, lifecycle, all host surfaces, independent human quantitative
approval and production/library/latency certificates remain pending. All 2,154
mandatory library requirements retain their existing Planned status.


Complete original product graph capture, fitted-output/solver/helper diagnostic
comparison and original quote-risk replay are documented in
[the schema-3 replay contract](CALIBRATION-SNAPSHOT-REPLAY.md). Pure native calculations remain provider/storage/MQ/clock neutral.
Existing schema1/2 identities and original numerical controls remain; CVX-063
and all mandatory library/market/model/host/production/latency certificates stay open.
