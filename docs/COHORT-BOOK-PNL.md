# Cohort P&L: lots, cash entitlements and physical delivery

`cohort_book_pnl` extends the causal book P&L reconciliation with explicit lots,
cash entitlements and physical delivery of securities and invoice cash. It runs
through `convex::library::execute`, shared JSON, `convex_library`, MCP or WASM.
The existing `book_pnl` operation is unchanged for its own scope.

"Checked" below means the function validates its input and returns an error
instead of guessing.

```sh
cargo run --locked -p convex --example cohort_pnl
cargo run --locked -p convex --example residual_pnl
cargo run --locked -p convex --example claim_pnl
cargo test --locked --all-features -p convex -p convex-ffi -p convex-mcp -p convex-wasm --test cohort_pnl -- --test-threads=1
cargo test --locked --all-features -p convex --test cohort_pnl_boundaries -- --test-threads=1
```

The input holds the same `book` resources, products, markets,
before/ordered/standalone points and numerical policies described in
[CAUSAL-BOOK-PNL.md](CAUSAL-BOOK-PNL.md). It adds these fields:

| Field | Meaning |
|---|---|
| `cohorts` | One signed lot per expected holding, in the same order. |
| `cohorts[].acquisition` | Already owned at opening, one full fill, or the checked physical delivery from one future. |
| `cohorts[].disposal_fill` | Optional full disposal of an acquired lot that is not physical. Physical stock uses `physical_disposals` facts. |
| `cohorts[].entitlements` | Every replayed cash key appears exactly once, with an explicit owned or excluded decision and an immutable legal source that you supply. |
| `cohorts[].entitlements[].residual` | Optional distinct legal residual ID, owned fraction, and acquisition cutoff with the complete list of prior receipt IDs. |
| `physical_deliveries` | The future, a separate cash-bond cohort and the delivered security revision. |

## Lots and entitlements

No scalar price, invoice, delivered face, duration or Greek can replace the Rust
owner of a value. Current quantity is the opening exposure plus fills plus checked
security receipts. Partial fills use separate cohorts. The lot's quantity stays
fixed and determines the cash it earns. Hypothetical pending quantity, executions
in the interval and the remaining contractual fraction are kept distinct.

Disposal ends the lot's held future exposure (from that date on), but its declared earned
entitlements and their receipts remain. An acquired lot explicitly includes or
excludes prior outstanding claims. A `residual` excludes the cash already received
from its separately identified remaining claim. Legal entitlement is always
supplied. It is never inferred from the trade date, a calendar-day shortcut or a
model-selected CTD.

Every obligation stays visible in the replayed state and the mark. That includes
excluded cash, with its key, dates, proposed, recorded and outstanding amounts and
provenance. `entitled` says whether its value and recorded cash belong to this
cohort. Future claims use the same-currency collateral discount-factor ratio. Due
arrears use the explicit `nominal` policy or fail under `reject`. Unknown recovery
and unavailable conditional-cash models stay incomplete or unsupported. No credit,
collection cost, penalty or funding model is invented.

For an active CDS, `price_cds_remaining_exposure` uses the same integration,
curves, premium and protection, and fair quotation equations. It excludes initial
cash so that the cash can be valued as a lifecycle claim. A `Pending` payment stays
pending after its original settlement date. Partial receipts reduce its outstanding claim,
and only a full receipt changes the payment state to `Settled`. No fictitious paid
flag is used to bypass the pricer. The `price_cds` contract and its stricter
initial-payment cutoff are unchanged, and so are the delayed conditional premium
guards.

## Physical delivery

Physical delivery uses the checked delivery event, the full invoice-security
terms, the legal notice, the official cleared points, the conversion factor, the
accrued interest and the contractual rounding. The asset must contain those bond
terms, the security revision, the signed face, the currency and the reference
size. Projected asset and invoice effects are separate from actual security and
cash receipts. Security can arrive before or after the invoice cash, and neither
implies the other. A missing cash-bond valuation is incomplete, never zero.

The future ends quote exposure but keeps its unrounded official quote anchor for
model gain. Rounded variation obligations stay separate from that quote gain. The
asset cohort values both the security already received and the security still
receivable, through the full cash owner. Its `physical_delivery` keeps the checked
proposal and receipt, the face, the invoice outstanding amount and PV, and the
receipts. Invoice value and recorded invoice cash are assigned to this cash owner
once. The future keeps the invoice provenance without counting the invoice again
as variation cash. Partial invoice receipts reduce the liability and raise actual
cash by equal signed amounts.

