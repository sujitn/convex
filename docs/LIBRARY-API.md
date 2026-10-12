# Supplied-input library API (CVX-063)

Direct contextual `icma_yield_risk` actions `yield_sensitivity` and `hedge_vector`
retain full original cash, context, five valuations and exact own-yield factors.
See [required fields, bounds and hedge composition](ICMA-YIELD-RISK.md).

`convex::library` exposes checked native analytics through one closed request
contract. JSON, C, Java, Excel, MCP and WASM adapters call the same dispatcher.
The dispatcher reconstructs full native contracts and calls their owning product
modules. It contains no database, provider lookup, MQ, clock, order submission or
ledger update. The typed Rust product APIs remain available for prepared, repeated
calculations and application-specific composition.

**Acceptance is partial.** The new adapters and engineering corpus do not make
Convex function complete. Every original required LB0/LB1 operation stays in the
[capability report](../tools/library/api-capabilities.json). Unsupported native
models and operations remain required work; none have been waived. AC2 cannot be
accepted until each mandatory operation has real dispatch and its own positive,
boundary and failure examples. LB0/LB1 and production gates remain separate.

## Start and inspect a request

```sh
cargo run --locked -p convex --example library_workflow
cargo run --locked -p convex --example library_workflow --no-default-features
cargo run --locked -p convex --example library_workflow --features schemars
cargo run --locked -p convex --example library_workflow --features schemars -- --emit-schema
python3 tools/library/api.py
```

The example executes 110 complete requests: 96 admitted operations and 14 native
financial failures. [The input corpus](../tests/fixtures/library/cases.json)
contains original terms and explicit resources; it contains no financial expected
outputs. Select a request by its `id` and pass its `request` object as JSON text.
For example, `bond.construct`, `calibration.mixed_rates`, `credit.cs01`,
`optional.quote_risk`, `future.roll` and `lifecycle.zero` demonstrate the shapes.
Use the exact IDs listed in the corpus when scripting a consumer.

```rust,ignore
let request = convex::library::decode_request(request_json)?;
let value = convex::library::execute(&request)?;
let envelope_json = convex::library::execute_json(request_json);
```

`decode_request` performs strict, bounded decoding. `execute` performs native
construction, market admission and financial validation. Serializing a complete
typed request roundtrips its economic data. Do not replace checked construction
with deserialization of cached native objects or lifecycle state.

## Wire contract

A request has exactly `schema_version` (currently 1), `inputs` and `operation`.
`operation.kind` selects one of the closed operations below. Native instrument,
curve, market and event schema revisions remain independent of the library wire
version. An unsupported revision is an error, never a guessed migration.

`inputs` supplies these immutable values:

| Field | Meaning |
| --- | --- |
| `calendars` | Exact calendar identity/version, actual sorted unique holidays and weekday weekend rules; no calendar-name inference |
| `helper_calendars` | Explicit mapping from an older helper's opaque revision to an actual supplied calendar pin; no revision parsing or guessing |
| `curves` | Complete dated curve recipes, roles, domains, nodes/quotes, interpolation, extrapolation and numerical controls |
| `calibration` | Optional complete rate/credit/cash quote graph, original helpers, resource revisions and fit settings; actual rebuild replaces supplied baselines only through native graph admission |
| `fixings` | Full observations, publication times, sources and revisions; caller supplies eligible-history selection |
| `publications` | Actual publication requirements used to distinguish missing due history, awaiting publication and future observations |

Each bond supplies full `BondTerms` and four independent payment/reset/observation/
exercise pins. Each IRS/OIS/basis leg retains its own calendar, schedule, profile,
reset/observation, projection, collateral and sign conventions. CDS retains actual
entity, seniority, legal documents, recovery, settlement and survival mapping.
Futures retain the real contract, clearing, basket, conversion factors, delivery
window, source quality, carry and economic cutoffs. Model assumptions or dated
swaption calibration inputs are explicit. Calendar or curve IDs never select a
provider or authorize a lookup.

Success is `{"ok":"true","schema_version":1,"result":{"data":...,"market_calibration":...}}`.
`data` retains the operation's native output. `market_calibration` is null without
a graph, otherwise it retains the actual graph diagnostics and original helper
repricing. Solver roots, native residuals, limits, rank/condition diagnostics and
model/grid refinements remain visible. No partial graph becomes a successful
curve. Missing auction recovery or portfolio coverage stays explicitly pending
or incomplete when that is the native result's semantics.

Failure is `{"ok":"false","schema_version":1,"error":{"code":...,"message":...,"details":...}}`.
There is no successful zero substitute. Codes distinguish `invalid_input`,
`missing_input`, `not_applicable`, `unsupported`, `no_root`, `non_unique`,
`no_convergence`, `resource_limit`, `analytics` and `internal`. Details retain the
native missing manifest or numerical diagnostic when available. The existing C
boundary also reports pointer/UTF-8/admission/panic errors in its original error
envelope; those boundary failures can precede request decoding.

## Operations and financial units

