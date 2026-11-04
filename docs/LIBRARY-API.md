# Library API

`library()` is the single entry point for Convex's JSON request API. You send it
one JSON request that holds all the inputs (calendars, curves, fixings, terms)
and one operation. It returns one JSON envelope with either a result or an error.
The Rust implementation is `convex::library`, in the `convex` facade crate. It is
not on crates.io, so use a git dependency.

Every host calls the same dispatcher, so the same request has the same meaning
everywhere. Floating-point results can still differ slightly between hosts (see
[cohort book P&L](COHORT-BOOK-PNL.md) for wasm32):

| Host | Entry point |
| --- | --- |
| Rust | `convex::library::{execute, execute_json, decode_request}` |
| C | `convex_library(const char*)` |
| Java | `Convex.library(String)` |
| Excel | `CX.LIBRARY` and `CX.LIBRARY.RANGE` |
| MCP | `library({request_json})` and `library_schema` |
| WASM | `library(request_json): string` and `library_schema(): string` |

Each operation validates its input and returns an error instead of guessing. The
dispatcher reads nothing from a database, a data provider, a message queue or a
clock, and it does not submit orders or update a ledger. For repeated
calculations, use the typed Rust product APIs, which let you prepare inputs once.

The API is partial. Required convention, model and operation coverage is still
being built (see [library coverage](internal/LIBRARY-COVERAGE.md)), and nothing
here has independent financial approval or host, production or latency
certification. Each section's "Not yet covered" list says what is missing.

To try one request, run the example and read the request it prints:

```sh
cargo run --locked -p convex --example library_workflow
```

It executes 110 requests from [the input corpus](../tests/fixtures/library/cases.json).
Pick one by its `id` (for example `bond.construct` or `credit.cs01`) and pass its
`request` object, as JSON text, to any of the entry points above.

Status: not every planned operation and model is covered yet. The
[capability report](../tools/library/api-capabilities.json) lists what remains,
and the host checks under "Clients and verification status" are not all done.

Direct contextual `icma_yield_risk` actions `yield_sensitivity` and `hedge_vector`
keep the full cash, context, five valuations and exact own-yield factors. See
[required fields, bounds and hedge composition](ICMA-YIELD-RISK.md).

## Start and inspect a request

```sh
cargo run --locked -p convex --example library_workflow
cargo run --locked -p convex --example library_workflow --no-default-features
cargo run --locked -p convex --example library_workflow --features schemars
cargo run --locked -p convex --example library_workflow --features schemars -- --emit-schema
python3 tools/library/api.py
```

The example executes 110 complete requests: 96 successful operations and 14
financial failures. [The input corpus](../tests/fixtures/library/cases.json)
contains terms and explicit resources; it contains no expected financial outputs.
Select a request by its `id` and pass its `request` object as JSON text. For
example, `bond.construct`, `calibration.mixed_rates`, `credit.cs01`,
`optional.quote_risk`, `future.roll` and `lifecycle.zero` show the shapes. Use the
exact IDs listed in the corpus when scripting a consumer.

```rust,ignore
let request = convex::library::decode_request(request_json)?;
let value = convex::library::execute(&request)?;
let envelope_json = convex::library::execute_json(request_json);
```

`decode_request` performs strict, bounded decoding. `execute` constructs the
contracts, validates the market data and checks the financial inputs. Serializing
a complete typed request roundtrips its economic data. Do not replace this
validated construction by deserializing cached Rust objects or lifecycle state.

## Wire contract

A request has exactly `schema_version` (currently 1), `inputs` and `operation`.
`operation.kind` selects one of the operations below; no others exist. The
instrument, curve, market and event schema revisions are independent of the
library wire version. An unsupported revision is an error, never a guessed
migration.

`inputs` holds these immutable values:

| Field | Meaning |
| --- | --- |
| `calendars` | Exact calendar identity and version, sorted unique holidays and weekday weekend rules; the calendar is never inferred from its name |
| `helper_calendars` | Explicit mapping from an older helper's opaque revision to a calendar pin you pass in; revisions are never parsed or guessed |
| `curves` | Complete dated curve recipes, roles, domains, nodes/quotes, interpolation, extrapolation and numerical controls |
| `calibration` | Optional complete rate/credit/cash quote graph, helpers, resource revisions and fit settings; a rebuild replaces the baselines you passed in only after the graph validates |
| `fixings` | Full observations, publication times, sources and revisions; the caller selects the eligible history |
| `publications` | Publication requirements used to tell missing due history, awaiting publication and future observations apart |

Each bond carries full `BondTerms` and four independent payment, reset,
observation and exercise pins. Each IRS, OIS or basis leg keeps its own calendar,
schedule, profile, reset and observation rules, projection, collateral and sign
conventions. A CDS keeps its entity, seniority, legal documents, recovery,
settlement and survival mapping. Futures keep the contract, clearing, basket,
conversion factors, delivery window, source quality, carry and economic cutoffs.
Model assumptions and dated swaption calibration inputs are explicit. A calendar
or curve ID never selects a provider or triggers a lookup.

Success is `{"ok":"true","schema_version":1,"result":{"data":...,"market_calibration":...}}`.
`data` holds the operation's output. `market_calibration` is null without a
graph; otherwise it holds the graph diagnostics and the helper repricing. Solver
roots, residuals, limits, rank and condition diagnostics, and model and grid
refinements are all visible. A partial graph never becomes a successful curve.
Missing auction recovery or portfolio coverage is reported as pending or
incomplete where that is what the result means.