Native-currency and reporting totals keep cash PV separate from futures quote
levels. Each point uses direct FX. The ordered, independently available
standalone and nonlinear interaction effects, and the unclamped residuals, are all
kept. These are model reconciliations of the facts you supply. They do not
authenticate a ledger, post cash, reserve risk or authorize execution.

Every active delivery mark revalues the full basket and keeps all independent
selection and fair winners, ties and switches. `delivery_comparisons` records the
security actually selected next to the last complete model CTD state. A delivery
outside the model's winner set remains an explicit actual fact. No CTD exercise
and no delivery-option value is inferred.

## Bounds

All earlier numerical controls and bounds, the routes, the product, curve,
calibration, history, domain, solver and convergence owners, and the hard work
limits are unchanged. Full replay and cohort, claim and link validation run before
any financial marking, and a failure returns no partial book. The limits are:

- At most 4096 entitlements per lot.
- 128 holdings, 33 points and 4224 marks.
- 2MiB on the wire, with the depth, node and string limits.
- 16MiB for the response.

The math crates still read no provider, database, message queue, clock or
operational runtime.

Not yet covered:

- Legal amendments, novation and conditional claim transfers beyond those below.
- Other delivered asset models and delivery options.
- Daily-margin financing.
- Execution in managed Java/Excel and in wasm32/browser hosts.
- Broad scale and latency testing.
- Independent quantitative, model and production validation.

## Test corpus

The independent corpus is `tests/fixtures/library/cohort-pnl-calendars.json` plus
`tests/fixtures/library/cohort-pnl-full-wire.json`. It has 14 books, 27 typed
failures and 372 product marks. It covers:

- Overdue and partly received CDS initial cash.
- Acquired and excluded coupons.
- Earned cash after a full disposal.
- Partial and full IRS termination.
- Five-cause and USD/EUR mixed books.
- Long and short physical delivery, with security and invoice receipts in either
  order.

Delayed calendar cash, all basket members, CTD ties and switches, and actual versus
model delivery choices are kept. The earlier independent preparations and failed
trials are kept separately. The Python writers check those fixed bytes without
reading the Rust answers. The original financial bounds and 37,943 independent
regression comparisons are still required. Parity checks between the Rust output
and the client output validate transport only.

The `residual-pnl-canonical.json` corpus adds 14 books, 27 typed failures and 550
product marks. It covers fractional coupon and recovery ownership, cash before
acquisition, earned claims after disposal, five-cause and USD/EUR books, and
signed physical receipts in security-first, cash-first and interleaved order. Its
writers keep the financial bounds and all earlier preparations and failed trials.
The same 77 routes remain, and 41 more requests bring the mapped corpus to 4,595.

## Residual ownership

A residual owner declares `id`, `fraction` in (0,1], and `cutoff` with `date`,
`known_at` and `receipt_ids`.

- A fill owner's cutoff must equal the acquisition fill.
- An opening owner uses an available opening cutoff.
- Each excluded receipt must belong to this obligation and to the acquisition
  prefix.
- All cash visible before acquisition keeps its prior owner.
- Receipts that share a date are resolved by their IDs and the legal source you
  supply, never by the date alone.
- Duplicate, foreign, unavailable, omitted or fully exhausted residual facts fail
  before pricing.
- The earned or recovery proposal must be available at acquisition. Later auction
  information cannot be transferred back.
- The complement of the fraction is explicitly excluded, whatever quantity of the
  security the lot holds.
- Variation margin and delivery invoice transfers keep their own owners.

For an original amount A, excluded receipts B, current recorded receipts C and
legal fraction f, owned proposed cash is (A-B)*f, owned recorded cash is (C-B)*f,
and owned outstanding cash is (A-C)*f. Checked Decimal facts determine these
amounts. `residual_ownership` keeps the legal source, the original proposal, the
prior receipt events and all four cash amounts. The full obligations stay visible.
Disposal keeps the earned residual cash. Each mark must use the same validated
state and receipt provenance.

## Partial security receipts

