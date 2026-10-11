# Cohort-owned cash and actual physical delivery reconciliation

`cohort_book_pnl` extends the pure causal reconciliation contract with explicit
original lots, cash entitlements and actual physical asset/invoice handoff. Use
`convex::library::execute`, shared JSON, `convex_library`, MCP or native WASM.
The existing `book_pnl` contract remains available for its original scope.

```sh
cargo run --locked -p convex --example cohort_pnl
cargo run --locked -p convex --example residual_pnl
cargo test --locked --all-features -p convex -p convex-ffi -p convex-mcp -p convex-wasm --test cohort_pnl -- --test-threads=1
cargo test --locked --all-features -p convex --test cohort_pnl_boundaries -- --test-threads=1
```

The strict input contains the same complete `book` resources, products, markets,
before/ordered/standalone points and numerical policies described in
[CAUSAL-BOOK-PNL.md](CAUSAL-BOOK-PNL.md), plus:

| Field | Meaning |
|---|---|
| `cohorts` | One exact signed original lifecycle lot per expected holding, in the same order. |
| `cohorts[].acquisition` | Already owned at opening, one full actual fill, or the checked physical delivery from one exact future. |
| `cohorts[].disposal_fill` | Optional full actual disposal of that acquired original lot. |
| `cohorts[].entitlements` | Every actual replayed original cash key appears exactly once with an explicit owned/excluded decision and supplied immutable legal source. |
| `cohorts[].entitlements[].residual` | Optional distinct legal residual ID, owned fraction and exact acquisition cutoff with the complete original prior receipt IDs. |
| `physical_deliveries` | Exact original future, separate cash-bond cohort and actual delivered security revision. |

No scalar price, invoice, delivered face, duration or Greek can replace a native
owner. Current quantity conserves opening exposure plus actual fills plus exact
checked security receipts. Partial fills use separate original cohorts. Original
native lifecycle quantity stays fixed and determines its earned cash; hypothetical
pending quantity, actual interval executions and remaining contractual fraction
remain distinct. Disposal ends the lot's held future exposure while its declared
earned entitlements and their actual receipts remain. An acquired lot explicitly
includes or excludes prior outstanding claims. A `residual` excludes the exact
previously received cash from its separately identified remaining claim. Legal entitlement is supplied, never
inferred from trade date, a calendar-day shortcut or a model-selected CTD.

Every original obligation remains visible in the replayed state and mark, including
excluded cash with its original exact key, dates, proposed/recorded/outstanding
amounts and provenance. `entitled` explains whether its value and recorded cash
belong to this cohort. Future claims use the exact same-currency collateral DF
ratio; due arrears use explicit `nominal` policy or fail under `reject`. Unknown
recovery and unavailable original conditional-cash models remain incomplete or
unsupported. No credit, collection cost, penalty or funding model is invented.

For active CDS, `price_cds_remaining_exposure` uses the same original integration,
curves, premium/protection and fair quotation equations, while excluding initial
cash for separate exact lifecycle claim valuation. Actual `Pending` remains
pending after its original settlement date. Partial receipts reduce its exact
outstanding claim; only full original actual receipt changes payment state to
`Settled`. No fictitious paid flag is used to bypass the original pricer. The
existing `price_cds` contract and its stricter initial-payment cutoff remain.
Original delayed conditional premium guards also remain.

Physical delivery uses the actual original checked delivery event, complete
invoice-security terms, legal notice, official cleared points, conversion factor,
accrued and contractual rounding. Its asset owner must contain those exact full
bond terms, security revision, signed native face, currency and reference size.
Projected asset and invoice effects are separate from actual security and cash
receipts. Security can arrive before or after invoice cash; neither implies the
other. A missing original cash-bond valuation is incomplete, never zero.

The future ends quote exposure but retains its explicit actual unrounded official
quote anchor for model gain. Rounded variation obligations stay separate from
that quote gain. The asset cohort values both received security and the exact
remaining security receivable/delivery obligation through the original full cash
owner. Its `physical_delivery` retains the checked proposal/receipt, exact face,
invoice outstanding/PV and receipts. Invoice value and recorded invoice cash are
assigned to this cash owner once; the future retains full invoice provenance
without counting the invoice again as variation cash. Partial invoice receipts
reduce the liability and increase actual cash by equal signed amounts.