Failure is `{"ok":"false","schema_version":1,"error":{"code":...,"message":...,"details":...}}`.
A failure never returns a zero in place of a result. The codes are `invalid_input`,
`missing_input`, `not_applicable`, `unsupported`, `no_root`, `non_unique`,
`no_convergence`, `resource_limit`, `analytics` and `internal`. Details hold the
list of missing inputs or the numerical diagnostic when there is one. The C
boundary also reports pointer, UTF-8, input-validation and panic errors in its
own error envelope. Those errors can occur before the request is decoded.

## Operations and financial units

| Operation | Behavior and principal limits |
| --- | --- |
| `capabilities`, `roundtrip` | Report of the supported surface; validated reconstruction and echo of the inputs |
| `schedule`, `fixings` | Independent date rules and eligible-history requirements |
| `curves`, `curve_query`, `curve_fit` | Curve construction, explicit step-zero and flat-forward sources, one-sided grid forwards, date/tenor/DF/forward/survival/hazard queries that can fail, and bounded dated money-market fitting |
| `parametric_fit` | Nelson–Siegel/Svensson continuous-zero observations, full initial parameters, bounds, objective, rank and work controls, and every residual; no sampled-grid or product-price substitute |
| `curve_time`, `rate_conversion` | Bounded date/tenor inverse with explicit rounding, and discount/rate/compounding conversions over dates and a day count, or over a year fraction you pass in |
| `calibration` | Full rate/CDS/cash graph build, product price, quote scenario, stable quote risk and calculated bucket reconciliation; keeps the helpers and history |
| `bond` | Construction, requirements, full entitled cash, fixed and floating model PV, dated fixed-cash yield or spread/DM inverse, guarded raw quote risk |
| `swap`, `basis_swap` | Full IRS/OIS or two-floating-leg basis construction, requirements, projected cash and signed valuation |
| `cds`, `credit_risk` | Full single-name pricing and conversion, calibrated scenarios, signed quote and recovery risk, and pending or supplied JTD |
| `helper`, `repo_forward` | Helper descriptions and builds where supported, complete rate/CDS/cash helper repricing against the curves you pass in, cash quote conversions, the basis-bond model and all dated repo cash horizons |
| `cash_security`, `government_spread` | Full observed cash-security quotation and invoice, and government spread against actual securities, under an explicit common dated yield convention; keeps the helpers, quantities and the separate reference and quote revisions |
| `icma_government_spread`, `icma_government_price` | Contextual ICMA spread against actual securities, and its inverse; common policy and compounding, every benchmark, price and spread residuals and the signed consideration; see [contract](ICMA-GOVERNMENT-SPREADS.md) |
| `zero` | Full zero terms, validated per100/yield pricing and risk; short-bill bank discount/BEY only for at most 182 days with face equal to absolute redemption |
| `optional_bond` | HW1F assumptions or dated swaption sigma calibration; supported price, OAS, scenario and separately refined risk |
| `workout`, `floating_workout` | Conditional fixed cash and yield, and floating projected cash and DM, with full legal paths and purchase state; floating paths keep all puts without a worst-DM selection |
| `icma_workout` | Contextual fixed legal-path prices and yields and a defined worst-call selection; every source and time is validated before valuation; see [contract](ICMA-WORKOUTS.md) |
| `rate_future`, `bond_future` | Remaining fixing/projection/convexity model, quote risk and all-member bond delivery/CTD valuation/risk |
| `cash_hedge`, `futures_math`, `round_hedge`, `bond_cds_hedge` | Indicative cash hedge, clearing settlement/tick/roll, integer hedge residual, and legal/recovery/basis validation |
| `hedge` | Current/pending aggregation over the factors you pass in, and bounded constrained quantity/lot/cost proposals; inspect feasibility and work diagnostics; no execution authority |
| `aggregate` | Expected trade/factor manifest and same-snapshot FX; missing outcomes and unknown gross remain visible |
| `lifecycle` | Deterministic reconstruction and replay over full terms, event inputs, receipts and exact signed quantities; proposed cash and security effects carry no posting authority |

Decimal terms, monetary values and exact quotations use JSON **strings**.
Send `"99999999999999999.00000000001"` as text. Numeric JSON cash is rejected, and
so is more precision than the Rust Decimal can hold. Floating-point model rates,
computed per100 quote metrics and unrounded numerical diagnostics keep their
floating-point representation; the adapter does not add precision to them.

Percent, basis-point, decimal-rate, futures-point, original-face and absolute
currency units stay distinct. In particular, the short-bill conversion returns
percent, while `BondMethod` yield/Z/DM and OAS are annualized decimal rates. Risk
keys and quantity and reference units must agree with the product. Do not infer
meaning from a field called `value`.

Optional instruments cannot use plain bond pricing. Floating instruments cannot
use a contractual fixed-cash yield. Raw futures quote sensitivity is distinct
from remaining-rate risk and posted variation margin. OAS risk must pass the
price and duration/convexity refinement at both h and h/2. Price convergence
alone is not enough.

## Dated transformations and bucket reconciliation

`curve_time` uses the exact curve revision and time basis you pass in. Its
`date_to_tenor` action takes an actual `date`; `tenor_to_date` takes `tenor` and
explicit `AtOrBefore`, `AtOrAfter` or `Nearest` rounding. Results keep the
curve key, reference date, time basis, action and units. Leap years
follow the day count. Business-day adjustment is a separate schedule rule.

