# Shared supplied bond quotes

CVX-017 routes nominal fixed-bullet and zero pricing through pure library
functions. The engine supplies validated terms, explicit curve roles and a
completion timestamp. REST and C ABI adapters translate inputs and outputs. REST and WebSocket
share dirty-price serialization so quote resynchronization preserves the
same economic payload.
There is no provider, database, queue, clock or asynchronous runtime in the
analytics path. The unused pricing-model selector and engine copies of bond
pricing/risk mathematics are removed.

This is engineering implementation and comparison evidence. It does **not**
certify a market convention pack, every instrument family, host integration,
execution eligibility, latency, or LB0/LB1. Production startup still requires
the outstanding provider/deployment composition and release gates.

## Native route and economic units

`convex::bond_quote::quote_fixed` accepts a checked fixed-coupon instrument,
settlement, a trader mark, output yield frequency, `BondCurves`, central risk
half-width in basis points, and an ordered key ladder. It returns prices,
analytical yield risk, supported spreads, diagnostic curve risk, and individual
structured measure errors. Missing optional inputs leave their measures absent.
A missing input needed to interpret the mark fails the entire calculation.

`price_from_mark` and yield functions use `Bond::yield_cash_flows`. Checked fixed
bonds override that route with actual payment dates and retained regular ICMA
reference periods. Discount time accumulates full-period fractions less the
already elapsed fraction; this preserves non-additive 30/360 boundaries and
adjusted payment timing. Coupon cash and accrued use the same core periods.
Settlement-date cash is excluded. Ex-coupon affects coupon entitlement and
accrued rebate, while final principal remains payable. Future cash includes
zero coupon entries where needed to preserve the discount-time grid.

`Bond::try_cash_flows` and `try_accrued_interest` propagate checked fixed-bond
failures. The older infallible conveniences require a valid instrument; custom
trait implementations and calendar callbacks must terminate and satisfy their
declared preconditions. Checked fixed construction and native deserialization
reject invalid dates, calendars, day counts and economics. Amounts returned as
cash flows are contractual cash; quote prices/accrued normalize by actual face.

| Measure | Unit and interpretation |
|---|---|
| Clean, dirty, accrued | Price per 100 face; dirty = clean + accrued |
| YTM | Annualized decimal in the requested periodic compounding |
| Macaulay/modified duration | Years |
| Convexity | Years squared |
| `dv01` / `dv01_loss` | Historical positive quoted-yield loss per 100 for +1bp |
| Z-spread | Continuous ACT/365F spread, basis points |
| I-spread | Bond yield less the declared native swap par benchmark, aligned compounding; rounded native basis points |
| G-spread | Bond yield less actual supplied government security yields; rounded native basis points |
| Par-par ASW | Existing declared par-par calculator, supplied swap curve and native rounded basis points; full floating-leg/stub conventions remain unqualified |
| `zero_pv01_signed` | Signed price per 100 for +1bp continuous-zero shift |
| `z_pv01_signed` | Signed price per 100 for +1bp continuous ACT/365F Z shift |
| `pv01`, `cs01` | Compatibility positive losses: minus the corresponding signed zero/Z sensitivity; `cs01` is **not** a CDS hazard or calibrated credit-quote sensitivity |
| Key-rate duration | Partial duration under triangular continuous-zero shifts at the original curve reference date |

Curve risk holds the solved Z-spread fixed. The central numerator divides by
the **actual** half-width, then normalizes to one basis point. Exact discount
function shifts preserve interpolation and settlement normalization; nodes are
not resampled. First/last key weights extend to the endpoints. Partial buckets
can be negative when settlement normalization offsets a shifted earlier pillar.
They partition parallel zero risk within the tested finite-difference error.
These measures do not rebuild a calibrated curve from market quotes.

The engine retains its explicit 10bp half-width. Native callers select the
width; C ABI KRD uses 1bp. Equivalent comparisons must use the same width and
key ladder. Analytical quoted-yield DV01 is independent of this width. Curve
PV01/CS01 are absent when their curve calculation fails, with an error; yield
DV01 is never substituted. The standalone example uses 1bp:

```sh
cargo run --locked -p convex --example bond_quotes
cargo run --locked -p convex --example bond_quotes --no-default-features
cargo run --locked -p convex --example bond_quotes --features schemars
```