Native-currency and reporting totals continue to separate cash PV from futures
quote levels. Each point uses exact direct FX. Complete ordered, independently
available standalone and nonlinear interaction effects and actual unclamped
residuals remain. These are model and supplied-fact reconciliations, without
ledger authentication, posting, risk reservations, fencing or execution authority.

Every active delivery mark revalues the full original basket and retains all
independent selection/fair winners, ties and switches. `delivery_comparisons`
records the actual selected security alongside the last complete model CTD state;
an actual delivery outside a model winner set remains an explicit actual fact.
There is no assumed CTD exercise or inferred delivery-option value.

All previous numerical controls/bounds, correct routes, native product/curve/
calibration/history/domain/solver/convergence owners and hard work limits remain.
Complete replay and cohort/claim/link admission precedes financial marking.
Failures return no partial accepted book. Each original lot has at most4096
entitlements, with the existing128-holding/33-point/4224-mark ceilings and original
2MiB wire, depth/node/string and16MiB response guards. The mathematical crates
still read no provider, database, message queue, clock or operational runtime.

Historical legal amendments, multi-lot split/novation and broader conditional
claim transfers, further delivered asset models,
delivery options and daily-margin financing remain separate work. Actual managed
Java/Excel and wasm32/browser execution, broad scale/latency and independent
quantitative/model/production certification remain unverified. CVX-063 is partial.

The complete independent engineering corpus is
`tests/fixtures/library/cohort-pnl-calendars.json` plus
`tests/fixtures/library/cohort-pnl-full-wire.json`: 14 financial books, 27 typed
failures and 372 complete product marks. It covers overdue/partly received CDS
initial cash, acquired/excluded coupons, earned cash after full disposal, partial
and full IRS termination, full five-cause and USD/EUR mixed books, long/short physical delivery
and either security/invoice receipt order. Exact original delayed calendar cash,
all basket members, CTD ties/switches and actual-versus-model delivery choices
are retained. Earlier independent preparations and failed trials remain separate;
the Python writers check those immutable bytes without reading native answers.
The original financial bounds and 37,943 independent regression comparisons
remain required. Native parity envelopes validate transport only.

The additional `residual-pnl-canonical.json` corpus contains 14 complete financial
books, 27 typed failures and 550 full native product marks. It covers fractional
coupon/recovery ownership, pre-acquisition cash, earned claims after disposal,
full five-cause and USD/EUR books, and signed physical receipts in security-first,
cash-first and interleaved sequences. Its independent writers retain the original
financial bounds and all earlier preparations and failed trials. The same 77
routes remain; 41 complete requests extend the mapped corpus to 4,595.

Residual owners declare `id`, `fraction` in (0,1], and `cutoff` with actual `date`,
`known_at`, and `receipt_ids`. A fill owner's cutoff must equal the original
acquisition fill. Opening owners use an available opening cutoff. Each excluded
receipt must belong to this exact original obligation and actual acquisition
prefix. All cash visible before acquisition retains its prior owner. Exact IDs
and supplied legal source resolve receipts sharing a date; dates alone do not
determine ownership. Duplicate, foreign, unavailable, omitted or fully exhausted
residual facts fail before pricing. The complete original earned/recovery proposal
must also be available at acquisition; later auction information cannot be
retroactively transferred. The fraction's complement is explicitly
excluded, independently of the lot's held security quantity. Variation margin
and delivery invoice transfers retain their separate original owners.

For original amount A, actual excluded receipts B, current recorded receipts C
and legal fraction f, owned proposed cash is (A-B)*f, owned recorded cash is
(C-B)*f, and owned outstanding cash is (A-C)*f. Exact checked Decimal facts
determine these amounts. `residual_ownership` retains the legal source, original
proposal, full prior receipt events and all four exact cash amounts. Original full
obligations stay visible. Disposal retains earned residual cash. Each mark must
use the same complete admitted original state and receipt provenance.

Actual security receipts may be partial. Each nonzero signed receipt targets the
original delivery key/security revision, consumes a unique actual receipt ID and
cannot exceed remaining face. `security_receipts` retains every complete event
in replay order; `security_receipt` is the latest event. Native `delivery_pending`
contains the exact remaining face until complete. Received face plus outstanding
face equals the original proposal, for long and short positions and either
invoice/security order. Current received face and remaining asset receivable use
the same complete bond owner. Partial receipts change neither the actual invoice
nor the model CTD choice. This allocating batch workflow has no latency certificate.