`rate_conversion` takes `period` as either
`{"kind":"dated","start":"2024-02-28","end":"2024-03-01","day_count":"ActActIsda"}`
or `{"kind":"year_fraction","value":0.5}`. Actions are `discount_from_rate`
with `rate` and `compounding`, `rate_from_discount` with `discount` and
`compounding`, or `convert_rate` with `rate`, `from` and `to`. Rates are annualized
decimals. Results keep the fraction, period, conventions and either
`discount_factor` or `decimal_rate` units. Context-free day-count and positive
compounding-base checks apply. Missing ICMA references are unsupported. Numerical
domain failures report `no_convergence` and their cause. No value is returned
for an invalid simple or periodic base, or for an inverse at zero time. Discount
at zero time is the mathematical origin, one.

Exact representation, continuous-spread and roll transformations use the
full `CurveRecipe` in `inputs.curves`. `DiscountEquivalent` preserves
the base discount function. `Rolled` takes `StickyForward` or `StickyZero`, an
exact base dependency and a new reference date, with `DiscountFactors` output.
Sticky forward uses the discount ratio over the date offset; sticky zero
preserves the remaining-tenor discount function. These are scenarios you define.
Interpolation and extrapolation are inherited without resampling. Backward
rolls, changed methods, missing versions and an empty remaining domain fail. Query the new exact key with `curve_query`.

`calibration` action `reconcile_buckets` takes an `input` containing a full
`product`, `buckets` of `QuoteRiskKey`s, an explicit `parallel` key, a
`position`, `snapshot_id`, `settings` and a separate monetary `tolerance`. It
calculates every bucket and the simultaneous sensitivity with the product and
graph. Each result keeps all five PVs, the signed quantity, units and the
half-step policy. Quote IDs, versions and units must match
the declared parallel quote set. Node reconciliation checks every informative
grid index, including nodes with zero exposure; DF and hazard origins
are mathematical anchors. Duplicate, missing, incompatible or unstable inputs
produce no partial result. A complete nonlinear set may return
`reconciliation.passed:false` with its discrepancy and bound.

This operation accepts 1–128 buckets plus one parallel sensitivity. Each uses
the existing five-PV route, with all nested helper, solver and work limits still
enforced. It returns computed records and does not accept cached bucket values.
Optional-bond and bond-futures risk keep their dedicated model and refinement
paths, and this generic operation cannot bypass those checks.

Run the additional input-only corpus and inspect its 14 operation mappings:

```sh
cargo run --locked -p convex --example library_operations
cargo run --locked -p convex --example library_operations --no-default-features
cargo run --locked -p convex --example library_operations --features schemars
python3 tests/scripts/library_operations_inputs.py
python3 tools/library/api.py
```

The first 110 requests and their evidence pins are unchanged. Another 110
requests exercise conversions, exact views and computed quote/zero/DF/hazard
reconciliation. Client outputs are compared with freshly generated Rust
envelopes to confirm that every host returns the same text.

## Full cash quotation, invoice and government spread

`cash_security` takes `input` containing a complete native `CashBondHelper` and
four independent `calendars` pins, an actual `reference_date`, an explicit
`DatedYieldConvention`, original `BondInverseSettings`, and `action` with kind
`quote` or `invoice`. Fixed, stepped, amortizing and bill securities retain their
full supplied cash profiles, principal prices, redemption, settlement and coupon
entitlement. Original clean/dirty/yield/bank-discount/par-rate quote conversion
and native validation run first. The dated yield inverse then uses the same
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
are not accepted by this government operation.

Every original observed price is inverted under the same supplied dated yield
convention and original numerical controls, including unselected securities.
`ExactMaturity` requires one security with the exact target maturity.
`LinearMaturity` selects adjacent supplied maturities and weights their annual
decimal yields by calendar days, accepting only the closed supplied
maturity interval. Delayed cash-payment pillars do not replace contractual
maturity. Original source order, selected indices, dates and weights are
returned. G spread is target yield minus benchmark yield; both decimal spread
and fractional basis points are retained without rounding. A government zero
curve or an I-spread calculation cannot substitute for these actual securities.
There are at most 128 benchmarks; all original nested native work limits apply.

Run the 64-case input corpus and its separately declared 34 independent
Decimal(60) controls:

```sh
python3 tests/scripts/cash_quotation_reference.py
cargo run --locked -p convex --example library_cash
cargo run --locked -p convex --example library_cash --no-default-features
cargo run --locked -p convex --example library_cash --features schemars
python3 tools/library/api.py
```

The corpus covers 34 successes and 30 typed failures. Independent expected values
for cash, yield, invoice and dated spread are declared separately in
[cash-reference.json](../tests/fixtures/library/cash-reference.json), before the
Rust code was run. New routes add four exact operation mappings; the current
report contains 18 mappings and 284 requests across three packs. The synthetic
controls have not been validated against market conventions.

## Rate-helper repricing on supplied curves

`helper` input kind `rate_reprice` takes a full `RateCalibrationInput` and an
explicit `CalibrationContext`. Pass the curves in `inputs.curves`, or a
complete validated graph in `inputs.calibration`. The helper recipe's output
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

Native recipe validation and complete missing-history preflight run before
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

At most 128 helpers are accepted per recipe, with all original nested
history/calendar/valuation limits. This is a resource ceiling, not a latency
guarantee. Keep prepared Rust inputs for repeated low-latency calculations.

```sh
python3 tests/scripts/library_rate_helpers_inputs.py
cargo run --locked -p convex --example library_rate_helpers
cargo run --locked -p convex --example library_rate_helpers --no-default-features
cargo run --locked -p convex --example library_rate_helpers --features schemars
```

The additional 64 requests have 18 successful diagnostics and 46 typed
failures. Six exact helper mappings add to the earlier 18 mappings and
284 requests. The independent positive and negative mixed-rate controls
are reused without changing their quotes or bounds. Freshly generated client
expectations only test that the hosts agree. These USD synthetic examples do not
cover every market, helper recipe or fitting method.

