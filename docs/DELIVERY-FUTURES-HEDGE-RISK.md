# Full delivery-futures basket risk and hedges

`convex::library` calculates graph risk, scenarios and hedge columns for a
complete physical bond-futures basket. This page describes that route, which
Rust code reaches through `bond_futures_graph_risk`, `bond_futures_graph_scenario`
and `workflow::delivery_futures_hedge_risk`.

Each scenario reprices every cash security, reapplies its financing and coupon
carry, and selects the cheapest-to-deliver (CTD) bond again. All alternatives are
kept, including exact ties and CTD switches. The existing cash/carry, invoice,
curve/calibration, differentiation and hedge code does the financial calculations.

Run the example and its tests:

```sh
cargo run --locked -p convex --example delivery_hedge
cargo test --locked --all-features -p convex -p convex-ffi -p convex-mcp -p convex-wasm --test delivery_hedge -- --test-threads=1
cargo test --locked --all-features -p convex --test delivery_boundaries -- --test-threads=1
```

It checks 79 complete transport requests: 62 full-basket risk/scenario and typed
boundary controls, two fresh five-factor columns, and 15 full hedge-book requests.
The direct embedding test also checks 14 complete controls against the Rust
basket code. It rejects a different complete curve recipe that shares the
same ID and revision. The fixtures are synthetic, with explicit source quality,
availability and revisions. They are not exchange security data, and they have not
been validated against market conventions.

## Operations and integration

| Existing shared operation | Input | Output |
| --- | --- | --- |
| `calibration` / `product_risk` | `product.kind=bond_future`, complete `BondFutureInput`, stamped discount curve, explicit member budget, graph, factor, signed contracts, snapshot and differentiation settings | Five full baskets, actual signed monetary sensitivity, frozen-member and one-sided diagnostics, ties and switches |
| `calibration` / `product_scenario` | Same full product/graph/position, coordinate and signed delta | Full baseline/shocked baskets and signed quote-level change |
| `graph_hedge_risk` | Same full product/graph, exact hedge identity, positive one-contract reference, all required factors/settings, snapshot and work bounds | Full per-factor basket results and the `HedgeRiskVector` |
| `hedge` / `aggregate`, `solve` | Fresh complete vectors, current/pending holdings, candidate lots/limits/liquidity, side costs, eligibility and reporting FX | Exposure report or bounded `proposal_only` hedge result |

These extend existing operations. No new route, database, queue, market-data
adapter, global state or clock is added. Applications resolve all inputs through
their existing provider-neutral ports before calling the library.

Shared JSON, `convex_library`, MCP and WASM call the same calculation. Java and
Excel have their own complete-envelope tests and CI generation, but those tests
have not yet run on real Java and Excel hosts.

Full runnable requests and independent answers are in
[the accepted controls](../tests/fixtures/library/delivery-hedge-decimal.json),
[quote rebuild controls](../tests/fixtures/library/delivery-hedge-rebuilt.json) and
[non-tied switch controls](../tests/fixtures/library/delivery-hedge-frontier.json).
To print the first request:

```sh
python3 - <<'PY'
import json
from pathlib import Path
document = json.loads(Path('tests/fixtures/library/delivery-hedge-decimal.json').read_bytes())
print(json.dumps(document['cases'][0]['request']))
PY
```

## Factor meaning and complete repricing

Borrow the discount curve from `graph.baseline().curves`. It must be that same
immutable, built curve, with the exact complete recipe: source, construction,
role, time basis and dependencies. A different fit or resolved dependency can
share a helper or derived recipe, so recipe equality alone cannot accept a
foreign curve. The shared facade resolves the graph automatically, and cloned
curve sets keep the same immutable owners.

Every member has:

- one exact security and revision;
- a full validated bond contract and calendar set;
- an observed clean quote and its source;
- a conversion factor, repo rate and haircut;
- an own-cash policy and a coupon-carry policy.

The basket code rejects incomplete or duplicate baskets, unsupported terms and
missing, stale or contradictory source facts.

Curve moves price the complete dated cash under the scenario discount curve.
The residual between the observed clean price and the model clean price is held
fixed, and the resulting scenario clean price goes into the full carry pricer.
Quote coordinates rebuild the calibration graph. They keep complete recipes,
helper repricing in native quote units, dependency order, measured work and
frozen fixing-publication decisions. Node moves follow the graph's own rules.