Security receipts may be partial. Each nonzero signed receipt targets the delivery
key and security revision, uses a unique receipt ID and cannot exceed the
remaining face. `security_receipts` keeps every event in replay order, and
`security_receipt` is the latest. `delivery_pending` holds the remaining face until
delivery is complete. Received face plus outstanding face equals the original
proposal, for long and short positions and for either invoice and security order.
The face received so far and the remaining asset receivable use the same bond
owner. Partial receipts change neither the invoice nor the model CTD choice. This
allocating batch workflow has not been measured for latency.

## Bond amendments and lot splits

Boundary bond amendments and conserved lots use the same `cohort_book_pnl`
operation. Run `cargo run --locked -p convex --example lot_pnl`.

A `bond_amendment` lifecycle event supplies the prior and later legal versions, the
agreement and the complete successor terms. The opening terms and the event prefix
stay. All due steps and cash must be processed before the event, at an accrual
start. Input validation keeps the full schedule, calendars, historical profile
values and principal cash, the current principal, fixing mechanics, rounding and
quotation units. Only future fixed-bond profiles may change. Earned obligations,
proposal events and receipts are unchanged, and later earned cash uses the checked
amended contract. The point's security revision must match the legal revision.
Conditional, floating, mid-period, calendar, schedule, index and retroactive
amendments fail.

Optional `lot_splits` supplies the legal identity, parent, distinct child cohorts,
effective and known-at cutoffs, source revision and every point index it applies
to. The indices refer to the baseline, the ordered endpoints and the available
standalone points, in that order. Index zero cannot apply a new interval split. A
split belongs to the `position` cause. Every later ordered point keeps it, and
unrelated standalone causes cannot acquire it. Children use
`acquisition.kind=lot_split` and the `split_id`. Their signed quantities must sum
exactly to the parent in the same direction. A later partition must follow the
child's acquisition, with no cycles and no repeated consumption.

A supported security partition needs the same fixed, unrounded, nonoptional bond,
security revision, currency, reference face, entity and fixing history at each
acquisition. Earned cash and historical receipts stay with the parent, including
separately identified residual fractions, and the children explicitly exclude all
pre-split earned keys. Future earned keys belong to the successors, and the retired
parent explicitly excludes them. Each lot and every excluded claim stays visible.
The parent's held face falls to zero and the successor face is conserved. No fill,
consideration, fee, average cost or cash receipt is inferred. A later full disposal
of a successor uses its own fill. Delivered-security partitions are covered under
"Physical custody partitions" below. Other rounded or conditional instruments need
complete owners and fail explicitly.

The limits stay at 128 holdings, 33 points and 4,224 marks, with the wire bounds.
The prepared path binds the current terms, the resolved coupon and principal dates
and all amendment, proposal and receipt facts. Mixed-book valuation, causal
interaction, CTD winner, tie and switch, and partial delivery controls remain. The
active independent controls are `lot-pnl-chronology.json`, produced by
`tests/scripts/lot_pnl_chronology.py`. Reference, forward and typed diagnostic
declarations stay next to it, and earlier trials are kept. The earlier residual,
cohort and causal suites are still required separately. Not yet covered: managed
and browser hosts, and production and latency testing.

## Claim transfers

Earned-cash transfers use the `claim_transfers` list on the same operation. Each
record supplies:

- A unique legal `id`.
- The `origin` cohort and cash `key`.
- The `proposal_id`.
- The `entitlement_source` and an optional `residual_id`.
- The current legal `from` and `to`.
- A positive `fraction`.
- The cash `cutoff`.
- An immutable legal `source`.
- The `applied_points`.

Fractions refer to the entitled cash after its residual fraction and excluded paid
prefix. They are independent of the receiving lot's signed face.

Only earned, available claims can transfer. All parties must be acquired legal
cohorts with the same currency, cash discount role and overdue convention. A split
successor may receive an earned claim without replaying the parent's payments or
acquiring the security again. Legal transfers belong to the Position cause and
persist on the ordered path. Independent Position points carry only the transfer
prefix available to them. Partial, entire, sequential and returned ownership is
conserved exactly. Duplicate, foreign, unavailable, retroactive, exhausted and
overdrawn facts fail.