## Direct CDS-helper repricing diagnostics

`helper` input kind `credit_reprice` takes one complete original `CdsCurveRecipe`
in `recipe`. Supply the exact survival and discount curves in `inputs.curves`,
or an accepted graph in `inputs.calibration`. The output key must already exist.
The route delegates to `convex::credit_calibration::reprice_cds_helpers` and
performs no fit or curve replacement.

Full source-order helpers retain quote revisions, native running/clean/dirty
units, coupon/notional, legal entity/seniority/document/restructuring/recovery,
premium and settlement calendars, actual cash state and exact curve dependencies.
Shared native recipe validation is reused. The supplied survival curve must match
the exact key, role/recovery, currency, reference date, time basis, representation
and methods. Complete contracts are constructed before valuation; original
Standard183 pricing, integration-domain and model-quote guards remain active.
At most 128 helpers are accepted; 129 fails before adapter calendar resolution.
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

The 50 additional requests reuse the unchanged ISDA survival
capture and mixed quote controls. They include all three quote kinds, source
reversal, seller direction, pending/settled cash, negative discount rates, zero
hazard, maximum helpers and typed resource/unit/version/role/domain failures.
One exact USD credit mapping adds to the prior 24 mappings and 348 requests
(25 mappings and 398 requests total). Windows/Excel and wasm32/browser execution
has not been verified.

## Direct cash-helper repricing diagnostics

`helper` input kind `cash_reprice` takes the existing full `CashCalibrationInput`
in `input`: a complete original `CashCurveRecipe` and one exact calendar shape
per helper. Supply the output and every reference/credit dependency in
`inputs.curves`, or an accepted graph in `inputs.calibration`. The output key
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
`within_tolerance:false`; it does not accept a fitted curve. Any validation or
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
mappings add to the prior 25 mappings and 398 requests (29 mappings and
496 requests total). Windows/Excel and wasm32/browser execution has not been
verified.

## Clients and verification status

| Client | Entry point | Current verification |
| --- | --- | --- |
| Rust | `convex::library::{execute,execute_json,decode_request}` | Corpus run, plus Rust error and roundtrip tests |
| C | `convex_library(const char*)` | Response text matches Rust; free every returned pointer with `convex_string_free` |
| Java | `Convex.library(String)` | FFM/C output matches Rust on a macOS ARM64 JDK; packaging for other platforms is not verified. Not yet on Maven Central |
| Excel | `CX.LIBRARY` and `CX.LIBRARY.RANGE` | P/Invoke tests run in Windows CI; .NET and Excel hosts have not been run locally. Ships as a zip on GitHub releases |
| MCP | `library({request_json})` and `library_schema` | Tool handler output matches Rust; remote transport and server authority are separate |
| WASM | `library(request_json): string` and `library_schema(): string` | Rust export output matches; wasm32/browser execution is not verified. Ships as npm tarballs on GitHub releases |

All generic client entry points accept and return raw JSON text. The generated
request schema is available with the Rust `schemars` feature, C
`convex_schema("LibraryRequest")`, MCP `library_schema` and WASM `library_schema`.
JSON Schema checks structure only. The Rust code still checks financial
meaning, exact resource revisions, history, numerical limits and missing data.

For C/Java/Excel build the `excel` profile, which unwinds across the guarded host
boundary: `cargo build --locked --profile excel -p convex-ffi`. Rebuild and repackage
the client's native libraries together with the wrapper; an older library without
`convex_library` is incompatible and must not silently fall back.

Excel's one-cell function allows at most 32,760 UTF-16 characters.
`CX.LIBRARY.RANGE` concatenates textual cells in row order and spills the full raw
response in a single column. It preserves digits and Unicode pairs; it rejects
numeric cells and never truncates a long response. Combine returned chunks before
parsing JSON. Existing convenience functions keep their documented units and limits. The older
FRN entry points cannot take full history, publication or projection resources,
and they fail with an explicit error. The generic route does not add complete
FRN support to them.

Government zero-curve convenience routes, fields and controls have been removed;
use actual securities through `government_spread`. See the
[removed routes and caller migration](internal/ROUTE-MIGRATION.md), including Excel
positional arguments and WASM exports. Tenor-only bootstrap and indicative hedge
conveniences remain `compatibility_unverified`, which means they have not been
checked for complete multi-curve or hedge behavior. Earlier quantitative
fixtures and bounds are unchanged.

## Bounds and deployment

Strict JSON parsing limits input to 2 MiB, 64 levels, 32,768 nodes, 4,096 items
per collection and 8,192 UTF-8 bytes per text value. Unknown/discarded nested
fields, duplicate keys, trailing values and lossy numeric/string conversions
fail. Calendars and curves are each limited to 256; calendar holidays
and replay events to 4,096. Output above 16 MiB is a typed failure with no partial
success. Per-product, calendar, solver and helper limits apply as well. These
are input ceilings, not a latency guarantee or a total CPU bound for nested work.

The request API is pure: any process or server given the same immutable inputs
computes the same result. Storage, messaging, provider integration, cache
ownership, snapshot selection and authorization belong to the application.
Co-located and hot hedge loops should use prepared typed Rust objects, keep their
immutable source pins, and avoid serializing or recalibrating the full graph on
each tick. Measure host timing, resource budgets and fault handling yourself.

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

Freshly generated Rust outputs only test that the hosts agree; they are not an
independent check of the financial results. The quantitative fixtures, captures
and tolerances are unchanged. The capability report lists all 2,154 required
records, 174 combinations and 30 areas.

