# Graph risk and hedge columns

`convex::workflow::curve_hedge_risk` and the shared `graph_hedge_risk` operation
calculate every declared common factor by revaluing the full product on the
curve graph. The result contains the baseline valuation, the position, the
measurement basis, the input manifest and a `HedgeRiskVector` for the
[constrained hedge operations](CONSTRAINED-HEDGES.md).

Pass `graph`, `product` and `input` under `operation`, together with the usual
calendars, curves, fixings and publication inputs. `graph` is a `CalibrationInput`
and `product` is a `RiskProductInput`. Direct curves you pass in and recalibrated
helper graphs keep separate coordinates and response policies. The operation
accepts no cached risk, own-yield substitute or inferred zero column. Each zero
exposure keeps all five priced PVs.

## Reference size and identity

The positive `input.reference_position` has a quantity equal to the reference
quantity. Bond face and CDS notional must match the full contract. Rates futures
use one contract, with the unrounded quote level times currency per point; this
measurement stays `FuturesQuoteLevel`, separate from economic PV.

IRS/OIS use the first fixed-leg notional that you supply. One full trade copy is
bound to that notional and keeps every leg profile, amortization, spread, gearing,
payment and payer/receiver direction. The workflow still prices one `trade_copies`
reference. The same five PVs pass through the normalization code with a reference
scale of exactly one in each representation, and the result exposes both
positions. A hedge quantity of half the reference notional means half of the
*entire trade*, with all its profiles; it does not replace a leg notional or price
a different swap. A zero initial fixed-leg notional has no positive reference here.

`input.identity` supplies the holding or candidate ID, instrument revision,
economics manifest, calculation manifest and entity. Swap and CDS instrument pins
must match the trade ID and version; futures pins must match the dated instrument
and reference revision. `BondTerms` has no security identifier, so the application
supplies and resolves the security pin. The calculation keeps the full economics
but cannot check the contents of external manifests.

Settlement is taken from the bond settlement, swap valuation date, CDS valuation
date or futures economic date. A CDS keeps its legal credit reference. Bonds
discounted on a curve get no invented issuer-credit scope. Each factor keeps its
full curve, helper, node, model, convention, unit and response key, its bump and
half-step policy, its monetary PV and the common snapshot.

## Work, failure and composition

List every factor in the order of the hedge problem. There must be 1..16 distinct
factors and an explicit `max_factor_evaluations` in 1..80. Five calls per factor
are reserved before valuation. This counts five-price factor calls; it is not a
count of every cash, pricing or calibration evaluation underneath, and not a
latency figure. All graph, calibration, schedule, horizon, history, model and
numerical work bounds still apply.

An invalid key or reference, a conflicting instrument revision, a missing source,
an unstable bump, an exhausted reservation or a failed scenario returns no partial
vector. Raw yield does not apply to a curve representation. Optional bonds and
delivery futures need their own model and all-member risk code; this operation
gives no way around those guards.

Calculate all holdings and candidate columns on the same complete graph and
snapshot. Attach same-snapshot reporting FX when required, then pass the
unchanged vectors to `hedge` `aggregate` or `solve`. The hedge code checks factor
coverage and order, identical policy, base PV and reference quantities, credit
scope, current and pending state, signed lots, side costs, eligibility and all hard
constraints. A missing factor is missing input. A proposal has `proposal_only`
authority; application limits and orders are separate work that is not done
yet.

## Controls and example

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

The [request corpus](../tests/fixtures/library/graph-hedge-api.json) contains
independent Decimal60 controls for fixed, stepped and amortizing cash, CDS
premium, default and protection, IRS/OIS, and arithmetic, compounded and term
futures. Positive and negative rates and both swap and CDS economic sides are
covered. Separate files keep the requests that originally failed (hazard spelling,
credit-role spelling, CDS premium start, unadjusted intermediate accrual). The
adjusted CDS date schedule is covered by its own control file with the same bounds.
No request that failed is counted as a passing financial control.

Every column also agrees exactly with the single-factor graph code. The rebuilt
IRS quote case shows that agreement and that recalibration is reachable; it is not
a new independent market-quote check. The example inserts freshly calculated
columns into a mixed bond/CDS/IRS book and solves IRS and futures trades against
independently specified targets, including pending positions, integer contracts,
side costs and USD/EUR reporting FX.

The first mixed-book control, with very broad exposure limits, returned a
`numerical_failure` diagnostic with no proposal. That result is kept as evidence.
A separate control supplies finite exposure limits and explicit factor scales while
keeping the solver tolerance, rank criterion, work bound, trade target, lot and
cost. These controls cover the cases shown; they do not show that the solver is
reliable for every constraint scale.

The shared JSON, C and MCP routes and the WASM build use the same code. The Java
and Excel test sources use freshly generated transport expectations. Execution in
managed runtimes and browsers, and independent market, model and latency
validation, are not covered. This work is partial and has not been certified for production use.

[Strata's parameter-sensitivity contract](https://strata.opengamma.io/apidocs/com/opengamma/strata/market/param/CurrencyParameterSensitivities.html)
describes distinct curve and currency entries and compatible metadata when merging.
[ORE's sensitivity implementation](https://github.com/OpenSourceRisk/Engine/blob/master/OREAnalytics/orea/engine/sensitivityanalysis.cpp)
is a reference for scenario-based factor calculations. Keeping complete,
compatible keys and revaluing with the original product code is a design choice
made here; neither source has been run as a comparison.

The API inventory reads the full-term request corpus as two compact parts, each
under its 8 MiB input bound. Case order, metadata and every string and number
spelling are preserved; only unquoted JSON whitespace is removed. Runtime tests
still read the full corpus.

Related pages:

- [Cash basis hedge risk](CASH-BASIS-HEDGE-RISK.md) covers credit-survival and
  issuer-basis cash through `cash_basis_hedge_risk`: the security, mapping and
  factor contract, the ICMA and rebuild controls, and the mixed-book scope.
- [CDS recovery binding](CDS-RECOVERY-HEDGE-RISK.md) extends the same scenario and
  product code to the five-factor issuer cash/CDS/IRS/futures book.
- [Optional graph hedge risk](OPTIONAL-GRAPH-HEDGE-RISK.md) covers guarded
  optional-model risk and hedge columns.
- [Delivery futures basket risk](DELIVERY-FUTURES-HEDGE-RISK.md) covers all-member
  graph and scenario risk and delivery-futures hedge columns, including exact ties
  and cheapest-to-deliver switches. Run
  `cargo run --locked -p convex --example delivery_hedge`. It keeps the
  fixed-date model and does not value the delivery options.