The cutoff keeps the ordered receipt IDs for that claim, and earlier cash stays
with its recipient. Every later receipt is allocated by the shares in force at that
time, and another transfer moves outstanding cash only. `claims[].transfer_ownership`
keeps the source's allocation. `transferred_claims` separately keeps each
recipient's cash and proposal, entitlement and residual, transfer prefix, share and
receipt allocations. The obligations and receipt history stay visible. Recorded
cash plus outstanding cash is each recipient's allocated proposed cash. Checked
Decimal partitioning conserves each cash amount exactly, and the last nonzero legal
share receives the remainder of the Decimal representation. The same-currency
collateral ratio and the point's FX price every share.

All product values, the removal of embedded cash from futures, current, pending and
fill and physical receipt facts, the causal, standalone and interaction
reconciliation, the unclamped residuals and every CTD winner, tie and switch are
kept. The allocation limits are 128 legal transfers and 4224 allocations (origin,
party and point), alongside the holding, point, pricing and wire limits. Margin and
unearned or conditional claims keep their own owners. Physical invoice transfers
are described under "Physical custody partitions". No payment, consideration, fee,
security fill or ledger posting is inferred.

The independent control is `claim-pnl-bound.json`, produced by
`tests/scripts/claim_pnl_bound.py`. Initial and earned declarations, failed trials
and separate plain-Decimal receipt facts are kept. Run
`cargo run --locked -p convex --example claim_pnl`, or the four `--test claim_pnl`
adapter tests. Parity envelopes between the Rust and client outputs are not
independent financial references. The financial bounds and 37943 regression
comparisons are still required. Not yet covered: managed and browser execution and
production and latency testing.

## Physical custody partitions

Delivered custody uses the `physical_partitions` list. Each record has:

- `split`: the parent asset, two or more child cohorts, a unique legal ID, a source,
  an economic and knowledge cutoff, and the applied points.
- The linked `future`.
- The `delivery_id`.
- The ordered security `receipt_ids` at that cutoff.

Children use a `lot_split` acquisition with this split ID. Their signed face sums
exactly to the delivered face. They keep the security revision, terms, resolved
calendars, entity, currency, units and history. The delivery and every partial
receipt stay with the future. A model CTD choice never creates custody.

At the Position partition, the parent's security custody ends and its children
acquire the allocated received and outstanding face. Custody covers the assets
already received as well as the security still receivable. Earned cash stays with
its declared owner. Every security receipt is allocated with checked Decimal
arithmetic. Received face is the exact sum of those allocations, and outstanding
face is proposed face minus received face. The children conserve all three totals.
`physical_delivery.partition` keeps the legal facts, the total faces and each
receipt's allocated face. The consumed parent's custody share is zero, and its
invoice ownership is independent. No fill, fee, payment, execution cost or
consideration is created.

An earned `delivery_invoice` may also use `claim_transfers`. Its `origin` and the
proposal and receipt pins name the future, and the linked asset is the initial
legal cash owner. Transfer `from` and `to` name acquired cash cohorts with the same
currency, collateral discount and overdue convention. Security children acquire
invoice cash only through explicit legal transfers. The receipt cutoff rules keep
earlier paid recipients, and transfers of signed outstanding cash may be
fractional, entire, successive or returned. `transferred_claims` reports the
invoice, proposal, entitlement and receipt allocations.
`physical_delivery.invoice_excluded` means the full invoice is kept as provenance
and its cash is included through those allocations. The future keeps invoice
provenance without counting it as variation cash.

Partitions persist on the ordered path and apply only to available independent
Position experiments. These fail explicitly:

- A foreign or missing delivery.
- An omitted or reordered receipt prefix.
- Unavailable knowledge.
- Duplicate custody.
- Incomplete or overdrawn face.
- A mismatched security.
- Unsupported conditional custody facts.

At most 128 holdings and partitions, 33 points and 4,224 product marks apply, and
each cutoff has at most 4,096 receipts. Shares must be exactly representable in the
Decimal face. Conditional delivery models need their own complete owners. The Rust
preparation signatures delegate with empty partitions, and omitting the field in
JSON defaults to empty. Rust `CohortBookPnlInput` literals must supply
`physical_partitions: Vec::new()` or a complete list, and
`physical_disposals: Vec::new()` or complete disposal facts.

Run `cargo run --locked -p convex --example physical_partition_pnl`. The independent
corpus is `physical-partition-pnl-final.json`, and the earlier financial, wire and
chronology trials are kept unchanged. Its nine books include long and short partial
receipts, custody before, between and after ingress, distinct cash recipients and
entire invoice transfers, and the earlier mixed cash books and CTD switches. Thirty-five
malformed controls and Rust conservation tests are separate from transport parity.
Not yet covered: managed and browser hosts, other delivery models, and production
and latency testing.