Not yet covered:

- Street, make-whole, trigger and ex-coupon workout conventions.
- American and make-whole models, and FRN cap, floor and collar models.
- Complete curve method, calibration and risk coverage, and per-operation
  helper, aggregation and reconciliation coverage.
- Server-side hedge orchestration.

Every exact combination still needs its own independent contract, model and
convention test pack. Excel and browser/WASM checks need hosts that can run them.
Each record in the capability report lists what remains.

## Complete workout quotations

The strict `workout` operation rebuilds the full `BondInput` terms and the
separately versioned calendars, reference and quote revisions. The price, yield
and worst actions keep explicit trade, purchase and notice state, full stepped
and amortizing cash, delayed claims, original-face units and the inverse
controls. Every continuous exercise date is inspected within work budgets that
the caller declares. Puts are reported separately from the issuer-call/
maturity minimum; timely supplied notice forces its exact path. These are dated
cash quotations, with normal optional-model valuation guards unchanged. Read
[workout quotations](WORKOUT-QUOTATIONS.md) for request fields, rounding,
diagnostics and unsupported conventions.

The `floating_workout` operation rebuilds the same term or overnight contract
with its eligible history and publications and separate exact projection and
discount revisions. Price, same-cash DM and every eligible legal path
retain full cash, explicit shifted observation prefixes, original-face signed
consideration, notice and bounded inverse diagnostics. It freezes projected
coupons and carries no stochastic exercise or worst-DM meaning. Read
[floating workout quotations](FLOATING-WORKOUTS.md) for fields, controls and limits.

## Complete dated curve families

`CurveRecipe.source` accepts full `Parametric`, `Segmented` and `Delegated` inputs.
Nelson–Siegel/Svensson parameters and dated-zero residuals stay separate from
sampled interpolation grids. Segment sources retain exact full
dependency versions, finite coverage, fallback policies and checked C0 joins.
`curve_query` adds the in-horizon parametric `InstantaneousForward`.
`parametric_fit` calls the bounded solver with explicit quote scales, weights, closed parameter bounds and undamped rank/work controls; every
observation must pass its own residual bound, including zero-weight inputs.

Read [curve families](CURVE-FAMILIES.md) for all fields, query units, fit diagnostics,
source domain/extension disclosure, strict schema and executable supplied cases.
Bare parametric tags on discrete nodes still reject. The existing `calibration`
operation separately accepts [full product-price parametric fits](PRODUCT-PARAMETRIC-CALIBRATION.md)
and dependent quote-risk rebuilding with complete model, helper and solver
diagnostics. Credit/basis segment integration and some curve-risk and method
combinations are not yet covered.

Complete [step-zero and flat-forward method contracts](CURVE-METHODS.md) retain original nodes/units/time basis, method/representation/role/currency/version and exact source/side/tail guards through the existing shared adapters. The linear substitution for PiecewiseConstant and the older raw FlatForward tail substitutions are removed; see [migration](internal/ROUTE-MIGRATION.md). Product calibration and full quote-risk are separate.

The [explicit UFR convergence contract](CURVE-UFR.md) retains the full exact
FlatForward or parametric base, supplied LLP, UFR and alpha. Native analytic left
forward/zero boundaries, stable integrated tails, tail-specific `UfrForward`
queries, actual date/time/unit/extension diagnostics and strict missing/domain
failures flow through the same generic clients. No derivative default, sampled
grid, regulatory Smith–Wilson or product-fit substitute is accepted.

The complete [nonnegative MonotoneConvex contract](CURVE-MONOTONE-CONVEX.md) exposes original positive dated/model-time nodes, exact native units, checked nonnegative means and all local regimes with a common analytic primitive. `MonotoneConvexForward` requires a side; native origin and declared terminal tails retain actual provenance. Strict schema rejects lost fields and undisclosed model policies. Full product calibration and quote-risk graphs have
[schema-3 capture and replay](CALIBRATION-SNAPSHOT-REPLAY.md). Complete recipes survive [schema-2 engine snapshot replay](CURVE-SNAPSHOT-REPLAY.md).


Product graph capture, comparison of fitted outputs and solver and helper
diagnostics, and quote-risk replay are documented in
[the schema-3 replay contract](CALIBRATION-SNAPSHOT-REPLAY.md). The calculations
stay independent of providers, storage, message queues and clocks. Existing
schema 1 and 2 identities and numerical controls are unchanged.

Full-product exponential UFR/speed fitting over exact immutable analytic bases, required one-sided joined forwards, original quote-risk and schema3 replay use [the explicit UFR product contract](PRODUCT-UFR-CALIBRATION.md). Complete Helpers recipes and actual mixed-unit parameters/diagnostics are retained. No sampled objective, regulatory procedure or unsupported base is substituted.

Full G-spread-to-price against actual securities is available through the
`government_spread_price` shared operation. It retains complete dated target
cash, independent reference/spread-quote revisions, every actual benchmark
inverse, signed consideration and separate price/spread roundtrip bounds.
See [the full inverse contract](GOVERNMENT-SPREAD-INVERSE.md). The removed
convenience proxies stay removed.

The `hedge` actions `aggregate` and `solve`, given complete factors, use
[the constrained proposal code](CONSTRAINED-HEDGES.md). Full original
five-PV risk/keys/settings/source pins, legal credit scope, explicit FX, signed
current/pending/reference quantities, costs, eligibility, hard factor/holding/
liquidity constraints and actual lot lattices survive strict shared/native/C
and generic clients. Only a completed `feasible` diagnostic carries a proposal;
inspect status, original constraints, rank and work. Every outcome remains
`proposal_only`. Provider freshness, authoritative limits, orders and the server-side tasks are
not yet covered; the scalar advisor stays indicative.