Raw member factors require the exact security ID, source ID and revision,
the security's currency and the `spot` tenor:

- `RepoRate` with `DecimalRate` changes only that member's actual repo.
- `CashCleanPrice` with `PricePer100` changes only that member's clean residual
  basis, keeping its cash model and quotation context.

Every other member is repriced under each move. A raw factor that no accepted
cash hedge uses still gets all five full valuations; its zero exposure is
calculated, not a constant you pass in. The existing parallel-repo route is
still available for an explicitly parallel scenario. This route does not invent
one source revision for the repos of unrelated securities.

## CTD changes and numerical checks

The selected member (minimum net basis, maximum implied repo or minimum forward)
is distinct from the minimum-forward member that sets this fixed-date model's
fair futures price. Every evaluation reports both winners and all alternatives.
Exact Decimal ties keep the identity and revision order. A small positive gap
between two members is not rounded into a tie.

Risk evaluates exactly `0, +h, -h, +h/2, -h/2`. The central/half-step
stability policy must pass. The result contains:

- the monetary sensitivity with the CTD reselected;
- a separate diagnostic with the fair member held fixed;
- plus-side and minus-side secants;
- selection switches, fair-member switches and ties;
- `ctd_stable`.

 A switch or tie
sets `ctd_stable=false`. These are finite-bump measurements at the supplied
coordinate and size; they do not assert a smooth infinitesimal derivative or
value the probability of a future switch. Unresolved half-step risk returns a typed failure and no hedge column. Full
signed scenario repricing is available independently of the derivative check.

Independent controls keep the larger-bump instability cases and add
separately declared smaller-bump controls under the same tolerances. Non-tied
controls cross a real positive baseline selection gap. The controls also cover
both shock signs, negative positions, reordered baskets, identical ties, distinct
competing maturities, one-member baskets and complete quote rebuilds.

The carry, net-basis and implied-repo relationships follow the concepts described
in [CME's CTD introduction](https://www.cmegroup.com/education/courses/introduction-to-treasuries/get-to-know-treasuries-ctd).
Conversion factors and clean cash versus converted futures basis are described
in [CME's basis introduction](https://www.cmegroup.com/education/courses/introduction-to-treasuries/the-basics-of-treasuries-basis).
These sources explain the economics. The implementation and synthetic controls
have not been checked against exchange-specific conventions.

## Money, work and hedge authority

Futures money is unrounded fair price points times the supplied currency per
point per contract. The signed integer contract quantity scales it once.
The hedge reference is exactly one positive contract; cash keeps its positive
face reference. This is quote-level marking and scenario
change, distinct from discounted lifetime PV, rounded daily margin, actual
cash posting and realized P&L. Reporting FX is explicitly attached to fresh
vectors with the same snapshot; it is not guessed from currency names.

Full USD and EUR five-factor cash/delivery books keep current and pending
holdings, exact lots, position and liquidity limits, cost and eligibility
observations, and FX. The independent close-out controls require sale of 350
cash face and three futures contracts, zero final holdings and all factor
constraints, with side costs 7.25 USD or 6.525 EUR. Ineligible, missing, stale, mismatched, illiquid and unaffordable alternatives
either fail input validation or return the infeasible proposal. They never grant
order authority.

`BondFuturesGraphPolicy` reserves `12 * members * factors` member-price/carry
visits before calculation: the carry, the cash model and five cash/carry
evaluations. It accepts 1..128 members and 1..16 factors, within an explicit
budget of 1..24,576. A two-price scenario conservatively uses the same
reservation. The independent graph/calibration, five-factor-callback and hedge
solver bounds still apply. These work limits do not measure latency.

The deterministic model accepts fixed-coupon bullet deliverables at par,
including stepped coupons. Optional, floating, amortizing, non-par, indexed and
ex-coupon deliverables are not supported. Delivery timing, the wildcard option,
end-month options and daily-margin financing are not valued. Generic delivery
bucket risk is not supported; use the dedicated basket API described here.

Not yet covered: additional factor mappings, stochastic delivery optionality,
lifecycle and P&L, solver behavior on large books, measured latency on
multiserver or colocated setups, and server-side limits, pending orders and
execution authority. This work is partial, and there is no independent
financial, model or production certification.

The adapter outputs only prove that every host returns the same text as Rust.
They do not validate the financial results. Java, JNI, Windows, .NET, Excel,
wasm32 and browser hosts have not yet been run.