Successors may be partitioned again after their own acquisition. Each partition
binds the same `future` and `delivery_id`, and the parent must be an acquired live
custody cohort. Children conserve the parent's remaining signed stock plus the
delivery receivable. Acquisition precedes later partitions in the ordered path, and
every independently supplied information set includes its available ancestry.
Cycles, double acquisition or consumption, foreign security revisions and missing
ancestry fail. The delivery, notice, security, basket, CF, accrual, rounding and
receipt events stay visible. `physical_delivery.lineage` records the ancestry.

## Physical disposals

The `physical_disposals` list binds each disposal of delivered stock to:

- A unique legal `id`, the acquired `asset`, the `future` and the `delivery_id`.
- An existing `fill_id` with its full original quantity, consideration and fees.
- The `effective_date` and `known_at` that match that fill, an immutable `source`,
  the ordered security `receipt_ids` cutoff, and `applied_points`.
- Nonempty `receipts`, each naming a `receipt_id` and the signed received `face`
  disposed from this owner.

Receipt allocations must be distinct, already available and within this cohort's
remaining stock. Their signed sum must exactly oppose the fill quantity. Disposal
subtracts received stock, and the security receivable is unchanged. It creates no
fill, consideration, fee, payment or invoice ownership. Later partitions conserve
the custody remaining after disposal. At every point, remaining proposed face plus
disposed face equals the full delivery, and remaining received face plus disposed
face equals the receipts. `proposed_face = received_face + outstanding_face` still
holds for each legal owner. `partition.receipts` shows the remaining owned receipt
face. `physical_delivery.disposals` keeps the legal facts, the fills, the checked
receipt events and the removed face, with the disposing cohort.

For example, a delivery of 200,000 face is split into 80,000 and 120,000. After
50,000 face arrives, the first child holds 20,000 with 60,000 still due. A sale of
10,000 of stock leaves 10,000 received plus 60,000 due. Splitting that into 17,500
and 52,500 allocates 2,500 and 7,500 received and 15,000 and 45,000 outstanding.
The disposing parent keeps its sale cash and fees. Paid invoice recipients and
outstanding invoice shares stay with their own cash facts, even after all received
stock has been sold or custody consumed.

Up to 128 disposal facts and 4,096 receipt allocations per fact are accepted,
within the holding, point and product limits. Historical receipts and cash, native
quantities, legal sources, independent information sets and exact Decimal
conservation are checked before pricing. These cases produce typed failures:

- Disposal after consumption.
- Overdrawn stock.
- Unmatched or competing fills.
- Omitted receipt prefixes.
- Unavailable knowledge.
- Changed history.
- Unused facts.

Run `cargo run --locked -p convex --example custody_lineage_pnl`. The independent
controls are `custody-lineage-pnl-admitted.json`, and all earlier declarations and
failures are kept. The Rust entry point is `prepare_cohort_pnl_with_disposals`, and
earlier signatures delegate with an empty disposal list. Omitting the field in JSON
defaults to empty. The Rust, C, MCP and WASM tests, the independent model controls
and the transport parity checks are different kinds of evidence. Not yet covered:
managed and browser execution, and production and latency testing.

## Dated earned coupon and principal ownership

The `cohort_book_pnl` input accepts optional `dated_entitlements` (default empty).
Each `CohortDatedEntitlement` names:

- The `origin` and obligation `key`.
- The `proposal_id`.
- The `entitlement_source`.
- An existing `CohortResidualClaim` (`id`, positive `fraction <= 1`, and `cutoff`).
- The legal `source` revision.
- The `applied_points`.

The cohort's entitlement must be owned and must have no competing legacy residual.
This applies only to an already earned, nonconditional fixed-bond coupon or
principal, with complete terms, calendars, profiles and replayed boundaries.
Floating, optional and credit-dependent cash need their own qualified claim owners.