Contextual ACT/ACT ICMA price and same-cash yield inversion are available over
fixed, stepped and amortizing cash and legal workout cash. They require a
separately versioned, complete nominal reference mesh, exact policy, security,
quote and calendar pins, payment coverage, and signed quantities in units of
face. See [the full contract](ICMA-QUOTATIONS.md). Contractual accrual,
quotation compounding and cash dates stay separate.


The existing `helper.cash_quote`, `helper.cash_reprice` and full `calibration` operations now accept the strict `ContextualIcmaYield` helper pack. Observed and model quotations, calendars, units, full recipes and work diagnostics are kept through graph rebuild and snapshot replay. See [ICMA cash helpers](ICMA-CASH-HELPERS.md); this adds no transport operation or C export.


## Complete common-factor hedge columns

The distinct `graph_hedge_risk` operation composes the original full product and
graph revaluation owners into complete bond/CDS/IRS/OIS/rates-futures columns
for the existing hedge aggregator and proposal solver. See
[quantity binding, inputs, controls and remaining limits](GRAPH-HEDGE-RISK.md).
Own-security ICMA yield remains a separate factor. Not yet covered by this route:
optional models, credit-survival cash products and delivery-futures all-member
risk.


Credit-survival and issuer-basis cash have their own `cash_basis_hedge_risk`
route. See [cash basis hedge risk](CASH-BASIS-HEDGE-RISK.md) for the security,
mapping and factor contract, the ICMA and rebuild controls, the mixed-book scope
and the remaining recovery limits.

Complete guarded optional graph/model risk and hedge columns are documented in
[OPTIONAL-GRAPH-HEDGE-RISK.md](OPTIONAL-GRAPH-HEDGE-RISK.md).


Full all-member graph/scenario risk and fresh delivery-futures hedge columns,
including exact ties and CTD switches, are documented in
[Delivery futures basket risk](DELIVERY-FUTURES-HEDGE-RISK.md). Run
`cargo run --locked -p convex --example delivery_hedge`. This dedicated full
basket route keeps the fixed-date model and its unvalued delivery
options. It makes no claim about hosts, latency or server order authority.


Mixed-book full revaluation and model P&L attribution compose the original pure
product and graph owners, retaining every delivery CTD switch and separate cash
PV/futures quote-level meanings. See [the input contract](MIXED-BOOK-SCENARIOS.md)
and run `cargo run --locked -p convex --example mixed_scenario`. Explicit path,
quantity, FX and work limits, independent controls and dedicated CI add to the
existing checks.


The additive `book_pnl` route reconstructs complete causal information sets,
replays original lifecycle/cash facts and retains original nonlinear product
values and all-member CTD switches. Read [CAUSAL-BOOK-PNL.md](CAUSAL-BOOK-PNL.md)
for full inputs, distinct quote/cash bases, actual fills/receipts, attribution,
work bounds and the remaining limits.

The supplied `lifecycle_hedge_risk` operation composes original graph scenarios
with the same replayed native state and causal quantity/fill history. Actual
current risk includes outstanding owned cash; pending exposure and fresh hedge
candidates retain separate amounts. The existing `hedge` operation accepts
`solve_lifecycle` with complete validated state sensitivities. See
[the core lifecycle risk contract](../tools/LIFECYCLE-RISK.md) for full inputs,
validation limits and the runnable partial-termination/receipt/recovery example.

The same operation accepts [native single-name CDS default/recovery](../tools/CDS-EVENTS.md).
Unknown recovery returns a `lifecycle_unpriced` audit with no actual risk/P&L mark.
Known recovery retains actual fixed claims and zero pending exposure;
`lifecycle.default_entity` binds fresh-candidate exclusion and complete affected
holding coverage in the existing native solver. Earlier non-default wire shapes
remain unchanged.

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

The existing `cohort_book_pnl` route also supports explicit full fixed-bond boundary amendment events and conserved original security lot splits. Full terms, original cash ownership and applied point facts cross the shared schema/C/MCP/native WASM path. Run `cargo run --locked -p convex --example lot_pnl`; see [the complete contract](COHORT-BOOK-PNL.md). Java and Excel tests are included, but they have not been run on real hosts.


The same `cohort_book_pnl` operation adds optional `claim_transfers`: complete
original earned cash/proposal/entitlement pins, legal parties and fractions, exact
receipt cutoffs, immutable sources and applied points. Outstanding ownership is
conserved across complete legal cohorts, including split successors; prior actual
cash remains with its original recipient. Root claims retain `transfer_ownership`
and recipients retain separate `transferred_claims` with original proposal and
receipt allocation provenance. Full native quantities/models, causal P&L and CTD
switches remain. Run `cargo run --locked -p convex --example claim_pnl`; see
[the complete original cash contract](COHORT-BOOK-PNL.md). Managed-host and browser execution is not yet covered.

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
[the complete custody contract](COHORT-BOOK-PNL.md). Managed-host and browser execution is not yet covered.