`quote_zero` normalizes actual redemption by face and uses an explicit
context-free ACT/360, ACT/365F, ACT/ACT ISDA, 30/360 US or 30E/360 year fraction.
Yield marks honor their frequency; derived yield/risk use the zero instrument's
declared annual/semiannual/quarterly/monthly/continuous compounding. Continuous
duration is time and convexity is time squared. Expired inputs, nonpositive
prices, invalid rates, absent ICMA quasi-coupon references and unsupported
spread/key-rate conventions fail. This API does not implement every bill's
discount-rate quote or money-market convention; those are pending coverage.

## Exact roles and failure behavior

| Input | Consumed by | If absent or invalid |
|---|---|---|
| Discount curve | Z price/spread and diagnostic curve risk | Required marks fail; optional measures carry errors |
| Swap benchmark curve | I-spread and par-par ASW | No substitution from the discount curve |
| Actual government securities and yields | G-spread and native G-spread mark price | No inferred Treasury securities or zero-rate proxy |
| Eligible fixing history and projection curve | Floating coupons/DM | Current service/C ABI route refuses until the complete fixing-aware request exists |
| Explicit option model/volatility | Callable/putable model price/OAS | Current nominal service refuses embedded optional terms and nominal volatility inputs |

The government adapter accepts 1..128 actual securities with explicit
sovereign/currency, maturity, identifier and quoted yield. Duplicate tenor or
maturity keys and ineligible dates fail. Checked interpolation uses actual
remaining ACT/365F maturities, converts yields to semiannual compounding, and
uses linear interpolation with explicit flat endpoints. It does not replace
the outstanding government curve calibration/model certification work.

The checked quote route supports distinct native I/G mark inputs. The generic
older `price_from_mark` G-par-curve interpretation and older YAS/model helpers
remain compatibility diagnostics; they are not the supplied-government quote
contract. The v1 C ABI pricing/risk request cannot supply government securities,
so its G price mark fails; the spread operation accepts actual benchmarks.

Bounds include 4096 cash-flow periods, 150-year horizons, 64 strictly ordered
positive key tenors below 150 years, and a finite half-width in (0,100] bp.
Fixed-bond face is positive and at most 1e18, coupon is in [0,10], redemption
is positive and at most ten times face, and date offsets are bounded by the
shared primitives. Solver defaults remain 100 iterations and 1e-10 tolerance.
`ZSpreadCalculator::spread_dv01` now returns a checked `AnalyticsResult<Decimal>`;
Rust callers must propagate or handle its error. Zero/curve errors are explicit; failed financial conversions do not become par,
zero price, zero spread sensitivity or a fabricated curve.

## REST and C ABI contracts

`POST /api/v1/quote` keeps `bond`, `settlement_date`, `market_price`. Reference
terms and exact issuer/calendar/convention versions must already resolve.
`GET /api/v1/bonds/:id/quote` uses the same engine route. Both quote responses
expose checked derived dirty prices. These on-demand routes require a supplied
observed price; they do not assume par or generate model prices.

Optional `discount_curve` and `swap_curve` each contain required `currency` and
the full existing `SnapshotCurve`: exact curve ID/version, reference date,
source/recording times, origin/model, input revisions, quality, maximum age,
interpolation/extrapolation policies and original finite continuous-zero
ACT/365F pillars. Role currencies must match the bond. Reconstruction retains
the actual reference/pillars/policies. Future references, stale/invalid quality,
staleness at publication, bad versions/pillars and conflicting contents under
the same ID/version fail. When curves are supplied, a response's `curve_versions` lists the actual
discount/swap mappings. `government_curve` is a `GovernmentCurveSpec` containing
actual supplied securities, not generic zero nodes. JSON rejections and invalid
financial inputs return explicit 4xx errors; missing optional measures are
listed in `measure_errors` with side-qualified field names.

These supplied on-demand requests are indicative calculations. They do not
acquire a complete immutable historical `ValuationContext`, check current
market freshness against a valuation cutoff, or grant execution authority.
Snapshot/replay runtime, access control and release admission retain their
separate context, quality and ownership rules. WebSocket quote objects retain
the common clean/dirty/accrued fields and the new measure errors/signed
sensitivities. The v1 quote broadcast does not carry supplied REST curve
revision metadata; consumers requiring full provenance must use canonical
context-valued runtime publications. Complete common v2 transport parity
remains CVX-033/063.

