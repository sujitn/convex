# Bond quotes

Bond quoting for fixed-coupon bullet bonds and zero-coupon bonds is done by
pure library functions: `convex::bond_quote::quote_fixed` and `quote_zero`. The
engine passes in validated terms, explicit curve roles and a completion
timestamp. The REST and C ABI adapters only translate inputs and outputs. REST
responses carry the dirty-price serialization, and quotes are not broadcast on
the WebSocket.

The analytics path has no provider, database, queue, clock or async runtime.
The unused pricing-model selector and the engine's own copies of bond
pricing and risk code have been removed.

Status: this has been compared against QuantLib and independent calculations,
but not against a vendor system. Not yet covered: other market convention
packs, every instrument family, host integration, execution eligibility and
latency. Production startup still needs provider and deployment wiring.

## Rust route and units

`convex::bond_quote::quote_fixed` takes:

- a fixed-coupon instrument (validated on construction);
- the settlement date;
- a trader mark;
- the output yield frequency;
- `BondCurves`;
- the central risk half-width in basis points;
- an ordered key ladder.

It returns prices, analytical yield risk, the supported spreads, diagnostic
curve risk, and a structured error for each measure that failed. A missing
optional input leaves its measures absent. A missing input that is needed to
interpret the mark fails the whole calculation.

`price_from_mark` and the yield functions use `Bond::yield_cash_flows`. Fixed
bonds override that route with the actual payment dates and the regular ICMA
reference periods. Discount time accumulates full-period fractions less the
fraction already elapsed. This keeps 30/360 boundaries (which are not additive)
and adjusted payment timing correct. Coupon cash and accrued interest use the
same core periods.

- Cash on the settlement date is excluded.
- Ex-coupon affects coupon entitlement and the accrued rebate. Final principal
  is still payable.
- Future cash includes zero-coupon entries where needed to keep the discount
  time grid intact.

`Bond::try_cash_flows` and `try_accrued_interest` return the validation errors
of fixed bonds. The older infallible conveniences need a valid instrument.
Custom trait implementations and calendar callbacks must terminate and meet
their documented preconditions. Fixed-bond construction and deserialization
reject invalid dates, calendars, day counts and economics. Amounts returned as
cash flows are contractual cash. Quote prices and accrued interest are
normalized by the actual face.

| Measure | Unit and interpretation |
|---|---|
| Clean, dirty, accrued | Price per 100 face; dirty = clean + accrued |
| YTM | Annualized decimal in the requested periodic compounding |
| Macaulay/modified duration | Years |
| Convexity | Years squared |
| `dv01` / `dv01_loss` | Historical positive quoted-yield loss per 100 for +1bp |
| Z-spread | Continuous ACT/365F spread, basis points |
| I-spread | Bond yield less the declared swap par benchmark, aligned compounding; rounded to basis points |
| G-spread | Bond yield less the yields of the government securities you pass in; rounded to basis points |
| Par-par ASW | The existing par-par calculator, using the swap curve you pass in, in rounded basis points; full floating-leg and stub conventions are not yet validated |
| `zero_pv01_signed` | Signed price per 100 for +1bp continuous-zero shift |
| `z_pv01_signed` | Signed price per 100 for +1bp continuous ACT/365F Z shift |
| `pv01`, `cs01` | Compatibility positive losses: minus the corresponding signed zero/Z sensitivity; `cs01` is **not** a CDS hazard or calibrated credit-quote sensitivity |
| Key-rate duration | Partial duration under triangular continuous-zero shifts at the curve reference date |

Curve risk holds the solved Z-spread fixed. The central difference divides by
the actual half-width and then normalizes to one basis point. Shifts are
applied exactly to the discount function, so interpolation and settlement
normalization are preserved and nodes are not resampled. The first and last key
weights extend to the endpoints.

Partial buckets can be negative when settlement normalization offsets a shifted
earlier pillar. Within the finite-difference error, they add up to the parallel
zero risk. These measures do not rebuild a calibrated curve from market quotes.

The engine keeps its explicit 10bp half-width. Rust callers choose the width,
and C ABI key-rate duration uses 1bp. Comparisons must use the same width and
key ladder. Quoted-yield DV01 is analytical and does not depend on the width.
Curve PV01 and CS01 are absent, with an error, when their curve calculation
fails. Yield DV01 is never substituted for them. The standalone example uses
1bp:

```sh
cargo run --locked -p convex --example bond_quotes
cargo run --locked -p convex --example bond_quotes --no-default-features
cargo run --locked -p convex --example bond_quotes --features schemars
```