| Operation | Native behavior and principal limits |
| --- | --- |
| `capabilities`, `roundtrip` | Complete required surface report; checked immutable input reconstruction/echo |
| `schedule`, `fixings` | Independent actual date rules and eligible-history requirements |
| `curves`, `curve_query`, `curve_fit` | Native construction, explicit step-zero/flat-forward sources and one-sided grid forwards, fallible date/tenor/DF/forward/survival/hazard queries and bounded dated money-market fitting |
| `parametric_fit` | Original Nelson–Siegel/Svensson continuous-zero observations, full initial parameters/bounds/native objective/rank/work controls and every native residual; no sampled-grid or product-price alias |
| `curve_time`, `rate_conversion` | Native bounded date/tenor inverse with explicit rounding, and discount/rate/compounding conversions over actual dates/day count or a supplied year fraction |
| `calibration` | Original full rate/CDS/cash graph build, product price, quote scenario, stable quote risk and calculated bucket reconciliation; preserves original helpers and history |
| `bond` | Construction, requirements, full entitled cash, fixed/floating model PV, dated fixed-cash yield or spread/DM inverse, guarded raw quote risk |
| `swap`, `basis_swap` | Original full IRS/OIS or two-floating-leg basis construction, requirements, projected cash and signed valuation |
| `cds`, `credit_risk` | Original full single-name pricing/conversion, calibrated scenarios, signed quote/recovery risk and pending or supplied JTD |
| `helper`, `repo_forward` | Native helper descriptions/build where admitted, complete rate/CDS/cash helper repricing against supplied curves, original cash quote conversions/basis-bond model and all dated repo cash horizons |
| `cash_security`, `government_spread` | Full observed cash-security quotation/invoice and actual-security government spread under an explicit common dated yield convention; preserves original helpers, quantities and independent reference/quote revisions |
| `icma_government_spread`, `icma_government_price` | Independently contextual ICMA actual-security spread and inverse; common policy/compounding, every benchmark, price/spread residuals and original signed consideration; see [contract](ICMA-GOVERNMENT-SPREADS.md) |
| `zero` | Full original zero terms, checked per100/yield pricing/risk; checked short-bill bank discount/BEY only for at most 182 actual days with face equal to absolute redemption |
| `optional_bond` | Original HW1F assumptions or dated swaption sigma calibration; supported price, OAS, scenario and separately refined risk |
| `workout`, `floating_workout` | Complete original conditional fixed cash/yield and floating projected cash/DM, full legal paths and purchase state; floating paths retain all puts without a worst-DM selection |
| `icma_workout` | Complete contextual fixed legal-path prices/yields and defined worst-call selection using original cash/notice owners; every source/time is admitted before numerical valuation; see [contract](ICMA-WORKOUTS.md) |
| `rate_future`, `bond_future` | Remaining fixing/projection/convexity model, quote risk and all-member bond delivery/CTD valuation/risk |
| `cash_hedge`, `futures_math`, `round_hedge`, `bond_cds_hedge` | Native indicative cash hedge, clearing settlement/tick/roll, integer hedge residual, and legal/recovery/basis admission |
| `hedge` | Complete supplied-factor current/pending aggregation and bounded constrained native-quantity/lot/cost proposals; inspect feasibility and work diagnostics; no execution authority |
| `aggregate` | Expected trade/factor manifest and same-snapshot FX; missing outcomes and unknown gross remain visible |
| `lifecycle` | Deterministic reconstruction/replay over full original terms, actual event inputs, receipts and exact signed quantities; proposed cash/security effects carry no posting authority |

Decimal terms, monetary values and exact native quotations use JSON **strings**.
Send `"99999999999999999.00000000001"` as text. Numeric JSON cash is rejected;
more precision than the native Decimal can retain is rejected. Native floating
model rates, computed per100 quote metrics and unrounded numerical diagnostics
retain their original number representation; the adapter does not claim new
precision for them. Native percent, basis-point, decimal-rate, futures-point,
original-face and absolute currency units remain distinct. In particular, the
short-bill conversion returns percent, while `BondMethod` yield/Z/DM and OAS are
annualized decimal rates. Risk keys and quantity/reference units must agree with
the actual native product. Do not infer semantics from a field called `value`.

Optional instruments cannot use plain bond pricing. Floating instruments cannot
use a contractual fixed-cash yield. Raw futures quote sensitivity is distinct
from remaining-rate risk and posted variation margin. OAS risk must pass native
price and duration/convexity refinement at both h and h/2. Price convergence
alone does not admit risk.

## Dated transformations and bucket reconciliation

`curve_time` uses the exact supplied curve revision and time basis. Its
`date_to_tenor` action takes an actual `date`; `tenor_to_date` takes `tenor` and
explicit `AtOrBefore`, `AtOrAfter` or `Nearest` rounding. Results retain the
curve key, reference date, time basis, original action and units. Leap years
use the native day count. Business-day adjustment is a separate schedule rule.

`rate_conversion` takes `period` as either
`{"kind":"dated","start":"2024-02-28","end":"2024-03-01","day_count":"ActActIsda"}`
or `{"kind":"year_fraction","value":0.5}`. Actions are `discount_from_rate`
with `rate` and `compounding`, `rate_from_discount` with `discount` and
`compounding`, or `convert_rate` with `rate`, `from` and `to`. Rates are annualized
decimals. Results retain the actual fraction, period, conventions and either
`discount_factor` or `decimal_rate` units. Native context-free day-count and
positive compounding-base guards apply. Missing ICMA references are unsupported.
The original native numerical-domain failures retain `no_convergence` and their
cause; no value is returned for an invalid simple/periodic base or inverse at
zero time. Discount at zero time is the native mathematical origin, one.

