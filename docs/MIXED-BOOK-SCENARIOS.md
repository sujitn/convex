# Mixed-book scenarios and P&L attribution

Convex can fully revalue a mixed book of positions on one immutable set of
inputs. The code is in `convex::workflow` and the shared `library` operation.
It combines the cash, ICMA issuer-basis, CDS, IRS/OIS, rate-futures, HW1F
optional-bond and delivery-basket pricers. It returns hypothetical model
marking changes with ordered factor attribution, standalone effects, nonlinear
interactions and reconciliation residuals.

Start with the example:

```sh
cargo run --locked -p convex --example mixed_scenario
cargo test --locked --all-features -p convex -p convex-ffi -p convex-mcp -p convex-wasm --test mixed_scenario -- --test-threads=1
cargo test --locked --all-features -p convex --test mixed_scenario_boundaries -- --test-threads=1
```

The example checks eight independently declared financial books and forty typed
failures, then prints a compact result. `--emit-parity` emits full Rust
envelopes for transport comparison. `--features schemars -- --emit-schema`
emits the strict request schema. The request files are
`tests/fixtures/library/mixed-scenario-api-*.json`.

The independent financial answers, the failed inputs and the separate
acceptance checks are in the reference, admitted, publications and boundaries
fixtures, and scripts regenerate them read-only. Source corrections keep the
original financial answers and bounds. Rust output is transport evidence and is
never used as a reference price.

## Calculation boundary

Direct embedding uses
`convex::workflow::{prepare_book_scenarios, revalue_book_position, aggregate_book_scenarios}`:

1. Prepare a full `PreparedQuoteRiskGraph`.
2. Declare a `BookScenarioInput`.
3. Revalue every expected position through a `BookScenarioProduct`.
4. Aggregate.

Results are constructed privately and tied to that prepared path. Foreign,
missing, duplicated or reordered results fail. A PV callback you supply or a
cached Greek vector cannot stand in for a full product revaluation. Read-only
getters expose the full valuations and attribution to Rust callers.

The shared request uses `operation.kind = "calibration"` with the existing full
calibration `input`, and `action.kind = "book_scenario"` with a complete
`MixedBookScenarioInput`. Its `book.positions` owns identities and quantities.
`holdings` binds each expected ID to a full `RiskProductInput`, or to a
`CashSecurityInput` and issuer-basis model. `convex::library::execute` and
`execute_json`, `convex_library`, and the MCP and WASM `library` adapters all
dispatch to the same code. No C export is added.

The pure boundary leaves out databases, message queues, provider connectors,
clocks, distributed scheduling, ledger transactions, position reservations and
order authority.

The caller supplies source identity, economics and calculation manifests, the
legal entity, curve keys, quote versions, calendars, fixing history, the
known-at policy, cash state, models and numerical settings. The snapshot ID is
declared by the caller. The function does not authenticate a snapshot or
retrieve, relabel or time-shift its data. A server must bind these values to
its immutable snapshot before calling the library.

## Graph and nonlinear path

Pass 1 to 16 distinct, compatible `QuoteGraphMove` coordinates, in the order
you want them applied. Each `delta` is an annual-rate, fraction or per100-price
move. Its key keeps the quotation units and response policy. Helper quote
conversion stays in the graph code.

Raw product rate inputs use fractional decimal units. Yield, Z/DM and OAS
inputs must declare `DecimalRate`. HW1F parameter moves are model assumptions,
not volatility market quotes. Their opaque model identity and version must
match, and the numeric raw observation version is a separate declared pin.

For every prefix and standalone scenario, the quote, recovery and node moves
that rebuild are applied together before one dependency build. Frozen curve
overlays then apply to the rebuilt result. The solver, helper repricing, rank,
history and publication checks all still apply.

These fail before revaluation:

- conflicting quote and output-grid moves;
- duplicate or aliased raw coordinates;
- overlapping parallel and node moves, or joint and individual moves.

A declared raw coordinate must be used by a complete product somewhere in the
book. A missing exposure cannot become an invented zero.

The report has exactly `1 + 2F` points: the baseline, `F` ordered prefixes, and
`F` standalone moves. Every expected position is fully priced at every point,
including zero quantities and zero exposures. Each graph keeps the curves, the
calibration diagnostics, the quote residuals, the build order, the publication
requirements and the helper work. The final prefix is the full combined
scenario. Reversing the order keeps the final economics and changes the ordered
attribution.

Optional bonds keep the full notice and exercise terms, the cash, profile and
reset terms, known fixings, HW1F provenance, dated grids and price-convergence
checks. The supported graph responses are:

- frozen parallel discount, projection and joint shifts;
- security OAS;
- named HW1F parameters.

The baseline curves and each scenario curve are bound, including identical
recipes with different fitted values. Direct or indirect shaped or rebuilt
changes would need more model integration and fail explicitly. Parameter
scenarios rebuild the trees. The calibrated-source provenance is kept, but the
scenario does not refit swaption quotes.

## Quantities, money and attribution

Each position supplies a signed current quantity, a signed hypothetical pending
quantity, the unit, and a positive reference quantity.

- Bonds and CDS use the exact face or notional.
- Swaps use full trade copies, or the first fixed-leg notional as a scaling
  reference for the stepped trade.
- Futures use integer contracts and a one-contract reference.
- Optional bonds use face or full trade copies.

Security revisions must match where the product owns that identity. Plain bond
terms rely on the caller's security and economics manifest.

Current, pending and projected values stay separate. The projected quantity is
the exact Decimal sum of current and pending, scaled once. Pending is
hypothetical, and the result does not assert an execution, reservation or fill.

Reporting conversion needs one positive direct FX observation per foreign
currency, with its observation pin and the same declared snapshot. Missing,
duplicate, extra or wrong-snapshot FX fails. No inverse, triangulation or
scenario FX shift is inferred.

For each quantity view, position and book total:

- Total change is the full scenario value minus baseline.
- Ordered effect `i` is prefix value `i` minus prefix value `i-1`.
- Standalone effect `i` is its standalone scenario value minus baseline.
- Interaction is total change minus the sum of standalone effects.
- Reconciliation residual is total change minus the sum of ordered effects.

The residual is calculated and kept, never forced to zero. Ordered effects
depend on the path. Interaction contains real nonlinear cross-effects and
CTD/model branch changes, and is not allocated by an invented rule.

Cash PV and futures quote-level marking use different measurement bases. Local
totals group by currency and basis, and reporting totals still keep the two
bases apart. A futures level is the unrounded fair price in points times the
currency-per-point multiplier, per contract. Its change is an indicative model
marking.

This same-inputs scenario cannot infer quoted margin, clearing cash, daily
variation margin, realized trading P&L, accrued or carry time passage, or
lifecycle and ledger P&L. They need their own inputs.

## Delivery alternatives and CTD switches

Every delivery point reprices every cash member on the scenario curve. Each
member keeps its observed clean price, baseline and scenario model cash,
accrued interest, profile and observed-minus-model residual basis. Member
repo and clean-basis moves keep the financing, coupon carry, conversion
factors, invoices and source revisions. The basket and carry code reselects the
CTD at every point.

The alternatives keep independent selection and fair-value winners and exact
tie sets. Each ordered step records both endpoints, the changes to both winner
sets and whether either endpoint is tied. Standalone and final valuations keep
their CTD diagnostics. No baseline CTD is frozen for the combined P&L.

Fixed-date delivery is the supported scope. Timing, wildcard and end-month
options, daily-margin financing and other delivery models are not covered. The
economic distinction follows [CME's CTD explanation](https://www.cmegroup.com/education/courses/introduction-to-treasuries/get-to-know-treasuries-ctd)
and [delivery process](https://www.cmegroup.com/trading/interest-rates/files/us-treasury-futures-delivery-process.pdf).

## Bounds and tests

Requests need 1 to 128 positions, 1 to 16 moves, and explicit reservations for
up to 33 graph points and 4,224 product calls. Delivery reserves its
`12 * members * factors` policy before book scenario preparation. The member,
calibration, schedule, model and host resource ceilings all still apply. The
call counts are not measured latency or capacity. No partial book is returned
after a product, fit or source failure.

The tests include:

- opposing multi-factor mixed books and reversed paths;
- exactly flat projected quantities and USD/EUR conversion;
- recovery, basis, OAS and HW1F interactions;
- reselected delivery switches and ties;
- two quote rebuilds, ICMA cash, cash state and dated rate observations.

Shared JSON, C, MCP and compiled-WASM tests, the feature consumers, the schema
and the separate C-library parity checks complement these. Java/JNI and
Windows/.NET/Excel test paths exist, but they, and wasm32 and browser
execution, have not been run locally.

Not yet covered: full market, model and convention coverage, independent
quantitative review, causal lifecycle P&L, and server authority.

[ORE's analytics guide](https://opensourcerisk.org/content/uploads/2026/04/userguide.pdf)
gives context for full revaluation and P&L explanation. Nothing here claims
equivalence with ORE.