Cash acquisition is independent of security acquisition or disposal. Before its
Position point, the dated cash is excluded from this book. Afterward, the fraction
you supply owns the residual after the prior receipt prefix. Every earlier payment
stays visible and excluded, with its identity, amount, economic date, knowledge
cutoff and proposal. A security split may retire the source's custody while the
source keeps the later earned cash you assigned to it. A later disposal of the
security does not move earned cash. The fraction is never derived from held face,
outstanding delivery face, a fill timestamp, a price or a synthetic payment.
Excluded shares and prior cash are explicit scope exclusions, not invented
recipients.

For example, a coupon of 100 with 10 paid before the cash acquisition and a legal
fraction of 0.6 gives an owned residual of 54. A later payment of 20 gives owned
received cash of 12 and outstanding cash of 42. A later claim transfer of 0.25
moves 10.5 of that outstanding residual, and the 12 already received stays with
its recipient. Later original payments follow the legal shares in force at the time. Use a
`residual_id` equal to the dated residual ID in each `claim_transfers` fact, and
keep the proposal, source and paid-prefix pins. Receipt and outstanding allocations
conserve the Decimal cash, including the final representational remainder owner.

A dated entitlement starts on a Position cause and persists at every later ordered
point. An independent Position experiment needs the available earned proposal,
acquired cash party and receipt prefix, and unrelated experiments cannot acquire
it. These fail before valuation:

- Duplicate or foreign facts.
- Source or proposal revisions.
- Fractions outside (0,1].
- Unavailable or retroactive cutoffs.
- Missing or changing paid prefixes.
- Competing residuals.
- Exhausted claims.

The limits are 128 facts, 33 point indices, 4096 receipt IDs and 4224 product
evaluations, and the wire limits are unchanged.

Custody of received security and the disposal provenance survive coupon and bullet
redemption boundaries. Separately held cash-bond cohorts also support contractual
amortization. The physical delivery invoice still requires a fixed nominal bullet,
and pre-amortizing deliverables fail with the existing Unsupported outcome. Current
quantities stay in the original face units of the lifecycle, and the profile and ended status
determine the remaining exposure. Maturity ends model exposure and keeps the
separately owned coupon and principal receivables. New custody partitions and
disposals need the security to be active at the transition. Invoice cash, delivery,
notice, security, basket, CF, accrual, rounding and partial receipts are independent
and unchanged.

Run `cargo run --locked -p convex --example dated_cash_pnl`. The new Rust function
`prepare_cohort_pnl_with_dated_entitlements` accepts the dated facts, and all earlier
preparation signatures delegate with an empty vector. JSON callers may omit the
field. Rust `CohortBookPnlInput` literals must add `dated_entitlements`. No
operation, wire version or C export changes. The financial checkers and all CTD ties
and switches are still required.

The controls cover signed coupon, amortization and redemption, successive custody,
partial security receipts, disposal before and after the entitlement, prior and later
cash receipts, independent custody Position sets and separate invoice recipients.
Not yet covered: conditional, novation and amendment claims, fresh custody after
maturity, delivery options and margin financing, managed and browser execution, and
scale, latency and production testing. Authoritative cash, fill and FX postings,
limits, reservations, fencing and execution are outside this input API, which creates
none of them.

## Cash rights while physical delivery is pending

A dated entitlement can own an earned coupon or bullet principal while the delivery
has no security receipts or only partial receipts. The full proposed face stays
visible as received face plus the outstanding delivery receivable. Cash rights use
their own legal fraction and paid-prefix cutoff. Later receipts, successive custody
partitions and disposal of received stock do not move that cash right or the
invoice. A disposal can consume received allocations only.

An independent Position experiment supplies a complete information set and includes
its point index in the entitlement's `applied_points`. The party, earned proposal,
legal source, economic and knowledge cutoff and every excluded prior receipt must be
available in that set. It cannot borrow later receipts or legal prerequisites from
the ordered path. Before acquisition the claim is excluded, and both an actual and
an independently available acquisition apply the same residual.

For earned cash with a future payment date, the pricing curve and spread remove
embedded cash from held and receivable model exposure. Separately owned cash uses
the payment-date ratio of its collateral discount curve. Actual payments need the
due-date check. A Time roll comes before cash received on the later payment date,
and new ingress keeps its economic and knowledge clocks. Overdue cash needs the
explicit policy. Cash and security receipt histories are immutable ordered
prefixes, and a later legal cash transfer keeps recipients that were already paid.