`quote_zero` divides the actual redemption by face. It uses an explicit,
context-free year fraction: ACT/360, ACT/365F, ACT/ACT ISDA, 30/360 US or
30E/360. Yield marks honor their frequency. Derived yield and risk use the zero
instrument's declared compounding (annual, semiannual, quarterly, monthly or
continuous). Continuous duration is in time units and convexity in time
squared.

These fail:

- expired inputs;
- nonpositive prices;
- invalid rates;
- missing ICMA quasi-coupon references;
- unsupported spread or key-rate conventions.

The API does not implement every bill's discount-rate quote or money-market
convention yet.

## Input roles and failure behavior

| Input | Used for | If absent or invalid |
|---|---|---|
| Discount curve | Z price/spread and diagnostic curve risk | Required marks fail; optional measures carry errors |
| Swap benchmark curve | I-spread and par-par ASW | No substitution from the discount curve |
| Government securities and yields | G-spread and G-spread mark price | No inferred Treasury securities or zero-rate proxy |
| Fixing history and projection curve | Floating coupons/DM | The service and C ABI refuse until a fixing-aware request exists |
| Option model and volatility | Callable/putable model price/OAS | The service refuses embedded optional terms and volatility inputs |

The government adapter accepts 1 to 128 securities. Each needs a
sovereign/currency, maturity, identifier and quoted yield. Duplicate tenor or
maturity keys and ineligible dates fail. Interpolation uses the actual
remaining ACT/365F maturities. Yields are converted to semiannual compounding,
then interpolated linearly with flat endpoints. This is not a government curve
calibration or model.

The quote route supports distinct I-spread and G-spread mark inputs. The older
generic `price_from_mark` G-par-curve interpretation and the older YAS and
model helpers remain as compatibility diagnostics. They are not the
government-securities quote contract. The v1 C ABI pricing and risk request
cannot carry government securities, so its G-spread price mark fails. The
spread operation does accept real benchmarks.

Limits:

- 4096 cash-flow periods and 150-year horizons;
- 64 strictly ordered, positive key tenors below 150 years;
- a finite half-width in (0,100] bp;
- fixed-bond face positive and at most 1e18;
- coupon in [0,10];
- redemption positive and at most ten times face;
- date offsets bounded by the shared primitives;
- solver defaults of 100 iterations and 1e-10 tolerance.

`ZSpreadCalculator::spread_dv01` now returns a checked
`AnalyticsResult<Decimal>`, so Rust callers must propagate or handle its error.
Zero and curve errors are explicit. A failed financial conversion never turns
into par, a zero price, a zero spread sensitivity or an invented curve.

## REST and C ABI contracts

`POST /api/v1/quote` keeps `bond`, `settlement_date` and `market_price`.
Reference terms and exact issuer, calendar and convention versions must already
resolve.

Without curves in the request, it prices under an activated configuration: the
one named by `configuration`, or the one the runtime runs. That configuration's
curve roles are read at `known_at`. The response's `context` names the
configuration, activation and curve pins (see the server README). Curves in the
request are an exclusive what-if context.

`GET /api/v1/bonds/:id/quote` uses the same engine route. Both quote responses
expose checked derived dirty prices. These on-demand routes need an observed
price in the request. They do not assume par or generate model prices.

The optional `discount_curve` and `swap_curve` each contain a required
`currency` and a full `SnapshotCurve`. That includes:

- the exact curve ID and version, and the reference date;
- source and recording times, origin/model, input revisions, quality and
  maximum age;
- interpolation and extrapolation policies;
- finite continuous-zero ACT/365F pillars.

Role currencies must match the bond. These fail:

- future reference dates;
- stale or invalid quality, and staleness at publication;
- bad versions or pillars;
- conflicting contents under the same ID and version.

When curves are supplied, a response's `curve_versions` lists the discount and
swap mappings used. `government_curve` is a `GovernmentCurveSpec` holding the
government securities you pass in, not generic zero nodes. JSON rejections and
invalid financial inputs return 4xx errors. Missing optional measures are
listed in `measure_errors` with side-qualified field names.

Requests with curves are indicative calculations. They do not load a complete
immutable historical `ValuationContext`, check market freshness against a
valuation cutoff, or grant execution authority. REST quote objects carry the
common clean/dirty/accrued fields plus the measure errors and signed
sensitivities. Consumers who need full provenance or a stream must use the
context-valued runtime publications. Common v2 transport parity is not complete
yet.