Exact representation, continuous-spread and roll transformations use the
existing full `CurveRecipe` in `inputs.curves`. `DiscountEquivalent` preserves
the base discount function. `Rolled` takes `StickyForward` or `StickyZero`, an
exact base dependency and a new reference date, with `DiscountFactors` output.
Sticky forward uses the original discount ratio over the actual date offset;
sticky zero preserves the remaining-tenor discount function. These are supplied
scenarios. Original interpolation/extrapolation is inherited without resampling;
backward rolls, changed methods, missing versions and an empty remaining original
domain fail. Query the new exact key with `curve_query`.

`calibration` action `reconcile_buckets` takes `input` containing a full `product`,
`buckets` of original `QuoteRiskKey`s, an explicit `parallel` key, actual
`position`, `snapshot_id`, original `settings` and a separate monetary
`tolerance`. It calculates every bucket and the simultaneous sensitivity with
the native product and graph. Each result keeps all five actual PVs, signed
quantity, units and half-step policy. Quote IDs/versions/native units must match
the declared parallel quote set. Node reconciliation checks every actual
informative grid index, including nodes with zero exposure; DF and hazard origins
are mathematical anchors. Duplicate, missing, incompatible or unstable inputs
yield no accepted partial result. A complete nonlinear set may return
`reconciliation.passed:false` with its actual discrepancy and bound.

This operation admits 1–128 buckets plus one parallel sensitivity. Each uses
the existing five-PV native reference route, with all nested helper/solver/work
limits still enforced. It returns computed records, and does not accept cached
bucket values. Optional-bond and bond-futures risk retain their dedicated native
model/refinement paths. This generic operation cannot bypass those guards.

Run the supplemental input-only corpus and inspect its 14 operation mappings:

```sh
cargo run --locked -p convex --example library_operations
cargo run --locked -p convex --example library_operations --no-default-features
cargo run --locked -p convex --example library_operations --features schemars
python3 tests/scripts/library_operations_inputs.py
python3 tools/library/api.py
```

The original 110 requests and evidence pins are unchanged. Another 110 supplied
operation requests exercise conversions, exact views and computed quote/zero/DF/
hazard reconciliation. Actual client outputs are compared to fresh native
envelopes for transport parity. These are engineering subsets; every original
required record remains Planned and CVX-063 remains partial.

## Full cash quotation, invoice and government spread

`cash_security` takes `input` containing a complete native `CashBondHelper` and
four independent `calendars` pins, an actual `reference_date`, an explicit
`DatedYieldConvention`, original `BondInverseSettings`, and `action` with kind
`quote` or `invoice`. Fixed, stepped, amortizing and bill securities retain their
full supplied cash profiles, principal prices, redemption, settlement and coupon
entitlement. Original clean/dirty/yield/bank-discount/par-rate quote conversion
and native admission run first. The dated yield inverse then uses the same
actual cash and observed dirty price, retaining its full final residual, original
controls and work. This convention does not infer Street, workout, floating or
optional-bond yields. Missing ICMA reference context remains unsupported.

An invoice action supplies the original `RiskPosition` with decimal text
`quantity` and `reference_quantity`, `quantity_unit:"notional"`, and exact
currency. The reference must equal the original face. Consideration uses the
original observed dirty price per100 times signed face quantity divided by100;
it does not use the approximate inverse reprice. A positive quantity gives
positive unrounded consideration, a short gives negative consideration, and
zero still checks all terms and resources. Nonfinite or underflowed nonzero
cash fails. `consideration` is native floating `ModelMoney`; it is not a rounded
ledger entry, outgoing-payment sign, receipt, fee or settlement authorization.

`government_spread` takes a full `target`, actual `benchmarks`, `reference_date`
and `GovernmentSpreadSettings`. Each benchmark supplies a complete helper, its
own calendar pins, an exact `security` reference pin and `benchmark_id`. The
security ID must equal the helper ID. Reference and observed quote revisions
remain separate. Benchmark membership is an explicit caller-resolved reference
classification; the library performs no issuer or provider lookup. All
securities must share target currency, settlement and coupon entitlement and
have unique IDs and actual contractual maturities. Credit-mapped benchmarks
are not admitted by this government operation.

Every original observed price is inverted under the same supplied dated yield
convention and original numerical controls, including unselected securities.
`ExactMaturity` requires one security with the exact target maturity.
`LinearMaturity` selects adjacent supplied maturities and weights their annual
decimal yields by actual calendar days, admitting only the closed supplied
maturity interval. Delayed cash-payment pillars do not replace contractual
maturity. Original source order, selected indices, dates and weights are
returned. G spread is target yield minus benchmark yield; both decimal spread
and fractional basis points are retained without rounding. A government zero
curve or an I-spread calculation cannot substitute for these actual securities.
There are at most 128 benchmarks; all original nested native work limits apply.