Bullet maturity ends the security's model exposure. The remaining delivery face and
later security receipts stay visible, with separately supplied coupon, principal and
invoice ownership. No partition or disposal is accepted after maturity. Pending
redemption does not imply a market price or a new cash right. Amortizing delivery
invoices and post-delivery amendments are unsupported.

Run `cargo run --locked -p convex --example pending_cash_pnl`. The controls cover
long and short acquisition before and after partial security ingress, independent
cash Position experiments, delayed coupons with distinct discount roles, successive
custody and disposal, outstanding delivery through bullet maturity, cash receipts
and mixed-book models with CTD switches. This uses the existing preparation and JSON
operation, with no API migration. Not yet covered: managed and browser execution,
and production and latency testing.

## Earned cash on custody children

An acquired custody child can own a fixed-bond coupon or principal claim through a
`dated_entitlements` record. Supply the earned proposal, entitlement source, legal
residual fraction and ordered receipt cutoff. Earlier receipts stay excluded with
their recipients, including receipts in the child's historical replay before
security custody. Input validation checks that the child is acquired at every applied
cash point. Security face never establishes a cash right.

A claim transfer uses the same proposal and residual identity. Later receipts are
allocated to the legal shares in effect when each receipt occurred. Consumed custody
parents, and children with a disposal of received stock, keep their separately owned
cash. The delivery invoice and the pending security receivable keep their owners and
provenance.

An available independent Position experiment must supply its own baseline proposal,
paid prefix, custody facts and legal parties. It cannot import later payments or
other Event changes. Declare it unavailable when it cannot supply those
prerequisites on its own. Delayed earned cash keeps the separate pricing
discount-and-spread removal and the collateral cash discount, due-date, overdue and
FX rules.

Run `cargo run --locked -p convex --example child_cash_pnl`. The controls cover
signed first and successive custody children, excluded historical receipts,
independent cash acquisition and transfers, later cash, custody consumption and
disposal, and pending delivery. They also keep the earlier coupon, bullet principal
and mixed causal books with CTD. Amortizing physical invoices, conditional, floating
and credit cash, and post-delivery amendments still need their own complete owners.
This change affects input validation of existing fields only. No type, schema,
operation or C ABI migration is needed.

## Principal and final coupon on custody children through maturity

Run `cargo run --locked -p convex --example child_maturity_pnl` for long and short
books. Earlier custody partitions and disposals of received stock happen while the
fixed bullet security is active. Its maturity step ends the security's model exposure
and emits the full principal proposal, and the final coupon has its own proposal.
Children acquired earlier keep their nominal custody and pending delivery rights,
including later partial receipts, while their security model value is zero. Earned
cash and the invoice keep their own legal ownership and paid recipients. No new
custody or disposal is accepted after maturity.

The delayed controls set two calendar days of payment delay in every copy of the bond
terms. Cash is discounted with the collateral curve you supply until its due date,
independently of the pricing curve and spread. Actual payments are subject to the
due-date, overdue and FX rules. A child can acquire a dated residual principal or
final coupon claim, exclude an already-paid prefix, and transfer an outstanding legal
share while earlier paid cash stays with its recipient. Those cash shares are
independent of consumed custody parents, disposed received stock, pending delivery
face and the invoice.

Maturity cash acquisition and later transfers depend on Time and Event history that
the active baseline does not have. Their independent Position experiments are
explicitly `Unavailable`, and the library does not borrow later maturity proposals
or payment events. Earlier custody and disposal experiments supply their own
baseline facts. The controls keep the causal, ordered, standalone and interaction
P&L, mixed-model, history, credit and OIS checks, and the CTD tie and switch checks.
No type, schema, operation or wire change is needed. Not yet covered: full market,
model, managed-host, production, scale and latency testing, and authoritative
posting and execution.

## Concurrent principal and final coupon with returned cash shares

The cohort route accepts principal and final-coupon claims acquired independently at
the same time on first and successive custody children. Each claim is identified by
its cohort and obligation key, proposal, entitlement revision, positive legal
residual and ordered paid prefix. Several claims can share a recipient without
merging their legal ownership. Each transfer spends the sender's currently available
share of that residual. Returned shares keep the same proposal and identity.

