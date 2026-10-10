# Graph risk into complete hedge columns

`convex::workflow::curve_hedge_risk` and the shared `graph_hedge_risk` operation
calculate every declared common factor using the original full product and graph
revaluation owners. The result contains the full baseline valuation, original
native position, measurement basis, input manifest and a `HedgeRiskVector` for
the existing [constrained hedge operations](CONSTRAINED-HEDGES.md).

Supply `graph`, `product` and `input` under `operation`, together with the usual
complete calendars, curves, fixings and publication inputs. `graph` uses the
existing `CalibrationInput`; `product` uses `RiskProductInput`. Supplied direct
curves and actual recalibrated helper graphs retain separate coordinates and
response policies. No cached risk, own-yield substitution or inferred zero
column is accepted. Each zero exposure retains all five actually priced PVs.

## Reference size and identity

The positive `input.reference_position` has quantity equal to reference quantity.
Bond face and CDS notional must match the original full contract. Rates futures
use one contract, with the original unrounded quote level times currency per
point; this measurement remains `FuturesQuoteLevel`, separately from economic PV.

IRS/OIS use the explicitly supplied first original fixed-leg notional. One full
trade copy is bound to that notional, retaining every independent leg profile,
amortization, spread, gearing, payment and payer/receiver direction. The native
workflow still prices one `trade_copies` reference. The same five evaluated PVs
pass through the original normalization owner with reference scale exactly one
in each representation. The result exposes both positions. A hedge quantity
of half the reference notional means half of the *entire supplied trade*, with
all its profiles; it does not replace a leg notional or price a different swap.
Zero initial fixed-leg notional has no positive reference under this bridge.

`input.identity` supplies actual holding/candidate ID, instrument revision,
economics manifest, calculation manifest and entity. Swap/CDS instrument pins
must match trade ID and version; futures pins must match the dated instrument
and reference revision. Generic `BondTerms` has no security identifier, so the
application supplies and resolves that explicit security pin. The calculation
retains full native economics but cannot certify external manifest contents.

The result derives settlement from actual bond settlement, swap valuation date,
CDS valuation date or futures economic date. CDS retains its exact legal credit
reference. Curve-discounted bonds have no invented issuer-credit scope. Each
factor retains its full curve/helper/node/model/convention/native-unit/response
key, bump and half-step policy, native monetary PV and common snapshot.

## Work, failure and composition

Supply every intended factor in the exact hedge problem order. There must be
1..16 distinct factors and an explicit `max_factor_evaluations` in 1..80.
Five calls per factor are reserved before valuation. This counts original
five-price factor calls; it is not a count of every underlying cash/pricing/
calibration evaluation or a latency claim. All original graph, calibration,
schedule, horizon, history, model and numerical work bounds still apply.

An invalid key or reference, conflicting instrument revision, missing source,
unstable bump, exhausted reservation or failed native scenario returns no
accepted partial vector. Raw yield is not applicable to a curve representation.
Optional bonds and delivery futures require their original model/all-member
risk owners; this operation provides no shortcut through those guards.

Calculate all holdings and candidate columns on the same complete graph and
snapshot. Attach explicit same-snapshot reporting FX when required, then supply
the unchanged vectors to `hedge` `aggregate` or `solve`. The hedge owner checks
complete factor coverage/order, identical policy/base PV/reference quantities,
credit scope, current and pending state, signed native lots, side costs,
eligibility and all hard constraints. A missing factor is missing input. A
proposal has `proposal_only` authority; application limits/orders remain
CVX-025/031 work.

## Controls and consumer

```sh
python3 tests/scripts/graph_hedge_reference.py
python3 tests/scripts/graph_hedge_problem_reference.py
python3 tests/scripts/graph_hedge_admission.py
python3 tests/scripts/graph_hedge_roles.py
python3 tests/scripts/graph_hedge_terms.py
python3 tests/scripts/graph_hedge_cash_reference.py
python3 tests/scripts/graph_hedge_protocol.py
python3 tests/scripts/graph_hedge_limits.py
python3 tests/scripts/graph_hedge_census.py
cargo run --locked -p convex --example graph_hedge
cargo run --locked -p convex --example graph_hedge --no-default-features
cargo run --locked -p convex --example graph_hedge --features schemars
cargo test --locked --all-features -p convex -p convex-ffi -p convex-mcp -p convex-wasm --test graph_hedge -- --test-threads=1
```

The [complete request corpus](../tests/fixtures/library/graph-hedge-api.json)
contains independent Decimal60 controls for full fixed/stepped/amortizing cash,
CDS premium/default/protection, IRS/OIS and arithmetic/compounded/term futures.
Positive and negative rates and both swap/CDS economic sides are explicit.
Raw failed hazard spelling, credit-role spelling, CDS premium start and
unadjusted intermediate-accrual controls remain separate immutable artifacts.
Actual adjusted CDS chronology is independently declared in a separate control
file with unchanged bounds and direct integration primitives. No failed raw
control is promoted to financial qualification.

Every column also agrees exactly with the unchanged single-factor full graph
owner. The rebuilt IRS quote case establishes that owner agreement and actual
recalibration reachability, without a new independent market-quote certificate.
The consumer inserts freshly calculated columns into a mixed bond/CDS/IRS book
and solves IRS/futures trades against independently specified targets, including
pending positions, integer contracts, side costs and USD/EUR reporting FX.

The initial mixed-book control with very broad exposure limits returned a
`numerical_failure` diagnostic with no proposal. That raw result remains in the
engineering evidence. A separately declared control supplies finite application
exposure limits and explicit factor scales while retaining every original solver
tolerance, rank criterion, work bound, trade target, lot and cost. These controls
qualify the stated supplied cases; they do not establish solver robustness for
all constraint scales.

Shared JSON/C/MCP and native-compiled WASM use the same route. Java and Excel
test sources use freshly generated native transport expectations. Actual
managed/browser qualification and market/model/library/production/latency
certificates remain separate. CVX-063 remains partial and uncertified.

[Strata's parameter-sensitivity contract](https://strata.opengamma.io/apidocs/com/opengamma/strata/market/param/CurrencyParameterSensitivities.html)
describes distinct curve/currency entries and compatible metadata when merging.
[ORE's sensitivity implementation](https://github.com/OpenSourceRisk/Engine/blob/master/OREAnalytics/orea/engine/sensitivityanalysis.cpp)
provides a primary reference for scenario-based factor calculations. Keeping
full explicit compatible keys and original revaluation owners is the design
inference used here; neither source is an executed comparator for this increment.

The full-term raw request corpus remains intact. The API inventory reads two
lossless compact parts, each under its unchanged 8 MiB input bound. Case order,
metadata and every original string/number lexeme are preserved; only unquoted
JSON whitespace is removed. Runtime tests still read the original full corpus.
