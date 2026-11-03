# Optional bonds: graph risk and hedge columns

This document covers risk and hedge columns for callable and putable bonds. It
combines the optional-bond model with graph risk and the constrained hedge
calculation. The code is the shared `graph_hedge_risk` operation, the
`optional_bond` operation and `convex::library`.

Optional cash is priced by the checked HW1F stopping model, using the
contractual cash, notional and coupon steps, accrual, notice, settlement and
eligible observations. The implementation is a pure library. It takes immutable
resources you supply and proposes hedge quantities. Server authorization,
providers, storage, message queues, live orders and cash posting are outside it.

Not yet covered: shaped-node optional risk, recalibrated market quote and
volatility risk, optional graph bucket reconciliation, all-member
bond-delivery-futures hedge columns, unavailable hosts, and independent model,
library and production certification. This work is partial.

## Running it

```sh
cargo run --locked -p convex --example optional_graph_hedge > /tmp/optional-graph-hedge.json
cargo test --locked --all-features -p convex -p convex-ffi -p convex-mcp -p convex-wasm --test optional_graph_hedge -- --test-threads=1
cargo test --locked --all-features -p convex --test optional_graph_boundaries -- --test-threads=1
```

The example calculates fresh columns, checks independent financial controls,
compares the native risk results, and builds the hedge books from those
columns. Its JSON output contains the requests and full envelopes for transport
comparison. Rust output is never the independent financial reference.

The inputs are in these fixtures:

- [admitted controls](../tests/fixtures/library/optional-graph-hedge-resolved.json);
- [direct operations](../tests/fixtures/library/optional-graph-hedge-direct.json);
- [additional boundaries](../tests/fixtures/library/optional-graph-hedge-qualification.json);
- [calibrated full-OAS control](../tests/fixtures/library/optional-graph-hedge-calibrated.json);
- [book templates](../tests/fixtures/library/optional-graph-hedge-policy-book.json).

The raw trials remain next to their separately declared controls. The earlier
calibrated trial is kept but is not used as a control.

## Operations

`graph_hedge_risk` now accepts `product.kind = "optional_bond"`. The product
contains the existing `OptionalInput`, an `instrument` reference pin, the
fractional `oas_decimal`, and separate `policies.rates` and `policies.model`.
The graph, hedge identity, snapshot, reference position, factor manifest and
evaluation reservation keep their earlier contracts.

`calibration.action.kind = "product_risk"` accepts the same product. It returns
one canonical sensitivity plus both native risk results.

The `optional_bond` operation gains two actions:

| Action | Required additional inputs | Result |
| --- | --- | --- |
| `model_scenario` | `oas_decimal`, `parameter`, signed `delta` | Full price, grid/exercise diagnostics and model provenance |
| `model_risk` | `oas_decimal`, `parameter`, positive `bump`, explicit `settings` | First and second parameter derivatives on every grid and full base/up/down prices |

`parameter` is `mean_reversion` or `volatility`. The existing rate `risk`, rate
`scenario` and OAS-only `quote_risk` operations are unchanged, and no alias
bypasses their checks. Generic optional product scenarios and bucket
reconciliation are not integrated yet and return typed unsupported outcomes.

The shared request schema, the C response, the MCP library entry point and the
compiled WASM export all call the same library code. Java and Excel transport
tests and CI generation are extended. Those hosts, Windows, and wasm32 and
browser execution have not been run on this machine.

## Factor identity, money and units

All factors use the existing `QuoteRiskKey`. The `RiskCoordinate` enum is
unchanged. The input unit is `DecimalRate`, and the response is
`frozen_curves` for this integration.

| Factor | Coordinate identity | Reported sensitivity |
| --- | --- | --- |
| Discount parallel | `Zero` discount curve ID/version, `node = null` | Currency per +1bp |
| Projection parallel | Independent `Zero` projection ID/version, `node = null` | Currency per +1bp |
| Joint parallel | `Raw("joint_zero")`, `joint_zero_rate`, `zero_node`; `curve_pair` target with **both** curve IDs and opaque string revisions | Currency per simultaneous +1bp to both roles |
| Optional OAS | `Raw` security ID and exact security revision, `oas_spread`, matching instrument target | Currency per +1bp additive discount OAS |
| HW1F mean reversion | `Raw("mean_reversion")`, `hw1f_mean_reversion`, `model_parameter`; `model` target with exact model ID/revision | Currency derivative per unit of mean reversion in 1/year |
| HW1F diffusion | `Raw("volatility")`, `hw1f_volatility`, same exact model target | Currency derivative per unit of short-rate diffusion in 1/sqrt(year) |

The Raw revision identifies the coordinate definition you supply. The source
curve and model string revisions stay in the target. Different curve pairs or
model revisions do not net as equal keys. The HW1F diffusion parameter is not a
swaption market-volatility quote. A model-semantics version you supply is part
of every key.