Fixed C ABI `BondSpec` now consumes the supplied business-day convention and
optional complete `terms`: dated/first/penultimate dates, calendar, EOM,
settlement days, redemption and core ex-coupon rule. Absent terms retain the
old constructor defaults; explicitly supply them for complete term semantics.
Zero specs add optional actual `redemption_value`, retaining face redemption
when omitted. The v1 zero ICMA default remains serialized but is not admitted
by checked pricing without references; select an explicit supported day count.
FRN price/risk/DM/cash-flow calls return a `fixings` input failure because v1
cannot represent eligible fixing history. This also removes the old
discount-to-projection default and current-index shortcut from active dispatch.

`SpreadParams.quote_frequency` defaults to the unchanged v1 semiannual value.
`params.government` supplies the same actual benchmark record as REST. The old
`params.govt_curve` zero-curve proxy has been removed and fails as unknown input.
See [caller migration](ROUTE-MIGRATION.md). No C exports,
pointer ownership rules, allocation/admission ceilings or ABI symbols change.
The existing introspection schemas describe these additions; full generated
host schema parity remains CVX-063.

## Convention and evidence matrix

| Dimension | Actual evidence | Remaining qualification |
|---|---|---|
| Fixed bullet, coupon-date/negative yield | Supplied golden plus retained QuantLib replay | Exact market pack and independent owner approval |
| Non-100 face and redemption premium | Native/C ABI goldens and REST checks | All host construction/invoice workflows |
| Leap, ICMA EOM, 30/360, ACT/360, ACT/365F | Supplied dated cash and analytic expectations | Every advertised day-count/basis combination |
| Ex-coupon record boundary/principal rebate | Native/C ABI goldens for explicit core rules | REST v1 only has calendar-days/OnDate ex rule; complete contract migration |
| Short front, long front, short back | Native/C ABI goldens retain real references | REST admission remains none/short-first; long/back stubs explicitly rejected |
| Monthly anchor without day drift | Native/C ABI golden | Current reference admission's forward month walk still needs migration |
| Weekend and SIFMA Independence Day payment | Native/C ABI and admitted REST golden | Full dated calendar rule/admission packs |
| Exact supplied curves, currencies and failures | REST/native role comparisons and C ABI spread/KRD tests | Quote rebuild, complete curve families and model certificates |
| Zero ACT/360 and continuous basis | Analytic native identities; explicit REST/C ABI zero routes | ICMA references, bill discount conventions, zero curve risk |
| Callable/putable/FRN/amortizing/stepped | Explicit nominal-service refusal and missing-fixing controls | Complete models/contracts/profile integration in CVX-055 and owning curve tasks |

The [13-case fixture](../crates/convex-analytics/tests/fixtures/bond-quotes.json)
declares dates, amounts, discount times and per-100 units. Python Decimal at
50-digit precision recomputes PV, accrued and analytic duration/convexity/DV01
without Convex output. Native tests verify real schedules against those supplied
times/cash values, and real C ABI requests replay every case. REST replays its
admitted subset plus explicit zero/curve/error controls. The new bounds are
absolute 1e-9 for price/risk, 1e-10 for accrued/yield, plus relative 1e-11;
these are engineering tolerances awaiting independent quantitative approval.

The fresh retained QuantLib 1.40 comparison passes **392/392 measures** after
the payment-time correction. Original reference values, tolerances and all 69
earlier quantitative artifacts remain unchanged. CVX-040/060's historical
287 passes/105 misses stay in their original evidence. The retained external
capture remains `retained_unqualified`, `certified: false`; numerical agreement
does not retroactively authenticate that capture or award a library certificate.
New fixtures expand coverage rather than replace the old comparisons.

Primary implementation review used QuantLib 1.40's
[cash-flow discount steps](https://raw.githubusercontent.com/lballabio/QuantLib/v1.40/ql/cashflows/cashflows.cpp)
and [fixed coupon accrual](https://raw.githubusercontent.com/lballabio/QuantLib/v1.40/ql/cashflows/fixedratecoupon.cpp).
The implementation reuses Convex's core periods; no comparator output or
external financial code is substituted into pricing.

```sh
cargo test --locked -p convex-bonds -p convex-analytics -p convex-server -p convex-ffi
cargo test --locked -p convex-engine
cargo test --locked -p convex-server --test api_integration_tests cvx017_
```

Continue with CVX-055 for complete bond family contracts/dispatch. Independently
reviewed product, market-pack and standalone certificates remain CVX-061..065;
rates/CDS/futures, calibrated quote risk, live provider composition, measured
latency and multi-host deployment retain their owning tasks in the inventory.