`cohort_book_pnl` also accepts optional `dated_entitlements`, binding an original
earned fixed-bond coupon/principal proposal to a separate legal residual fraction
and exact original paid-receipt cutoff. Custody and actual disposal never infer
cash ownership. Existing native residual and earned-transfer owners retain paid
recipients through cash-bond amortization and bullet-delivery redemption.
Amortizing physical invoices retain the original Unsupported convention guard. Run
`cargo run --locked -p convex --example dated_cash_pnl`; see
[the cohort cash contract](COHORT-BOOK-PNL.md#dated-earned-coupon-and-principal-ownership).
Previous native preparation signatures delegate; JSON omission defaults empty,
while Rust cohort input literals add `dated_entitlements: Vec::new()`. Conditional
cash and managed-host/browser execution are not yet covered.

The same route supports separately supplied earned cash while security delivery
remains outstanding. Independent Position sets must contain the available original
proposal and complete legal/receipt prerequisites. Later security ingress,
custody and received-stock disposal preserve cash and invoice recipients. Delayed
earned cash uses its own collateral DF ratio; original pricing DF/spread removes
embedded cash, and actual payments retain the native due-date guard. Run
`cargo run --locked -p convex --example pending_cash_pnl`; see
[pending-delivery cash](COHORT-BOOK-PNL.md#cash-rights-while-physical-delivery-is-pending).
No new operation, type, schema field, C export or production authority is added.

## Physical child earned cash

The existing `cohort_book_pnl` operation accepts explicitly dated fixed-bond cash
on acquired physical custody children. Provide their original earned proposal,
entitlement revision, residual fraction and complete original paid prefix; custody
or disposal never supplies ownership. Earlier paid receipts remain excluded and
later claim transfers preserve each actual recipient. Independent Position sets
require their own complete baseline cash/custody prerequisites and cannot import
later actual Event payments. See [the complete cash contract](COHORT-BOOK-PNL.md)
and `cargo run --locked -p convex --example child_cash_pnl`. Existing fields,
wire version, strict schemas and C exports are unchanged. Managed-host and browser execution is not yet covered.

## Acquired-child principal and final coupon at maturity

The existing `cohort_book_pnl` operation composes original native fixed-bond
maturity proposals with previously acquired physical custody children,
`dated_entitlements`, original paid-prefix exclusions and `claim_transfers`.
Earlier received-stock disposal, pending nominal delivery and original invoice
ownership remain independent of earned cash. Matured security model exposure
is zero; original contractual cash keeps its supplied collateral discount,
actual due-date/overdue and FX guards. Fresh custody/disposal still requires
an active original security at the actual transition.

Run `cargo run --locked -p convex --example child_maturity_pnl`. The complete
signed controls explicitly declare two calendar days of payment delay in all
full security terms. Earlier custody/disposal have complete independently
available baseline sets; later maturity/cash Position experiments are explicitly
`Unavailable` when their actual Time/Event prerequisites do not independently
exist. Existing schemas/routes/wire/C exports are unchanged. The contract and
full causal mixed-book/CTD controls are described in `docs/COHORT-BOOK-PNL.md`.
The Rust, C, MCP and WASM checks run; the Java, .NET and Excel CI and fixtures
have not been run on a real managed host or browser.

## Concurrent custody-child cash and sequential returned shares

Use `cohort_book_pnl` with complete `dated_entitlements` and `claim_transfers`
for simultaneous original principal and final-coupon residual claims. Each
origin/key retains its original proposal, exact legal revision and distinct paid
receipt prefix. Transfer order, current sender share, original residual identity
and every previously paid recipient are checked independently of physical face
or security disposal. A recipient that returns its entire outstanding share can
still own recorded cash. Native maturity ends security model exposure while
original cash/nominal delivery/invoice owners remain separate.

`cargo run --locked -p convex --example concurrent_claims_pnl` executes56 full
requests:28 financial books/2645 product revaluations and28 typed failures. New
controls contain four simultaneous child cash claims and12 sequential/returned
transfers per signed book. Original full maturity/child/pending/mixed/CTD books
and financial checks remain. No new operation/type/schema/wire/C export is added.
The native Decimal cash-partition helper uses a common representable scale and
bounded integer division so allocations conserve the original total exactly at
precision boundaries. Separate pricing/collateral discount, due-date/overdue/FX,
Active-security acquisition/disposal and original fixed-bullet invoice guards
remain. Later independent Position requires its own complete available baseline
cash/custody/legal/paid-prefix facts; actual later Time/Event history cannot be
borrowed. Such maturity/cash experiments are explicitly Unavailable here.

Rust, C, MCP and WASM checks, the strict schema check and a separate C parity
check run every full request. Java and P/Invoke controls and CI are separate;
Java/JNI, Windows, .NET, Excel, wasm32 and browser execution has not been run.
Host agreement with Rust is not an independent check of the financial results.
Not yet covered: broader claims, amortizing invoices and models.

## Full claim settlement and remaining mandatory coverage

The unchanged `cohort_book_pnl` input accepts actual later receipts for explicitly
excluded original child cash after acquired custody. Those amounts stay outside
current legal cash/P&L ownership; the original native obligation may settle and
close. Historical paid cash before acquisition retains the original dated-prefix
guard. Complete original obligations, not selected legal claims alone, determine
`Matured` versus `Closed`. Exact Decimal cash allocation, original lifecycle,
full pricing/model/history/custody/invoice bodies and native exhaustion guards stay.
Run `cargo run --locked -p convex --example settled_claims_pnl`; the full 45-request
consumer retains34 complete financial books and11 typed failures. Direct native
controls also exercise exact exhausted-acquisition/transfer and historical-cash
ownership guards. [The cohort guide](COHORT-BOOK-PNL.md) explains cash meaning.

Run `PYTHONDONTWRITEBYTECODE=1 python3 tools/library/outstanding.py --json` to list
every required record and what remains for it. Use `--area LC-029` to inspect
the advertised clients. The report joins exact operation mappings, product,
convention and model terms, and surfaces; family examples do not fill unmapped
records. Java/C and wasm32 financial checks are recorded separately from strict
WASM envelope drift and from the Windows, Excel and browser hosts that are not
available. [The remaining-work guide](internal/CVX-063-REMAINING.md) gives the
execution order.


Treasury bills have an explicit complete supplied-date `zero`/`treasury_bill`
action for absolute price, annual decimal bank discount or investment rate. The
pure native owner retains issue-based365/366, half-year branching, both inverses,
separate auction rounding and signed analytic rate sensitivities. Original zero
terms/calendar/metadata survive; the existing bill_conversions route stays bounded.
See [Treasury bill quotations](TREASURY-BILL-QUOTES.md).


The Rust-only curve operations have exact method and executable control mappings
in `tools/library/native-operations.json`, for every default, no-default and
schema profile. The six existing curve consumers keep their independent
references, quote units, residuals, rank and work limits, method restrictions
and failure assertions. The facade preserves the 29 construction failures and
9 parametric-fit failures. Rust-only evidence does not replace a JSON or C
operation.


The supplied `directional_hedge` aggregate/solve route accepts explicitly tagged
central or directional numerical records. Directional records retain all six
actual sample parameters/PVs, the selected side, native and signed first/second
precision policies, original canonical point/units/source, reference position and
snapshot. Admission recomputes the original native stencil and arithmetic guards;
record conformance does not qualify the supplied financial source. All columns
for one full factor must agree on computation method, side, original parameter
and directional policies. Original central-only `hedge` requests and Rust entry
points retain their concrete API and wire fields.

`term_coupon_directional_hedge_risk` constructs the complete original coupon
column from native graph and selected-side parameter revaluation. Each intended
parameter needs one exact policy; original work controls reserve five graph and
six directional prices per factor before pricing. Every full native valuation
survives alongside the solver records. The existing constrained optimizer keeps
original current/pending quantities, full factor coverage, FX/legal identity,
liquidity/cost/lot/rank/work limits and final feasibility checks. Proposals carry
no order authority. Run `cargo run --locked -p convex --example directional_hedges`
for independent supplied-record and full five-factor coupon controls.


`yield_scalar` delegates supplied current/simple measures and scalar reporting to
checked native owners. Exact Decimal inputs, annual decimal-rate versus percentage
outputs and original positive face/per100 normalization remain explicit. These
are scalar measures; no dated instrument, market convention or solver result is
manufactured. [The scalar guide](YIELD-SCALARS.md) describes the Result migration.


`cash_yield` accepts strict `price` or `solve` operations with the actual supplied
positive discount years, cash and mathematical compounding. Prices retain input
cash units; inverses retain annual decimal rates and original native residual/
iteration diagnostics. It does not construct an instrument or infer a market
convention. See [supplied-cash yield pricing](YIELD-PRICING.md).


## Checked accrued-interest migration

The four native constant-coupon calculator methods and both yield-engine accrued
traits now return Result. Full coupon context corrects ISDA and ACT/365L accrual;
ex-coupon requests consume an actual supplied calendar and revision. The strict
shared `accrued_interest` route retains original face cash units and delegates
to the same native owner. Unsupported entitlement/calendar context fails.
Use `Act365Fixed` and `Thirty360US`; their ambiguous short aliases are removed.
Read [the accrued-interest contract and migration](ACCRUED-METHODS.md).
Remaining general dated yield/reference and calendar methods, full product/model/
calibration/claim/physical/mixed-book integration and mandatory mappings remain
required. This increment supplies no independent approval or Windows/Excel check.


## Explicit signed-cash yield selection

`cash_yield.solve_selected` and `cash_yield.dated_solve_selected` add explicit
closed-branch inversion. Supply the original cash, target, compounding,
`branch: {lower, upper, rate_tolerance}`, absolute cash-unit `tolerance` and
`max_iterations`. The dated action also requires the complete original
settlement and discount-time context. There is no seed or inferred branch.
Existing `solve` and `dated_solve` continue to require nonnegative cash.

The native `YieldSolver::solve_cash_selected` and `solve_dated_cash_selected`
reuse the same checked cash discount owner. Each absolute cash derivative is
monotonic under the four accepted mathematical discount laws. Endpoint bounds,
signed reversal, conditioning and accumulation allowances must establish one
strict derivative sign over the entire branch. Unresolved cancellation, poles,
subnormal terms, insufficient numerical precision and exhausted work fail.
The result retains the supplied branch, full original-cash residual, derivative
bounds, numerical price/rate allowances and actual discount-evaluation counts.
These are scoped numerical diagnostics, not a formal interval-arithmetic or
market-convention check. All4096cash/10000iteration hard bounds remain.

`no_root` with `details.root_scope = "selected_branch"` concerns only that
branch. A wide branch containing both roots can return `no_convergence` rather
than claim the complete price law has no root. Actual constant dated cash uses
`constant_cash` scope with `non_unique` or `no_root`. Only checked
`PreparedDatedCash` accepts strictly future payments whose actual discount time
is zero; those payments stay in the full target and final repricing.

For cash230at1year and-132at2years at target100 under annual compounding,
branches[.099,.101]and[.199,.201]select10%and20%respectively. Neither branch is
selected by default. The wire change is additive within schema1; existing
requests/envelopes and the original nonnegative inverse behavior are preserved.
Native exhaustive matches on `BondError` must handle the additive
`YieldBranchNoRoot` and `YieldSelectionUnresolved` variants.

Selected-yield numerical bounds are checked again after outward rounding.
Overflow to an infinite diagnostic returns a typed failure. The exact captured
extreme-value regression also runs through the shared request contract.