Cash already paid stays with its recipients. A party may return all of its
outstanding share and keep its recorded cash, because a zero current ownership does
not erase a receipt. Principal and coupon claims with different prefixes, and claims
from different child origins, cannot be exchanged. Custody consumption, earlier
disposals of received stock and later nominal delivery receipts do not choose cash
recipients. Invoice ownership stays bound to the delivered asset. Maturity ends the
security's model exposure, while earned cash and nominal pending delivery and invoice
rights remain. Fresh custody or disposal still needs an Active security. Future cash
uses its collateral discount role and due date, and the overdue policy and reporting
FX stay explicit.

Decimal cash partitioning uses the finest common scale the total can represent.
Bounded integer long division truncates earlier allocations toward zero, and the
final nonzero legal holder receives the remainder. This keeps finer-scale partial
amounts from losing cash when the remainder is subtracted, including at the signed
96-bit/28-decimal boundaries. The mathematical share, the cash amount and the pricing
model are unchanged. No tolerance accepts a conservation error, and no monetary
rounding or external posting is inferred.

Run `cargo run --locked -p convex --example concurrent_claims_pnl`. Six signed books
exercise four concurrent principal and coupon claims on two child origins, with
delayed, prefix and staggered acquisition and 12 sequential or returned share
transfers per book. Twenty-two earlier books (maturity, child, pending and mixed
model, history, credit, OIS and CTD) remain. The 56 requests run 2645 product
revaluations and 28 typed failures. The financial checker chain runs once per
request. A checked Rust reconstruction covers every claim, the received, disposed
and outstanding face, and the invoice cash. Signed Decimal boundary controls show
exact cash conservation. Earlier custody and disposal have independently available
baseline experiments. Later maturity, cash and transfer experiments are explicitly
Unavailable and cannot borrow Time or Event history. Rust, C, MCP and WASM tests, a
strict schema, and separate C parity controls are included.

The independent writers keep the initial trials and the separately supplied invoice
ownership and baseline replay repairs. The financial declarations use Decimal60 for
the product, cash, curve, history and custody values before the Rust code runs.
Parity envelopes show transport only. Conditional, floating, credit, novation and
amendment claims, amortizing physical invoices and post-delivery amendments are
unsupported in this route. Not yet covered: managed and browser hosts, production,
scale and latency testing, and authoritative postings, authenticated cash, fills and
FX, limits and execution. None of this work has independent quantitative, model
or production certification.

## Cash settlement and closure

Run `cargo run --locked -p convex --example settled_claims_pnl` for six signed
delayed, prefix and staggered books, plus all 28 earlier positive books. The four
concurrent child principal and final-coupon claims settle after the returned-recipient
payments in their ordered Event. Their outstanding cash becomes exactly zero, while
nominal physical delivery can still be pending. Every earlier paid recipient and every
recipient with a zero current share keeps its cash. The legal share fractions remain
historical ownership proportions, and a zero outstanding amount cannot be freshly
acquired or transferred.

When the selected claims are fully paid, the bond state stays `Matured`. The final
Event records the nominal and invoice ingress and all remaining cash, including past
coupon claims that are explicitly excluded from current legal ownership. Only then
does the lifecycle become `Closed`. Receipts use the bounded Decimal cash and
outstanding amount, never a model PV. The lifecycle proposals, receipt prefixes, legal
sources, calendar, profile, security, delivery and invoice terms and precision are
kept. Later receipts of explicitly excluded cash are accepted after custody, and they
stay excluded from current cash and P&L ownership. Earlier paid history still needs
its dated prefix, and fresh custody or disposal still needs an Active security. No
posting, cash authentication, collection, order or ledger is supplied.

The independent controls replay Decimal operations at each 96-bit/28-place cash
boundary. They keep the mixed, history, credit, OIS, model and CTD regressions and
the financial bounds. Three input trials stay separately pinned, and the active
corpus is `settled-claims-pnl-complete.json`. Only absent optional event attachments
are omitted from the wire, and the full input is shown to be reconstructed exactly
before execution. There are sixteen ordered causes, and the wire limits are
unchanged. A later independent Position stays Unavailable where its own baseline
maturity, paid prefix, legal and custody prerequisites do not exist.

Java FFM and C parity has run on the installed macOS JDK. The wasm32 Node glue passes
the independent financial checks, but a strict comparison against the Rust output
shows floating-point differences in the model and OIS outputs. These checks are
distinct, and exact WASM transport parity is still open. Windows, .NET, Excel,
browser and other host and platform testing are separate. See
[remaining work](internal/CVX-063-REMAINING.md).