Run the 64-case supplied corpus and its separately declared 34 independent
Decimal(60) controls:

```sh
python3 tests/scripts/cash_quotation_reference.py
cargo run --locked -p convex --example library_cash
cargo run --locked -p convex --example library_cash --no-default-features
cargo run --locked -p convex --example library_cash --features schemars
python3 tools/library/api.py
```

The corpus covers 34 successes and 30 typed failures. Independent actual cash,
yield, invoice and actual-date spread expectations are declared separately in
[cash-reference.json](../tests/fixtures/library/cash-reference.json), before
native execution. New routes add four exact operation mappings; the current
report contains 18 mappings and 284 supplied requests across three unchanged or
additive packs. Synthetic engineering controls do not certify market
conventions, complete helper recipes or any original mandatory record.

## Rate-helper repricing on supplied curves

`helper` input kind `rate_reprice` takes a full `RateCalibrationInput` and an
explicit `CalibrationContext`. Supply actual curves in `inputs.curves`, or a
complete admitted graph in `inputs.calibration`. The helper recipe's output
key must already exist in that resolved set. This operation calls
`convex::rate_calibration::reprice_rate_helpers`; it performs no fit, publishes
no curve and never changes the caller's supplied values.

Each deposit, FRA, IRS/OIS, rate future and two-leg basis helper keeps its
original dates, profiles, native quote unit/tolerance, pillar, calendars and
exact curve/source revisions. The supplied curve must match the recipe's key,
role, currency, reference date, time basis, representation, interpolation and
extrapolation. A declared OIS zero-basis projection must be the actual named
`OvernightProjection` view of the exact discount revision. A separately supplied
node curve cannot impersonate that assumption.

Native recipe admission and complete missing-history preflight run before
valuation. Publication decisions are frozen once per index/date. Pricing uses
the original helper functions, including actual query-horizon, quote-profile,
linear par-exposure and exchange fixing/source guards. Known FRA observations
retain their full revision and require no forward query. Optional payoffs,
missing due fixings, mismatched resources and failed native calculations return
no partial result.

The result has `quote_versions` and `repricing` with the full original `recipe`,
source-order `quotes`, full `valuations`, consumed `publications`, measured
`price_evaluations` and `within_tolerance`. Each quote preserves its observed
input, native-unit model quote, signed model-minus-input residual and original
tolerance. An off-market finite residual is a diagnostic result with
`within_tolerance:false`. A successful envelope confirms that the calculation
completed; it does not assert that these curves are calibrated to the quotes.
The original calibration builder still rejects failed final repricing.

At most 128 helpers are admitted per recipe, with all original nested
history/calendar/valuation limits. This is a resource ceiling, not a latency
certificate. Keep prepared native inputs for repeated low-latency calculations.

```sh
python3 tests/scripts/library_rate_helpers_inputs.py
cargo run --locked -p convex --example library_rate_helpers
cargo run --locked -p convex --example library_rate_helpers --no-default-features
cargo run --locked -p convex --example library_rate_helpers --features schemars
```

The additional 64 supplied requests have 18 successful diagnostics and 46 typed
failures. Six exact helper mappings supplement the original 18 mappings and
284 requests. The original independent positive/negative mixed-rate controls
are reused without changing their quotes or bounds. Fresh client expectations
test transport only. These USD synthetic examples do not qualify every market,
helper recipe, fitting method or mandatory record. CVX-063 remains partial.

## Direct CDS-helper repricing diagnostics

`helper` input kind `credit_reprice` takes one complete original `CdsCurveRecipe`
in `recipe`. Supply the exact survival and discount curves in `inputs.curves`,
or an admitted graph in `inputs.calibration`. The output key must already exist.
The route delegates to `convex::credit_calibration::reprice_cds_helpers` and
performs no fit or curve replacement.

Full source-order helpers retain quote revisions, native running/clean/dirty
units, coupon/notional, legal entity/seniority/document/restructuring/recovery,
premium and settlement calendars, actual cash state and exact curve dependencies.
Shared native recipe admission is reused. The supplied survival curve must match
the exact key, role/recovery, currency, reference date, time basis, representation
and methods. Complete contracts are constructed before valuation; original
Standard183 pricing, integration-domain and model-quote guards remain active.
At most 128 helpers are admitted; 129 fails before adapter calendar resolution.
Declared global iteration/objective settings stay bounded, although no solver runs.

The result retains `recipe`, source-order `quotes`, full CDS `valuations`,
`within_tolerance` and actual `price_evaluations`. Running spread remains in its
declared decimal/percent/basis-point unit. Clean and dirty upfront remain distinct
buyer-to-seller fractions. Protection direction signs PV; it does not reverse
the quote convention. Pending versus settled contractual initial cash affects
outstanding PV and remains separate from fair market upfront.
The cash-state boundary cases supply an explicit zero-business-day settlement
lag on the actual valuation date. Premature settlement remains a typed failure.

Finite off-market residuals are successful diagnostics with
`within_tolerance:false`. They do not accept a calibration, identify hazard
parameters or replace curves. The existing builder still rejects supplied output
collisions and every failed final native residual. Any missing/invalid resource
or native failure returns no partial diagnostics.