In the fixed C ABI, `BondSpec` now reads the business-day convention and the
optional complete `terms`: dated, first and penultimate dates, calendar, EOM,
settlement days, redemption and the core ex-coupon rule. Without `terms`, the
old constructor defaults apply, so pass them explicitly to get the complete
term semantics. Zero specs add an optional `redemption_value`, defaulting to
face redemption when omitted.

The v1 zero ICMA default is still serialized, but checked pricing rejects it
without references. Select an explicit supported day count.

FRN price, risk, DM and cash-flow calls return a `fixings` input failure,
because v1 cannot represent fixing history. This also removes the old
discount-to-projection default and the current-index shortcut from active
dispatch.

`SpreadParams.quote_frequency` defaults to the unchanged v1 semiannual value.
`params.government` takes the same benchmark record as REST. The old
`params.govt_curve` zero-curve proxy has been removed and fails as an unknown
input. See [caller migration](internal/ROUTE-MIGRATION.md). No C exports,
pointer ownership rules, allocation limits or ABI symbols change. The
introspection schemas describe these additions. The generated host schemas are
not yet fully in parity.

## Convention coverage

| Case | Evidence | Not yet covered |
|---|---|---|
| Fixed bullet, coupon-date/negative yield | Golden values plus a retained QuantLib replay | A specific market pack and independent approval |
| Non-100 face and redemption premium | Rust and C ABI goldens, REST checks | All host construction and invoice workflows |
| Leap, ICMA EOM, 30/360, ACT/360, ACT/365F | Dated cash and analytic expectations | Every advertised day-count/basis combination |
| Ex-coupon record boundary/principal rebate | Rust and C ABI goldens for explicit core rules | REST v1 only has the calendar-days/OnDate ex rule; complete contract migration |
| Short front, long front, short back | Rust and C ABI goldens using real references | REST accepts only none/short-first; long front and back stubs are rejected |
| Monthly anchor without day drift | Rust and C ABI golden | The reference check's forward month walk still needs migration |
| Weekend and SIFMA Independence Day payment | Rust, C ABI and REST goldens | Full dated calendar rule packs |
| Curves, currencies and failures | REST and Rust role comparisons, C ABI spread and key-rate tests | Quote rebuild, complete curve families, model validation |
| Zero ACT/360 and continuous basis | Analytic identities; explicit REST and C ABI zero routes | ICMA references, bill discount conventions, zero curve risk |
| Callable/putable/FRN/amortizing/stepped | Explicit refusal by the service and missing-fixing controls | Complete models, contracts and profile integration |

The [13-case fixture](../crates/convex-analytics/tests/fixtures/bond-quotes.json)
declares dates, amounts, discount times and per-100 units. A Python Decimal
script at 50-digit precision recomputes PV, accrued interest and analytic
duration, convexity and DV01 without any Convex output. Rust tests check real
schedules against those times and cash values, and real C ABI requests replay
every case. REST replays its supported subset plus explicit zero, curve and
error cases.

Tolerances are absolute 1e-9 for price and risk, 1e-10 for accrued interest and
yield, plus relative 1e-11. They have not been independently reviewed.

The retained QuantLib 1.40 comparison passes 392 of 392 measures after the
payment-time correction. The earlier reference values, tolerances and 69
earlier quantitative artifacts are unchanged. The historical 287 passes and 105
misses stay in their original evidence. The retained external capture is still
marked `retained_unqualified` with `certified: false`, and the numerical
agreement does not change that. The new fixtures add coverage and do not replace
the old comparisons.

The implementation was reviewed against QuantLib 1.40's
[cash-flow discount steps](https://raw.githubusercontent.com/lballabio/QuantLib/v1.40/ql/cashflows/cashflows.cpp)
and [fixed coupon accrual](https://raw.githubusercontent.com/lballabio/QuantLib/v1.40/ql/cashflows/fixedratecoupon.cpp).
It reuses Convex's core periods, and no comparator output or external financial
code is used in pricing.

```sh
cargo test --locked -p convex-bonds -p convex-analytics -p convex-server -p convex-ffi
cargo test --locked -p convex-engine
cargo test --locked -p convex-server --test api_integration_tests cvx017_
```

Not yet covered elsewhere: complete bond-family contracts and dispatch,
independently reviewed product and market-pack validation, rates/CDS/futures,
calibrated quote risk, live provider wiring, measured latency and multi-host
deployment.

Full G-spread-to-price using real government securities is available through
the `government_spread_price` shared operation. It keeps the dated target cash,
independent reference and spread-quote revisions, every benchmark inverse, the
signed consideration and separate price and spread roundtrip bounds. See
[the full inverse contract](GOVERNMENT-SPREAD-INVERSE.md). The removed
convenience proxies stay removed.
