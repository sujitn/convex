# Mixed-book P&L attribution

The `book_pnl` operation, run through `convex::library::execute`, reconciles a
book's before and after economics over an explicitly ordered path of causes
(time, fixings, market moves and so on). It is also available through the shared
JSON interface, `convex_library`, MCP and the WASM adapter.

It is pure, deterministic analytics. It does not authenticate cash or
executions, post a ledger, reserve risk, send orders or read a clock.

To run the example and its tests:

```sh
cargo run --locked -p convex --example causal_pnl
cargo test --locked --all-features -p convex -p convex-ffi -p convex-mcp -p convex-wasm --test causal_pnl -- --test-threads=1
cargo test --locked --all-features -p convex --test causal_pnl_boundaries -- --test-threads=1
```

The test controls live in `tests/fixtures/library/causal-pnl-economic-cutoffs.json`:
13 books, 55 typed failures and 224 product revaluations. Earlier preparations,
inputs and failures are kept separately. The expected monetary answers are
declared independently from dated cash, credit, rates, Gaussian option and carry
formulas. Rust envelopes only show that the transports agree. `--emit-parity`
returns the full Rust envelopes; `--emit-schema` requires the `schemars`
feature.

## Inputs

All data comes in the request. The outer `LibraryInputs` must be empty.
`MixedBookPnlInput` carries:

| Input | Required meaning |
|---|---|
| `resources` | Common calendars, received fixing revisions and the administrator publication schedule; no curves or calibration here. |
| `markets` | Curve recipes and, where used, the calibration helper graphs. |
| `holdings` | Product, model, context and numerical policies, plus the opening lifecycle and replay prefix for every cohort. |
| `before`, `steps[].after` | A unique snapshot and manifest, economic date, knowledge cutoff, ordered expected holdings, current/pending quantities, direct reporting FX and interval fills. Local indices select whole market/holding bundles. |
| `standalone` | One independently computed point, or an explicit missing-prerequisite reason, per cause. An absent effect never becomes zero. |
| `reporting_currency` | Explicit common reporting currency. |
| `max_points`, `max_product_evaluations` | Declared work limits, within the fixed ceilings. |

Bindings only deduplicate the wire representation. Each point rebuilds all
values from this request, then rebuilds the checked products, curves,
calibration and replayed states through the Rust code that owns them. There is
no database, provider, message bus, external ID lookup or mutable cache.
Appending point-specific observations or publication decisions preserves the
existing history prefix. Unused or foreign local bundles fail. Caller manifests
identify the data; this API does not authenticate them.

The hard limits are 1–128 holdings, 1–16 ordered phases, 1–33 points and
1–4,224 product marks. The existing 2 MiB input, depth/node/string and 16 MiB
serialized-response guards remain, as do the product, schedule, history, graph,
solver and optional-model convergence guards. Terminal marks count toward the
limits even when the lifecycle state shows exposure has ended. A failed build or
mark returns no partial book.

## Causal checks

Every change is checked against its declared category. Event prefixes, fixing
revisions, immutable curve/calendar/model/FX versions and expected holding
identities cannot be replaced or silently relabelled. Economic date and
knowledge cutoff are explicit and nondecreasing. Synthetic counterfactuals can
hold economic time fixed while exposing later knowledge; the product code still
enforces which observations are eligible at the economic date.

| Cause | Accepted economic change |
|---|---|
| `time` | Root/carry movement; the due profile, coupon/principal and already received fixing boundaries. A notified exercise can occur on its original date. Delivery funding interval starts and their resource revisions move with time; rates/quotes do not. |
| `fixing` | Newly received or published revisions and administrator decisions, fixing stamps and fixing events. |
| `market` | Whole versioned curves and helper graphs, and observed cash, issuer, repo and basket inputs. |
| `model` | Supported optional-model assumptions/OAS or futures convexity assumptions, keeping the numerical policies. |
| `fx` | Exact direct observation for each consumed foreign currency and this point's reporting currency. |
| `amendment` | Prospective economics before replayed events; historical amendment/cohort rebuilding is not inferred. |
| `position` | Fills plus current/pending quantities. Current equals opening quantity plus interval fills. Futures fills require integer contracts and valid dated quote units. |
| `event` | Receipts, notice/exercise/default/auction/clearing and settlement facts. A paid-CDS flag requires the corresponding recorded receipt. |