```sh
python3 tests/scripts/library_credit_helpers_inputs.py
cargo run --locked -p convex --example library_credit_helpers
cargo run --locked -p convex --example library_credit_helpers --no-default-features
cargo run --locked -p convex --example library_credit_helpers --features schemars
```

The 50 additional supplied requests reuse the unchanged actual ISDA survival
capture and mixed quote controls. They include all three quote kinds, source
reversal, seller direction, pending/settled cash, negative discount rates, zero
hazard, maximum helpers and typed resource/unit/version/role/domain failures.
One exact USD credit mapping supplements the prior 24 mappings and 348 requests
(25 mappings and 398 requests total). These are engineering subsets; all original
2,154 mandatory records remain Planned. Actual unavailable Windows/Excel and
wasm32/browser execution remains unverified. CVX-063 is partial.

## Direct cash-helper repricing diagnostics

`helper` input kind `cash_reprice` takes the existing full `CashCalibrationInput`
in `input`: a complete original `CashCurveRecipe` and one exact calendar shape
per helper. Supply the output and every reference/credit dependency in
`inputs.curves`, or an admitted graph in `inputs.calibration`. The output key
must already exist. The route delegates to
`convex::cash_calibration::reprice_cash_helpers` and performs no fit or replacement.

Government helpers retain complete bills, stepped/amortizing cash bonds and par
coupon securities. Issuer helpers retain an explicit reference-zero-spread or
credit-par-recovery model and legal mapping. Repo helpers retain actual business
start/end dates, collateral and interest basis. Clean/dirty prices remain per
100 original face; dated-cash yield, bank discount, par coupon and simple repo
rates retain their decimal/percent/basis-point quotation units. Par coupons are
derived from complete zero/unit coupon exposures, never interpreted as spot zeros.
Promised and immediate recovery PV remain separate under the declared credit
model; no coupon/accrual recovery is inferred.

Original full recipe preparation checks every contract, quote, calendar,
settlement, cash horizon, model and dependency before valuation. The supplied
curve must match the exact key, role, currency, origin, time basis, representation
and methods. Model metadata and credit principal timing are checked before any
helper price. Fitting and diagnostics share one calendar resolver. There are at
most 128 helpers; 129 fails before calendar resolution. Original nested cash,
yield inverse, curve and work limits remain active even though no fit runs.

The result retains the full `recipe`, source-order `quotes`, complete `valuations`,
`within_tolerance` and actual `price_evaluations`. Signed residuals are model minus
input in each original unit. A finite off-market result succeeds with
`within_tolerance:false`; it does not accept a fitted curve. Any admission or
native pricing failure returns no partial result. Existing calibration collision,
identification and final residual guards remain unchanged.

```sh
python3 tests/scripts/library_cash_helpers_inputs.py
cargo run --locked -p convex --example library_cash_helpers
cargo run --locked -p convex --example library_cash_helpers --no-default-features
cargo run --locked -p convex --example library_cash_helpers --features schemars
```

The 98 additional requests (28 successful diagnostics and 70 typed failures)
reuse unchanged independent positive/negative cash controls and actual ISDA
credit dependency. They cover both declared basis models, original cash and
adjusted payment dates, all quoted rate units, source reversal, off-market
residuals, 128-helper boundaries and missing/invalid inputs. Four exact USD
mappings supplement the prior 25 mappings and 398 requests (29 mappings and
496 requests total). These engineering subsets leave every original mandatory
record Planned. Windows/Excel and actual wasm32/browser execution remain
unverified. CVX-063 remains partial.

## Clients and qualification

| Client | Entry point | Current qualification |
| --- | --- | --- |
| Rust | `convex::library::{execute,execute_json,decode_request}` | Actual engineering corpus and native error/roundtrip tests |
| C | `convex_library(const char*)` | Actual owned UTF-8 response parity; free every returned pointer with `convex_string_free` |
| Java | `Convex.library(String)` | Actual FFM/C transport parity on the recorded macOS ARM64 JDK host; other platform packaging remains a host gate |
| Excel | `CX.LIBRARY` and `CX.LIBRARY.RANGE` | Actual P/Invoke tests are wired into Windows CI; .NET and Excel host execution are unavailable locally and remain unverified |
| MCP | `library({request_json})` and `library_schema` | Actual tool handler text parity; remote transport and server authority remain separate |
| WASM | `library(request_json): string` and `library_schema(): string` | Native Rust export parity; wasm32/browser execution remains unverified on this host |

All generic client entry points accept and return raw JSON text. The generated
request schema is available with the Rust `schemars` feature, C
`convex_schema("LibraryRequest")`, MCP `library_schema` and WASM `library_schema`.
JSON Schema checks structure; actual native admission still checks financial
meaning, exact resource revisions, history, numerical limits and missing data.

For C/Java/Excel build the `excel` profile, which unwinds across the guarded host
boundary: `cargo build --locked --profile excel -p convex-ffi`. Rebuild/repackage
client native libraries together with the wrapper; an older library without
`convex_library` is incompatible and must not silently fall back.

