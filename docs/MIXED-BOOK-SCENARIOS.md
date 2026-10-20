# Mixed-book scenarios and model P&L attribution

Convex can fully revalue a supplied mixed book on one immutable information set.
The pure owner composes the original cash, contextual ICMA issuer-basis, CDS,
IRS/OIS, rate-futures, HW1F optional-bond and complete delivery-basket pricers.
It returns hypothetical model marking changes with ordered factor attribution,
standalone effects, nonlinear interactions and reconciliation residuals.

Start with the complete bounded example:

```sh
cargo run --locked -p convex --example mixed_scenario
cargo test --locked --all-features -p convex -p convex-ffi -p convex-mcp -p convex-wasm --test mixed_scenario -- --test-threads=1
cargo test --locked --all-features -p convex --test mixed_scenario_boundaries -- --test-threads=1
```

The example checks eight complete independently declared financial books and
forty typed failures, then prints a compact result. `--emit-parity` emits complete
native envelopes for transport comparison; `--features schemars -- --emit-schema`
emits the actual strict request schema. Full supplied requests live in
`tests/fixtures/library/mixed-scenario-api-*.json`. Independent financial answers,
original failed inputs and separate admissions remain in the reference,
admitted, publications and boundaries fixtures; scripts regenerate them
read-only. Source/admission corrections retain the original financial answers
and bounds. Native envelopes are transport evidence, never reference prices.

## Calculation boundary

Direct embedding uses `convex::workflow::{prepare_book_scenarios,
revalue_book_position, aggregate_book_scenarios}`. Prepare a full original
`PreparedQuoteRiskGraph`, declare a `BookScenarioInput`, revalue every expected
position through a `BookScenarioProduct`, then aggregate. Results are privately
constructed and tied to that exact prepared path. Foreign, missing, duplicated
or reordered results fail. No supplied PV callback or cached Greek vector can
stand in for full product revaluation. Read-only getters expose full valuations
and attribution to native callers.

The shared request uses `operation.kind = "calibration"` with the existing full
calibration `input`, and `action.kind = "book_scenario"` with a complete
`MixedBookScenarioInput`. Its `book.positions` owns identities and quantities;
`holdings` binds each expected ID to the existing full `RiskProductInput` or
complete `CashSecurityInput` and issuer-basis model. The original
`convex::library::execute`/`execute_json`, `convex_library`, MCP `library` and WASM
`library` adapters dispatch through this same owner. No C export is added.

DB, MQ, provider connectors, clocks, distributed scheduling, ledger transactions,
position reservations and order authority stay outside this pure boundary.
Original source identity, economics/calculation manifests, legal entity, curve
keys, quote versions, calendars, fixing history, known-at policy, cash state,
models and numerical settings remain caller-supplied. The snapshot ID is an
explicit caller declaration; this function does not authenticate a snapshot or
retrieve, relabel or time-shift its data. Server ingestion must bind those
complete values to its immutable snapshot before calling the library.

## Actual graph and nonlinear path

Supply 1–16 distinct compatible `QuoteGraphMove` coordinates, in declared order.
Each `delta` is a canonical annual-rate/fraction/per100-price move, with the
original key retaining native quotation units and response policy. Actual helper
quote conversion remains in the original graph owner. Raw product rate inputs
use their original fractional decimal units: yield, Z/DM and OAS inputs must
explicitly declare `DecimalRate`. HW1F parameter moves are model assumptions,
not volatility market quotes. Their opaque model identity/version must match;
the numeric raw observation version remains a separate declared source pin.

For every prefix and standalone scenario, simultaneous rebuilding quote,
recovery and supplied-node moves apply before one original dependency build.
Frozen curve overlays then apply to the rebuilt result. All original solver,
helper repricing, rank, history and publication guards remain. Conflicting
quote/output-grid moves, duplicate or aliased raw coordinates, parallel/node
and joint/individual overlaps fail before revaluation. A declared raw coordinate
must be consumed by a complete native product somewhere in the expected book;
an absent exposure cannot become an invented zero.

The report retains exactly `1 + 2F` points: baseline, `F` ordered prefixes, then
`F` standalone moves. Every expected position is fully priced at every point,
including zero quantities and zero exposures. Each graph retains actual curves,
full original calibration diagnostics, native quote residuals, build order,
publication requirements and helper work. The final prefix is the full combined
scenario. Reversing the declared order preserves its final economics while
changing the ordered attribution.

Optional bonds retain the original full notice/exercise, cash/profile/reset,
known fixings, HW1F provenance, dated grids and price-convergence checks. The
supported graph response is frozen parallel discount/projection/joint shifts,
security OAS and named HW1F parameters. Actual materialized baseline curves and
each consumed scenario curve are bound, including identical recipes with
different fitted values. Direct or indirect shaped/rebuilt changes require
additional full-model integration and fail explicitly. Parameter scenarios
rebuild the original trees; calibrated-source provenance is retained without
claiming that the scenario refits swaption quotes. Scenario prices do not grant
a derivative-resolution certificate.