The hedge reference uses a positive original face and `quantity_unit = "notional"`. The
graph price and currency PV use that original face. They are not rebuilt from a per100
invoice or an outstanding amortized balance. Direct product risk also accepts
signed full trade copies with a unit reference. Scaling happens once, in the
five-PV normalizer. Model-risk diagnostics report derivatives of dirty price
per100 face. The canonical monetary column separately reports its position
scale.

OAS represents the optional security quotation you supply. It does not invent
CDS legal-default exposure. Only `oas_spread` is excluded from the hedge
factor's mandatory CDS credit-reference rule. All other credit, hazard and
recovery mapping and the document, seniority and recovery checks are unchanged.
Optional vectors carry no legal credit scope.

## Price and risk checks

Every accepted factor runs the native risk calculation at both h and h/2. Rates
and OAS must pass the two-finest-state-cell bump floor, price convergence and
both successive duration and convexity refinements. If the half-step is
unresolved, the whole column is rejected, even when h succeeds. The price-only
OAS callback still rejects optional risk.

Parameter scenarios change only the named HW1F constant. They rebuild the same
dated trees against the same discount inputs. The cash, exercised branches,
future reset structure, deterministic projection basis and known eligible
revisions stay intact. Each scenario keeps a `ParameterScenario` with the full
assumed or calibrated model and the signed move. Calibration quotes, fit
diagnostics and residuals are kept, and no shocked quote fit is implied.

Model-risk settings need explicit positive absolute and nonnegative relative
bounds for the first and second derivatives. All prices pass the price checks,
and the final two successive first and second refinements must both pass. The
rate-state floor has different units and does not show that a parameter step is
resolved. h/h2 stability is still required for the canonical column. Parameter
values keep the [0,10] mean-reversion and [0,1] diffusion domains, and the
central bump is positive and at most 0.1.

The five monetary PVs are base, +h, -h, +h/2 and -h/2. The h/h2 results
accompany the canonical column. Optional hedge output also includes the
baseline price and the hedge vector. Baselines must agree across all columns.
A failure never produces a partial vector or a zero fallback.

## Supported graph and contract domain

The optional curve recipes must equal the graph baseline recipes, including
values, roles, currencies and source policy. Parallel transforms currently
support ACT/365F interpolation grids: linear continuous zero rates or
log-linear discount factors. All log-linear DF model queries must stay within
the grid. Raw Flat and Linear DF tails mean something different and are
rejected. Nodes, MoneyMarket and Helpers grid sources keep their own input and
fitting checks. Derived, segmented and parametric source transforms are not
covered.

Floating optional cash has separate same-currency projection/index and
collateral discount roles. Future aligned term-reset coupons respond to the
same stochastic state, and their projected amounts are not frozen coupon
proxies. Eligible published revisions stay fixed. The supported European and
Bermudan fixed-price model constraints are unchanged. American, make-whole,
triggered, cap/floor, overnight, path-dependent and off-cycle FRN option
structures need more state and models.

## Hedge book and bounded work

The controls use a six-factor optional term-FRN and rate-futures book:
discount, projection, joint, OAS, mean reversion and diffusion. The futures
pricer reprices every OAS and model move that the futures do not consume, so
the resulting zero exposures are calculated and not inserted as constants.
Joint risk reprices both exact curve grids through the graph code.

The fresh columns go into the existing aggregator and proposal solver. These
are kept:

- current positions, pending quantities and expected holding IDs;
- instrument, economics, calculation, entity and snapshot pins;
- source cost observations and universe decisions;
- side liquidity and costs, and lots;
- USD/EUR FX for the same snapshot.

The controlled close-out is -350 original face and -3 futures contracts, with zero final
holdings and supplied costs of 10.375 USD or 9.3375 EUR. The result has
`proposal_only` authority.

The bridge reserves five callbacks per factor and accepts 1 to 16 factors, so
at most 80 callbacks. The optional risk results run six price evaluations per
factor plus one returned baseline, and the output discloses
`native_price_evaluations = 6*N + 1`. Each price keeps its tree, grid and node
work bounds, including the 32-million visit budget per grid. The callback
reservation is not the total calibration or tree work, and it is not a latency
estimate. The hedge limits of 16 factors, 4 candidates, 256 holdings and 100000
solve-work attempts are unchanged. No dependency or default changes.

## Tests

Independent controls use Decimal60 dated profiles, cash and accrual, closed
truncated Gaussian moments of the portfolio at the notice date, common-state
FRN reset economics and exact close-out arithmetic. The tolerances are:

- per100 price: 0.006;
- rate exposure: 0.0001;
- mean-reversion derivative: 1;
- diffusion derivative: 2;
- cash: 1e-10.

The price and rate policies are unchanged. These coarse-step controls do not
show that 1bp optional risk holds in general, nor market or model accuracy or
latency.

The 646 earlier quantitative records and defaults are unchanged. Independent
model and library validation, and runs on the unavailable hosts, have not been
done. The next step is all-member bond-delivery-futures risk and hedge-factor
assembly, keeping every basket, carry and quote source, CTD-switch scenario and
the indicative futures monetary meaning.
