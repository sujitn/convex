# Causal mixed-book lifecycle and cash reconciliation

`convex::library::execute` and the shared `book_pnl` operation reconcile full
supplied before/after economics over an explicitly ordered causal path. The same
operation is available through shared JSON, `convex_library`, MCP and the native
WASM adapter. It is pure, deterministic analytics. It neither authenticates cash
or executions nor posts a ledger, reserves risk, sends orders or reads a clock.

Run the bounded independent financial consumer:

```sh
cargo run --locked -p convex --example causal_pnl
cargo test --locked --all-features -p convex -p convex-ffi -p convex-mcp -p convex-wasm --test causal_pnl -- --test-threads=1
cargo test --locked --all-features -p convex --test causal_pnl_boundaries -- --test-threads=1
```

The complete source controls live in `tests/fixtures/library/causal-pnl-economic-cutoffs.json`:
13 financial books, 55 typed failures and 224 complete product revaluations.
Earlier preparations, inputs and failures remain separately retained. Monetary
answers are independently declared from original full dated cash, credit,
rates, Gaussian option and carry formulas. Native envelopes prove transport
agreement only. `--emit-parity` returns complete native envelopes;
`--emit-schema` requires the `schemars` feature.

## Complete local inputs

The outer `LibraryInputs` must be empty. `MixedBookPnlInput` carries:

| Input | Required meaning |
|---|---|
| `resources` | Complete common calendars, original received fixing revisions and administrator publication schedule; no curves or calibration here. |
| `markets` | Complete original curve recipes and, where used, full actual calibration helper graphs. |
| `holdings` | Complete original product/model/context/numerical policies and complete opening lifecycle plus supplied replay prefix for every cohort. |
| `before`, `steps[].after` | Exact unique snapshot and manifest, economic date, knowledge cutoff, ordered expected holdings, current/pending quantities, direct reporting FX and actual interval fills. Local indices select whole supplied market/holding bundles. |
| `standalone` | One complete independently admitted point or explicit missing-prerequisite reason per cause. No absent effect becomes zero. |
| `reporting_currency` | Explicit common reporting currency. |
| `max_points`, `max_product_evaluations` | Declared complete work admission within the fixed ceilings. |

Bindings only deduplicate the wire representation. Each point reconstructs all
actual values from this request, then independently rebuilds checked products,
curves/calibration and replayed states through their original native owners.
There is no database, provider, message bus, external ID lookup or mutable cache.
Appending point-specific observations/publication decisions preserves the
complete original history prefix. Unused or foreign local bundles fail.
Caller manifests identify supplied data; this API does not authenticate them.

The hard limits are 1–128 holdings, 1–16 ordered phases, 1–33 complete points and
1–4,224 logical product marks. The existing 2 MiB input, depth/node/string and
16 MiB serialized-response guards remain. Full original product, schedule,
history, graph, solver and optional-model convergence guards remain. Terminal
marks count toward admission even when native lifecycle state proves exposure
has ended. Failed builds or marks return no partial accepted book.

## Causal admission

Every change is checked against its declared category. Original event prefixes,
fixing revisions, immutable curve/calendar/model/FX versions and complete
expected holding identities cannot be replaced or silently relabelled.
Economic date and knowledge cutoff are explicit and nondecreasing. Synthetic
counterfactuals can hold economic time fixed while exposing later knowledge;
the native product owner still enforces economic observation eligibility.

| Cause | Admitted economic change |
|---|---|
| `time` | Root/carry movement; original due profile, coupon/principal and already received fixing boundaries. Actual notified exercise can occur on its original date. Full delivery funding interval starts and their explicit resource revisions move with time; rates/quotes do not. |
| `fixing` | Newly received/published revisions and administrator decisions, original fixing stamps and native fixing events. |
| `market` | Whole versioned curves/helper graphs and original observed cash, issuer, repo and basket inputs. |
| `model` | Explicit supported optional-model assumptions/OAS or futures convexity assumptions, keeping original numerical policies. |
| `fx` | Exact direct observation for each consumed foreign currency and this point's reporting currency. |
| `amendment` | Complete prospective supplied economics before replayed events; historical amendment/cohort rebuilding is not inferred. |
| `position` | Actual supplied fills plus current/pending quantities. Current equals opening quantity plus actual interval fills. Futures fills require original integer contracts and valid original dated quote units. |
| `event` | Actual receipts, notice/exercise/default/auction/clearing and settlement facts. A paid-CDS flag requires the actual corresponding recorded receipt. |