A metadata-only phase is invalid. A market change cannot hide under FX or time.
Changed numerical domains/settings cannot masquerade as changed quotes.
Existing correct `lifecycle`, `book_scenario` and valuation APIs remain separate.

## Values, obligations and cash

Every mark keeps the full valuation, lifecycle view, proposed effects, claim
keys, economic and due dates, proposed and recorded amounts, outstanding amounts
and CTD diagnostics. Mandatory boundaries up to the point must have been
replayed. The same checked terms, opening quantities, date/cutoff and
quantity/reference meanings bind each product and state. Current, pending, projected and actually executed
quantities remain distinct; outstanding claims belong to current holdings.

For cash-PV instruments:

```
current mark = remaining native product PV + outstanding claim PV
               - claim cash already embedded in that product PV
model gain level = current mark + recorded cash since before
                   + signed actual fill consideration + actual signed fees
interval model P&L = closing gain level - opening gain level
```

Earned coupons/principal remain claims until the receipts you pass in settle
them. Partial receipts reduce the outstanding claim. Completed claims are
removed from active product cash only through the coupon/principal/PV code that
owns it. Future claims use the same-currency collateral DF ratio you pass in. Overdue cash requires an explicit `nominal` policy or an
incomplete result under `reject`; credit/funding/recovery of arrears is not
invented. A notified fixed-price, fixed-coupon exercise uses the checked workout cash
code. Matured, exercised, terminated and observed-default
states retain outstanding claims after future exposure ends.

Futures quote-level marking is a distinct basis and total. Fill
consideration means signed quantity × execution points × currency/point; it is
not paid cash. Quote model gain deducts that consideration and includes
fees. Daily/final margin proposals and receipts are retained separately and are
not added again to quote model gain. An ended rate future has zero remaining
model exposure and an explicit unrounded final quote anchor in its gain
level. Rounded clearing cash remains separate. Margin obligations have
`null` PV/discount measurements, rather than a false zero cash-PV claim.
Theoretical model changes are not official variation margin or delivery-option
value. [CME's settlement explanation](https://www.cmegroup.com/education/courses/introduction-to-futures/mark-to-market)
describes the official daily settlement basis.

Each point converts its levels and held interval cash through its direct FX
observation. Totals group by currency **and** valuation basis;
reporting totals still separate cash PV from futures quote marking.

## Attribution and CTD state

The result reports total change, each ordered prefix minus its predecessor,
standalone changes, nonlinear interaction and the telescoping residual.
Interaction is unavailable if any standalone is unavailable. Cause order is part
of the answer. Residuals are reported without clamping or plugging.

This is model and cash reconciliation of the facts you pass in, not an
authoritative daily realized-P&L statement. Full P&L and P&L explain are also
separate workflows in
[ORE's user guide](https://opensourcerisk.org/content/uploads/2026/04/userguide.pdf);
this implementation is not claimed to match ORE.

Each delivery point revalues every basket member and recomputes
financing, invoice/carry and independent selection/fair CTD winners. Full ties,
selection changes and fair-deliverable changes survive the causal path. Physical
settlement is not a model-selected CTD exercise.

## Remaining limits

These cases return typed or incomplete outcomes:

- unknown recovery or auction terms;
- forced floating or multiple notices;
- conditional earned-cash decompositions that the optional, issuer or credit
  model does not provide;
- rolling curve-model or domain transitions;
- historical amendments.

Fills across existing claims, partial termination or defaults need separate
cohorts. Physical delivery needs a separately identified received security and
a full asset/invoice reconciliation; a security receipt alone does not create
its valuation. Active CDS valuation with unpaid initial cash beyond the pricer's
supported settlement cutoff needs a separate claim-aware pricer.

The checked lifecycle state keeps the received notice event through
`LifecycleState::exercise_notice()`. Its deadline tuple remains the contractual
last notice date; it is not treated as the actual issuance date. The American
callable cash fixture stays unsupported by the plain bond pricer, and its plain
stepped-bond control is a different contract revision. Neither input is
silently simplified during valuation.

Not yet covered: daily-margin financing, delivery options, ledger and FX cash
posting, position reservations and execution (server work). Java/JNI,
Windows/.NET/Excel and wasm32/browser execution need their own hosts. None of
this has been independently reviewed against a vendor system.