Excel's one-cell function allows at most 32,760 UTF-16 characters.
`CX.LIBRARY.RANGE` concatenates textual cells in row order and spills the full raw
response in a single column. It preserves digits and Unicode pairs; it rejects
numeric cells and never truncates a long response. Combine returned chunks before
parsing JSON. Existing convenience functions retain their original documented
units/limitations. In particular, the older FRN entry points cannot supply full
history/publication/projection resources and fail explicitly. They do not gain
complete FRN support from the addition of a generic route.

Government zero-curve convenience routes, fields and controls have been removed;
use actual supplied securities through `government_spread`. See the
[removed routes and caller migration](ROUTE-MIGRATION.md), including Excel
positional arguments and WASM exports. Tenor-only bootstrap and indicative hedge
conveniences remain `compatibility_unverified`; they cannot establish AC3 for
complete multi-curve or hedge behavior. Earlier quantitative fixtures and bounds
stay protected.

## Bounds and deployment

Strict JSON admission limits input to 2 MiB, 64 levels, 32,768 nodes, 4,096 items
per collection and 8,192 UTF-8 bytes per text value. Unknown/discarded nested
fields, duplicate keys, trailing values and lossy numeric/string conversions
fail. Calendars and supplied curves are each limited to 256; calendar holidays
and replay events to 4,096. Output above 16 MiB is a typed failure with no partial
success. Native per-product/calendar/solver/helper limits apply as well. These
are admission ceilings, not a latency SLA or total CPU bound for nested work.

The pure request API can run on any process and server supplied the same immutable
inputs. Storage, messaging, provider integration, cache ownership, snapshot
selection and authorization belong to the application. Co-location and hot hedge
loops should use prepared typed native objects, retain their immutable source
pins and avoid serializing/recalibrating the full graph on each tick. Host timing,
resource budgets and fault handling require their own measurements and gates.

## Validation and remaining work

```sh
cargo test --locked -p convex --all-features --test library -p convex-ffi --test library -p convex-mcp --test library -p convex-wasm --test library
cargo test --locked -p convex-ffi -p convex-wasm -p convex-mcp
python3 -m unittest discover -s tools/library -p 'test_*.py'
python3 -m venv /tmp/convex-library-schema
/tmp/convex-library-schema/bin/python -m pip install -r tools/library/requirements-schema.txt
/tmp/convex-library-schema/bin/python tests/scripts/library_schema.py --binary target/debug/examples/library_workflow
```

Build `library_workflow` with `schemars` before the schema command. To run real
Java/native parity, build both the example and FFI library in the `excel` profile,
stage the library under `bindings/java/src/main/resources/native/<platform>/`,
then run:

```sh
python3 tests/scripts/library_parity.py prepare --binary target/excel/examples/library_workflow --output bindings/java/target/library-parity.json
mvn -B -f bindings/java/pom.xml verify
```

Fresh native parity outputs test transport only. They are never independent
financial oracles. Original quantitative fixtures, captures and tolerances remain
unchanged. The capability audit retains all 2,154 original required records,
174 combinations and 30 areas with their owners/surfaces/gates. Family examples
do not establish every model/convention/operation certificate.

AC2 still needs the remaining Street/make-whole/trigger/ex-coupon workout
conventions, required American/make-whole and FRN cap/floor/collar models, complete
curve method/calibration/risk and per-operation helper/aggregation/reconciliation coverage,
and authoritative server hedge orchestration (CVX-025/031). Every exact
combination still needs its original independent contract/model/convention pack.
Excel and actual browser/WASM qualification need executable hosts. See each
report record for its remaining owner, and use the private CVX-063 follow-up
harness prompt. CVX-064/065 may certify only actual completed mandatory scope.

## Complete workout quotations

The strict `workout` operation reconstructs complete original `BondInput` terms
and independently versioned calendars, reference and quote revisions. Native
workflow price/yield/worst actions retain explicit trade/purchase/notice state,
full stepped/amortizing cash, delayed claims, original-face units and native
inverse controls. Every actual continuous exercise date is inspected within
caller-declared work budgets. Puts are reported separately from the issuer-call/
maturity minimum; timely supplied notice forces its exact path. These are dated
cash quotations, with normal optional-model valuation guards unchanged. Read
[workout quotations](WORKOUT-QUOTATIONS.md) for request fields, original rounding,
complete diagnostics and unsupported conventions. Native envelopes remain
transport expectations; CVX-063 and original mandatory certificates stay open.

The additive `floating_workout` operation reconstructs the same complete term/
overnight contract with actual eligible history/publications and separate exact
projection/discount revisions. Price, same-cash DM and every eligible legal path
retain full cash, explicit shifted observation prefixes, original-face signed
consideration, notice and bounded inverse diagnostics. It freezes projected
coupons and carries no stochastic exercise or worst-DM meaning. Read
[floating workout quotations](FLOATING-WORKOUTS.md) for fields, controls and limits.

## Complete dated curve families