A metadata-only phase is invalid. A market change cannot hide under FX or time.
Changed numerical domains/settings cannot masquerade as changed quotes.
Existing correct `lifecycle`, `book_scenario` and valuation APIs remain separate.

## Values, obligations and cash

Every mark retains the full native valuation, original lifecycle view, original
proposed effects, exact claim keys/economic/due dates, proposed and recorded
amounts, outstanding amounts and full CTD diagnostics. Mandatory original
boundaries up to the point must have been replayed. The same checked full terms,
native opening quantities, date/cutoff and original quantity/reference meanings
bind each product and state. Current, pending, projected and actually executed
quantities remain distinct; outstanding claims belong to current holdings.

For cash-PV instruments:

```
current mark = remaining native product PV + outstanding claim PV
               - claim cash already embedded in that product PV
model gain level = current mark + recorded cash since before
                   + signed actual fill consideration + actual signed fees
interval model P&L = closing gain level - opening gain level
```

Earned coupons/principal remain claims until supplied receipts settle them.
Partial receipts reduce the exact outstanding claim. Actual completed claims
are removed from any original active product cash only through its full original
coupon/principal/PV owner. Future claims use the explicitly supplied same-currency
collateral DF ratio. Overdue cash requires an explicit `nominal` policy or an
incomplete result under `reject`; credit/funding/recovery of arrears is not
invented. Notified supported fixed-price fixed-coupon exercise uses the original
checked workout cash owner. Matured, exercised, terminated and observed-default
states retain outstanding claims after future exposure ends.

Futures quote-level marking is a distinct basis and total. Actual fill
consideration means signed quantity × execution points × currency/point; it is
not paid cash. Quote model gain deducts that consideration and includes actual
fees. Daily/final margin proposals and receipts are retained separately and are
not added again to quote model gain. An ended rate future has zero remaining
model exposure and an explicit unrounded actual final quote anchor in its gain
level. Native rounded clearing cash remains separate. Margin obligations have
`null` PV/discount measurements, rather than a false zero cash-PV claim.
Theoretical model changes are not official variation margin or delivery-option
value. [CME's settlement explanation](https://www.cmegroup.com/education/courses/introduction-to-futures/mark-to-market)
describes the official daily settlement basis.

Each point converts its full levels and held interval cash through its exact
supplied direct FX. Native totals group by currency **and** valuation basis;
reporting totals still separate cash PV from futures quote marking.

## Attribution and CTD state

The result reports total change, each ordered prefix minus its predecessor,
actual standalone changes, nonlinear interaction and the actual telescoping
residual. Interaction is unavailable if any standalone is unavailable. Cause
order is part of the answer. Residuals are reported without clamping or plugging.
This is model/cash reconciliation of supplied facts, not an authoritative daily
realized-P&L statement. Full P&L and P&L explain are also separate workflows in
[ORE's user guide](https://opensourcerisk.org/content/uploads/2026/04/userguide.pdf);
no ORE equivalence is asserted by this implementation.

Each delivery point revalues every original basket member and recomputes
financing, invoice/carry and independent selection/fair CTD winners. Full ties,
selection changes and fair-deliverable changes survive the causal path. Actual
physical settlement is not a model-selected CTD exercise.

## Explicit remaining limits

Unknown actual recovery/auction terms, unsupported forced floating or multiple
notices, conditional earned-cash decompositions not owned by the original
optional/issuer/credit model, rolling curve-model/domain transitions and
historical amendments return typed/incomplete outcomes. Fills across existing
claims, partial termination or defaults need separate original cohorts. Physical
delivery needs a separately identified received security and complete asset/
invoice reconciliation; a security receipt alone does not create its valuation.
Active CDS valuation with unpaid initial cash beyond the native pricing owner's
supported settlement cutoff requires a separate claim-aware pricing owner.
The checked lifecycle state preserves the actual received notice event through
`LifecycleState::exercise_notice()`. Its existing deadline tuple remains the
contractual last notice date; it is not treated as the actual issuance date.
The original American callable cash fixture remains typed unsupported by the
plain bond pricer; its plain stepped-bond control is an explicitly different
contract revision. Neither input is silently simplified during valuation.

Daily-margin financing, delivery options, authoritative ledger/FX cash posting,
position reservations/fencing and execution remain owning server work. Actual
Java/JNI, Windows/.NET/Excel and wasm32/browser execution require their own hosts.
Engineering controls do not supply independent quantitative review or product,
model, library, host, production or latency certificates. CVX-063 remains partial.