## Quantities, money and attribution

Each position supplies signed current quantity, signed hypothetical pending
quantity, native unit and a positive original reference quantity. Bonds/CDS use
the exact full face/notional. Swaps use full trade copies or the exact original
first fixed-leg notional as a scaling reference for the complete stepped trade.
Futures use integer contracts and a one-contract reference; optional bonds use
original face or full trade copies. Original security revisions must match
where the native product owns that identity; plain bond terms rely on the
caller's complete security/economics manifest.

Current, pending and projected values stay separate. Projected quantity is the
checked exact Decimal sum of current and pending, scaled once. Pending is
hypothetical: the result does not assert an execution, reservation or fill.
Reporting conversion requires one positive direct FX observation per foreign
currency, with its original observation pin and the same declared snapshot.
Missing, duplicate, extra or wrong-snapshot FX fails; no inverse, triangulation
or scenario FX shift is inferred.

For each quantity view, position and separate book total:

- Total change is the full scenario value minus baseline.
- Ordered effect `i` is prefix value `i` minus prefix value `i-1`.
- Standalone effect `i` is its standalone scenario value minus baseline.
- Interaction is total change minus the sum of standalone effects.
- Reconciliation residual is total change minus the sum of ordered effects.

The residual is calculated and retained, never forced to zero. Ordered effects
are path-dependent. Interaction includes genuine nonlinear cross-effects and
CTD/model branch changes; it is not allocated by an invented convention.

Cash PV and futures quote-level marking use different measurement bases. Local
totals group by currency **and** basis; reporting totals still separate the two
bases. A futures level is unrounded fair price points times the original
currency-per-point multiplier per contract. Its change is indicative model
marking. Quoted margin, clearing cash, daily variation margin, realized trading
P&L, accrued/carry time passage and lifecycle/ledger P&L require their own actual
inputs and owners; this same-information-set scenario cannot infer them.

## Complete delivery alternatives and CTD switches

Every delivery point reprices every original cash member on the actual scenario
curve. Each member retains observed clean price, full baseline/scenario model
cash, accrued, profile and original observed-minus-model residual basis. Exact
member-specific repo and clean-basis moves preserve original financing,
coupon-carry, conversion factors, invoices and source revisions. The original
basket/carry owner reselects CTD at every point.

Full alternatives retain independent selection and fair-value winners and
exact tie sets. Each ordered step records both endpoints, changes to both winner
sets and whether either endpoint is tied. Standalone and final valuations keep
their full CTD diagnostics too. No baseline CTD is frozen for combined P&L.
Fixed-date delivery is the admitted scope; timing/wildcard/end-month options,
daily-margin financing and additional delivery models remain separate work.
The economic distinction follows [CME's CTD explanation](https://www.cmegroup.com/education/courses/introduction-to-treasuries/get-to-know-treasuries-ctd)
and [delivery process](https://www.cmegroup.com/trading/interest-rates/files/us-treasury-futures-delivery-process.pdf).

## Bounds and qualification

Admission requires 1–128 positions, 1–16 moves and explicit reservations for up
to 33 logical graph points and 4,224 complete product calls. Delivery reserves
its original `12 * members * factors` policy before book scenario preparation;
all original native member, calibration, schedule, model and host resource
ceilings remain. Logical call counts are not measured latency or capacity.
No partial successful book is returned after a product, fit or source failure.

Controls cover opposing multi-factor mixed books, reversed paths, exactly flat
projected quantities, USD/EUR conversion, recovery/basis/OAS/HW1F interactions,
fully reselected delivery switches and ties, two actual quote rebuilds, full
contextual ICMA cash, original cash state and dated rate observations. Actual
shared JSON/C/MCP and native-compiled WASM tests, feature consumers, schema and
separate C-library parity complement these controls. Java/JNI and Windows/.NET/
Excel test paths are supplied; execution on unavailable hosts and actual
wasm32/browser execution remain unverified locally.

CVX-063 remains partial, unaccepted and uncertified. Original numerical fixtures,
financial tolerances, solver defaults, closed history, all required library
records and accepted engineering through CVX-062 remain protected. Full
market/model/convention coverage, independent quantitative review, production
and latency certification, full causal lifecycle P&L and server authority are
separate gates. [ORE's analytics guide](https://opensourcerisk.org/content/uploads/2026/04/userguide.pdf)
provides context for full revaluation and P&L explanation; these engineering
controls do not claim ORE financial equivalence or certification.