`CurveRecipe.source` admits full `Parametric`, `Segmented` and `Delegated` inputs.
Original Nelson–Siegel/Svensson parameters and native dated-zero residuals remain
separate from sampled interpolation grids. Segment sources retain exact full
dependency versions, finite coverage, fallback policies and checked C0 joins.
`curve_query` adds the original in-horizon parametric `InstantaneousForward`.
`parametric_fit` calls the original bounded solver with explicit native quote
scales, weights, closed parameter bounds and undamped rank/work controls; every
observation must pass its own residual bound, including zero-weight inputs.

Read [curve families](CURVE-FAMILIES.md) for all fields, query units, fit diagnostics,
source domain/extension disclosure, strict schema and executable supplied cases.
Bare parametric tags on discrete nodes still reject. The existing `calibration`
operation separately admits [full product-price parametric fits](PRODUCT-PARAMETRIC-CALIBRATION.md)
and original dependent quote-risk rebuilding with complete model/helper/solver
diagnostics. Credit/basis segment integration, remaining curve-risk/method
combinations and market/host certificates retain their original owners.

Complete [step-zero and flat-forward method contracts](CURVE-METHODS.md) retain original nodes/units/time basis, method/representation/role/currency/version and exact source/side/tail guards through the existing shared adapters. Native PiecewiseConstant linear and older raw FlatForward tail substitutions are removed; see [migration](ROUTE-MIGRATION.md). Product calibration, full quote-risk and actual unavailable-host qualification remain separate.

The [explicit UFR convergence contract](CURVE-UFR.md) retains the full exact
FlatForward or parametric base, supplied LLP, UFR and alpha. Native analytic left
forward/zero boundaries, stable integrated tails, tail-specific `UfrForward`
queries, actual date/time/unit/extension diagnostics and strict missing/domain
failures flow through the same generic clients. No derivative default, sampled
grid, regulatory Smith–Wilson or product-fit substitute is admitted.

The complete [nonnegative MonotoneConvex contract](CURVE-MONOTONE-CONVEX.md) exposes original positive dated/model-time nodes, exact native units, checked nonnegative means and all local regimes with a common analytic primitive. `MonotoneConvexForward` requires a side; native origin and declared terminal tails retain actual provenance. Strict schema rejects lost fields and undisclosed model policies. Full original product calibration and quote-risk graphs now have
[schema-3 capture/replay](CALIBRATION-SNAPSHOT-REPLAY.md); host certificates remain required. Complete original supplied recipes now survive [schema-2 engine snapshot replay](CURVE-SNAPSHOT-REPLAY.md).


Complete original product graph capture, fitted-output/solver/helper diagnostic
comparison and original quote-risk replay are documented in
[the schema-3 replay contract](CALIBRATION-SNAPSHOT-REPLAY.md). Pure native calculations remain provider/storage/MQ/clock neutral.
Existing schema1/2 identities and original numerical controls remain; CVX-063
and all mandatory library/market/model/host/production/latency certificates stay open.

Full-product exponential UFR/speed fitting over exact immutable analytic bases, required one-sided joined forwards, original quote-risk and schema3 replay use [the explicit UFR product contract](PRODUCT-UFR-CALIBRATION.md). Complete Helpers recipes and actual mixed-unit parameters/diagnostics are retained. No sampled objective, regulatory procedure or unsupported base is substituted.

Full actual-security G-spread-to-price is available through the additive
`government_spread_price` shared operation. It retains complete dated target
cash, independent reference/spread-quote revisions, every actual benchmark
inverse, signed consideration and separate price/spread roundtrip bounds.
See [the full inverse contract](GOVERNMENT-SPREAD-INVERSE.md). Original removed
convenience proxies remain absent; market/Street and unavailable host
qualification stay open. CVX-063 remains partial and uncertified.

Complete supplied-factor `hedge` actions `aggregate` and `solve` use
[the native constrained proposal owner](CONSTRAINED-HEDGES.md). Full original
five-PV risk/keys/settings/source pins, legal credit scope, explicit FX, signed
current/pending/reference quantities, costs, eligibility, hard factor/holding/
liquidity constraints and actual lot lattices survive strict shared/native/C
and generic clients. Only a completed `feasible` diagnostic carries a proposal;
inspect status, original constraints, rank and work. Every outcome remains
`proposal_only`. Separate provider freshness, authoritative limits, orders and
CVX-025/031 server tasks remain pending; the original scalar advisor stays
indicative. Original mandatory records and CVX-063 acceptance remain unchanged.

Explicit contextual ACT/ACT ICMA price and same-cash yield inversion are now
available over full original fixed/stepped/amortizing and legal workout cash.
The checked owner requires an independently versioned complete nominal reference
mesh, exact original policy/security/quote/calendar pins, payment coverage and
signed original-face quantities. See [the full contract](ICMA-QUOTATIONS.md).
Contractual accrual, quotation compounding and actual cash dates stay separate;
Street/model/market and unavailable host qualification remains open.


Existing `helper.cash_quote`, `helper.cash_reprice` and full `calibration` operations now admit the strict `ContextualIcmaYield` helper pack. Complete observed and model quotations, actual calendars, native units, full recipes and work diagnostics retain one original owner through graph rebuild and snapshot replay. See [ICMA cash helpers](ICMA-CASH-HELPERS.md); this adds no transport operation or C export.


## Complete common-factor hedge columns

The distinct `graph_hedge_risk` operation composes the original full product and
graph revaluation owners into complete bond/CDS/IRS/OIS/rates-futures columns
for the existing hedge aggregator and proposal solver. See
[quantity binding, inputs, controls and remaining limits](GRAPH-HEDGE-RISK.md).
Own-security ICMA yield remains a separate factor. Optional models, credit-survival
cash products and delivery-futures all-member risk remain owning work; actual
managed/browser and production/latency qualification remain open.


Credit-survival and issuer-basis cash now have a distinct full original-owner
`cash_basis_hedge_risk` route. See [cash basis hedge risk](CASH-BASIS-HEDGE-RISK.md)
for the complete security/mapping/factor contract, ICMA and rebuild controls,
actual mixed-book scope and remaining recovery/host/production qualification.

Complete guarded optional graph/model risk and hedge columns are documented in
[OPTIONAL-GRAPH-HEDGE-RISK.md](OPTIONAL-GRAPH-HEDGE-RISK.md).


Full all-member graph/scenario risk and fresh delivery-futures hedge columns,
including exact ties and CTD switches, are documented in
[Delivery futures basket risk](DELIVERY-FUTURES-HEDGE-RISK.md). Run
`cargo run --locked -p convex --example delivery_hedge`. This dedicated full
basket route retains the original fixed-date model and its unvalued delivery
options; it does not certify hosts, latency or server order authority.


Mixed-book full revaluation and model P&L attribution compose the original pure
product and graph owners, retaining every delivery CTD switch and separate cash
PV/futures quote-level meanings. See [the supplied-input contract](MIXED-BOOK-SCENARIOS.md)
and run `cargo run --locked -p convex --example mixed_scenario`. Explicit path,
quantity, FX and work admission, complete independent controls and dedicated
CI supplement existing checks; CVX-063 remains partial and uncertified.


The additive `book_pnl` route reconstructs complete causal information sets,
replays original lifecycle/cash facts and retains original nonlinear product
values and all-member CTD switches. Read [CAUSAL-BOOK-PNL.md](CAUSAL-BOOK-PNL.md)
for full inputs, distinct quote/cash bases, actual fills/receipts, attribution,
work bounds and explicit remaining limits. CVX-063 remains partial.

The additive `cohort_book_pnl` route retains exact original signed cohort cash
ownership and complete actual physical asset/invoice handoff, including all-member
CTD changes and actual-versus-model delivery choices. See
[COHORT-BOOK-PNL.md](COHORT-BOOK-PNL.md) for full terms, quantities, ownership and
explicit unsupported transfer/host/production boundaries.

The same route supports separately identified residual cash ownership and actual
partial security receipts. Run `cargo run --locked -p convex --example residual_pnl`.
Exact receipt cutoffs, original proposal availability, legal owned fractions,
signed cumulative face and full receipt provenance retain full causal mixed-book
P&L and independent all-member CTD winners/ties/switches. The original 77 routes
remain; no compatibility alias or scalar valuation route is added.

The existing `cohort_book_pnl` route also supports explicit full fixed-bond boundary amendment events and conserved original security lot splits. Full terms, original cash ownership and applied point facts cross the shared schema/C/MCP/native WASM path. Run `cargo run --locked -p convex --example lot_pnl`; see [the complete contract](COHORT-BOOK-PNL.md). Managed Java/Excel tests are supplied; actual host execution and certification remain unqualified locally.


The same `cohort_book_pnl` operation adds optional `claim_transfers`: complete
original earned cash/proposal/entitlement pins, legal parties and fractions, exact
receipt cutoffs, immutable sources and applied points. Outstanding ownership is
conserved across complete legal cohorts, including split successors; prior actual
cash remains with its original recipient. Root claims retain `transfer_ownership`
and recipients retain separate `transferred_claims` with original proposal and
receipt allocation provenance. Full native quantities/models, causal P&L and CTD
switches remain. Run `cargo run --locked -p convex --example claim_pnl`; see
[the complete original cash contract](COHORT-BOOK-PNL.md). Managed/browser and
production/latency qualification remains separate.

The same `cohort_book_pnl` operation also accepts `physical_partitions`: complete
legal custody splits over actual checked deliveries and original security receipt
cutoffs. `claim_transfers` may allocate the earned original delivery invoice from
its future proposal/receipt origin and linked initial cash asset. Custody and cash
recipients remain distinct; original receipts, signed face, full native products,
causal P&L and CTD changes remain. JSON omission defaults empty; Rust
`CohortBookPnlInput` literals add `physical_partitions: Vec::new()` and
`physical_disposals: Vec::new()` or complete actual facts. Successive same-delivery
partitions conserve remaining received stock and security receivables after
actual acquisition. `physical_disposals` binds existing actual fills to exact
original received-security allocations; it leaves remaining deliveries and invoice
cash independent. Full original ancestry, receipts, actual fills, consideration
and fees survive the causal P&L path. Paid cash recipients remain fixed.
No operation/version/C symbol changes. Run
`cargo run --locked -p convex --example custody_lineage_pnl` and see
[the complete custody contract](COHORT-BOOK-PNL.md). This scoped engineering
behavior does not confer actual managed/browser or production/latency certification.
