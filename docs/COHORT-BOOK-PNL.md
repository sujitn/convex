# Cohort-owned cash and actual physical delivery reconciliation

`cohort_book_pnl` extends the pure causal reconciliation contract with explicit
original lots, cash entitlements and actual physical asset/invoice handoff. Use
`convex::library::execute`, shared JSON, `convex_library`, MCP or native WASM.
The existing `book_pnl` contract remains available for its original scope.

```sh
cargo run --locked -p convex --example cohort_pnl
cargo run --locked -p convex --example residual_pnl
cargo run --locked -p convex --example claim_pnl
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
| `cohorts[].disposal_fill` | Optional full actual disposal of a nonphysical acquired original lot. Physical stock uses exact `physical_disposals` facts. |
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

Broader legal amendments/novation, conditional claim transfers and further delivered asset models,
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

Boundary bond amendments and conserved lots are available through the same
`cohort_book_pnl` operation. Run `cargo run --locked -p convex --example lot_pnl`.
An actual `bond_amendment` lifecycle event supplies the prior and later legal
versions, immutable agreement and complete successor terms. The original opening
terms and event prefix remain. All due original steps/cash must be processed
before the event, at an original accrual start. Native admission preserves the
full schedule, actual calendars, historical profile values and principal cash,
current principal, fixing mechanics, rounding and quotation units. Only future
fixed-bond profiles may change. Earned obligations, proposal events and receipts
remain exact; later earned cash uses the full checked amended contract. The
point's security revision must match the admitted legal revision. Conditional,
floating, mid-period, calendar/schedule/index or retroactive amendments fail.

Optional `lot_splits` supplies the actual legal identity, parent, distinct child
cohorts, effective/known-at cutoffs, source revision and every applied point index.
These indices refer to baseline, ordered endpoints and available standalone
points in that order; index zero cannot apply a new interval split. A split
belongs to the `position` cause. Every later ordered point retains it; unrelated
standalone causes cannot acquire it. Children use `acquisition.kind=lot_split`
and the exact `split_id`. Their declared signed quantities must sum exactly to
the full original parent, in the same direction. A later partition must follow
the original child's acquisition, with no cycles or repeated consumption.

The supported security partition has the same full fixed, unrounded, nonoptional
bond, security revision, currency, reference face, entity and fixing history at
each actual or independent acquisition. Earned original cash and historical
receipts stay with the parent, including separately identified residual fractions;
children explicitly exclude all pre-split earned keys. Future earned keys belong
to the appropriate successors; the retired parent explicitly excludes them.
Each original native lot and all excluded claims remain visible. Held parent
face falls to zero, successor face is conserved, and no fill, consideration, fee,
average cost or cash receipt is inferred. Later full disposal of a successor
continues to use its actual original fill. The separate physical custody contract
below owns checked delivery partitions. Other rounded/conditional instruments
requires further complete owners and fails explicitly.

The original 128-holding/33-point/4,224-mark limits and wire bounds remain. Complete
current native terms, actual resolved coupon/principal dates and all original
amendment/proposal/receipt facts are bound inside the prepared path. Every full
mixed-book valuation, causal interaction,
original CTD winner/tie/switch and actual partial delivery control remains.
The active complete independent controls are `lot-pnl-chronology.json`, produced
by `tests/scripts/lot_pnl_chronology.py`. Initial reference, forward and typed
diagnostic declarations remain alongside it without replacing earlier trials.
The original complete residual/cohort/causal suites remain required separately.
CVX-063 remains partial; actual managed/browser hosts and production/latency
qualification remain separate.

Explicit earned-cash transfers use the additive `claim_transfers` list on this
same operation. Each record supplies a unique legal `id`, original `origin`
cohort and cash `key`, actual `proposal_id`, exact `entitlement_source` and
optional `residual_id`, current legal `from` and `to`, positive `fraction`,
original cash `cutoff`, immutable legal `source` and `applied_points`. Fractions
refer to the original entitled cash after its original residual fraction and
excluded paid prefix. They are independent of the receiving lot's signed face.

Only already-earned, available original claims can transfer. All parties must
be complete acquired legal cohorts with the original currency, cash discount
role and overdue convention. A split successor may receive an earned claim
without replaying its parent's historic payments or acquiring security again.
Legal transfers belong to the Position cause and persist on the ordered path;
available independent Position points carry only their actual available transfer
prefix. Partial, entire, sequential and returned ownership is conserved exactly.
Duplicate, foreign, unavailable, retroactive, exhausted and overdrawn facts fail.

The cutoff retains the complete ordered original receipt IDs for that claim.
Earlier cash stays with its recipient. Every later actual receipt is allocated
under the shares in force then; another transfer moves outstanding cash only.
Native `claims[].transfer_ownership` retains the source's allocation and
`transferred_claims` separately retains recipients' original cash/proposal,
entitlement/residual, legal transfer prefix, exact share and full actual receipt
allocations. Original obligations and receipt history remain visible. Recorded
cash plus outstanding cash forms each recipient's allocated proposed cash.
Checked Decimal partitioning conserves each original cash amount exactly; the
last nonzero legal share retains the original Decimal representation's remainder.
The original same-currency collateral ratio and point FX price every share.

All original product values, future embedded-cash removal, current/pending/fill
and physical receipt facts, full causal/standalone/interaction and unclamped
reconciliation residuals and every CTD winner/tie/switch survive. The new bounded
allocation admits128 legal transfers and4224 complete original-root/party/point
allocations, alongside the original holding, point, pricing and wire ceilings.
Margin and unearned/conditional claims retain their separate owners. Physical
invoice transfers use the complete original delivery owner described below.
No payment, consideration, fee, security fill or ledger posting is inferred.

The complete independent control is `claim-pnl-bound.json`, produced by
`tests/scripts/claim_pnl_bound.py`. Initial and earned declarations, failed
trials and separate complete plain-Decimal receipt facts remain. Run
`cargo run --locked -p convex --example claim_pnl` or the four actual
`--test claim_pnl` adapter tests. Native transport envelopes are not independent
financial references. Original monetary bounds and37943 regression comparisons
remain required. Actual managed/browser execution, release acceptance and
production/latency certificates remain separate; CVX-063 is partial.

Delivered custody uses the additive `physical_partitions` list. Each record has
`split` (the original asset parent, two or more full native children, unique legal
ID, source, economic/knowledge cutoff and applied points), the linked `future`,
exact original `delivery_id` and complete ordered security `receipt_ids` at that
cutoff. Children use `lot_split` acquisition with this exact split ID. Their fixed
signed native face sums exactly to the original delivered face. They retain the
same complete security revision, terms, resolved calendars, entity, currency,
units and history. The original actual delivery and every partial receipt remain
with the future; a model CTD choice never creates custody.

At the Position partition, the original asset's security custody ends and its
children acquire their exact allocated received and outstanding face. Custody
includes already received assets as well as the remaining security receivable.
Original earned cash stays with its declared owner. Every native security receipt
is allocated with checked Decimal arithmetic; received face is the exact sum of
those allocations, and outstanding face is proposed face minus received face.
The children conserve all three totals. `physical_delivery.partition` retains
the full legal facts, original total faces and each original receipt's allocated
face. The consumed parent's custody share is zero; its invoice ownership remains
independent. No fill, fee, payment, execution cost or consideration is created.

An earned `delivery_invoice` may also use `claim_transfers`. Its `origin` and
proposal/receipt pins name the original future; the linked original asset is the
initial legal cash owner. Transfer `from`/`to` names complete acquired cash cohorts,
with the same currency, collateral discount and overdue convention. Security
children acquire invoice cash only through explicit legal transfers. The same
original receipt cutoff rules preserve earlier paid recipients; transfers of
signed outstanding cash may be fractional, entire, successive or returned.
`transferred_claims` reports full original invoice/proposal/entitlement and receipt
allocations. `physical_delivery.invoice_excluded` means the full original invoice
is retained as provenance and its cash is included through those allocations.
The future retains invoice provenance without counting it as variation cash.

Partitions persist on the ordered path and apply only to available independent
Position experiments. Foreign/missing delivery, omitted/reordered receipt prefix,
unavailable knowledge, duplicate custody, incomplete/overdrawn face, mismatched
security and unsupported conditional custody facts fail explicitly. At most
128 holdings and partitions, 33 points and 4,224 full product marks remain; each
cutoff has at most4,096 receipts. Shares must be exactly representable under the
original Decimal face owner. Conditional delivery models require their complete
owners. The native preparation signatures
delegate with empty partitions; JSON omission defaults to empty. Rust
`CohortBookPnlInput` literals must supply `physical_partitions: Vec::new()` or a
complete list, and `physical_disposals: Vec::new()` or complete actual disposal facts.

Run `cargo run --locked -p convex --example physical_partition_pnl`. The active
independent corpus is `physical-partition-pnl-final.json`; all preceding
financial/wire/chronology trials remain immutable. Its nine full books include
long/short partial receipts, custody before/between/after ingress, distinct cash
recipients and entire invoice transfers, plus the original complete mixed cash
books and CTD switches. Thirty-five full malformed controls and direct native
conservation tests remain separate from transport parity. Actual managed/browser
hosts, broader delivery models and production/latency qualification remain open.

Successors may be partitioned again after their own actual acquisition. Each
partition binds the same original `future` and `delivery_id`; the parent must be
an acquired live custody cohort. Children conserve the parent's remaining signed
stock plus delivery receivable. Actual acquisition precedes later partitions in
the ordered path and every independently supplied information set includes its
complete available ancestry. Cycles, double acquisition or consumption, foreign
security revisions and missing ancestry fail explicitly. All original delivery,
notice, security, basket, CF, accrual, rounding and receipt events remain visible.
`physical_delivery.lineage` records the complete successive legal ancestry.

The additive `physical_disposals` list binds each actual stock disposal to:

- Unique legal `id`, acquired `asset`, original `future` and `delivery_id`.
- An existing actual `fill_id` with full original quantity, consideration and fees.
- Exact `effective_date` and `known_at` matching that fill, an immutable `source`,
  the complete original security `receipt_ids` cutoff, and `applied_points`.
- Nonempty `receipts`, each naming an original `receipt_id` and the exact signed
  received `face` disposed from this legal owner.

Receipt allocations must be distinct, already available and within this cohort's
remaining stock. Their signed sum must exactly oppose the actual fill quantity.
Disposal subtracts received stock; the remaining security receivable is unchanged.
It creates no fill, consideration, fee, payment or invoice ownership. Subsequent
partitions conserve the remaining custody after disposal. At every point,
remaining proposed face plus actual disposed face equals the original full
delivery; remaining received face plus disposed face equals original actual
receipts. `proposed_face = received_face + outstanding_face` still holds for each
legal owner. `partition.receipts` shows remaining owned receipt face, and
`physical_delivery.disposals` retains full legal facts, actual fills, original
checked receipt events and exact removed face with the disposing cohort.

For example, an original 200,000 face delivery is split into 80,000 and 120,000.
After 50,000 face arrives, the first child holds 20,000 with 60,000 still due. An
actual 10,000 stock sale leaves 10,000 received plus 60,000 due. Its later partition
into 17,500 and 52,500 allocates 2,500/7,500 received and 15,000/45,000 outstanding.
The disposing parent retains its actual sale cash and fees. Paid invoice recipients
and outstanding invoice shares remain governed by the original separate cash
facts, even after the entire received stock has been sold or custody consumed.

Up to 128 actual disposal facts and 4,096 receipt allocations per fact are admitted
within the existing holding/point/product limits. Historical receipts and cash,
native quantities, legal sources, independent information sets and exact Decimal
conservation remain checked before pricing. Actual disposal after consumption,
overdrawn stock, unmatched/competing fills, omitted receipt prefixes, unavailable
knowledge, changed history and unused facts produce typed failures.

Run `cargo run --locked -p convex --example custody_lineage_pnl`. The complete
independent controls are `custody-lineage-pnl-admitted.json`; all preceding
declarations and failures remain retained. The native entry point is
`prepare_cohort_pnl_with_disposals`; earlier signatures delegate with an empty
disposal list. JSON omission defaults empty. Native Rust/C/MCP/WASM tests,
independent full model controls and transport parity are distinct evidence;
managed/browser execution and production/latency certificates remain open.

## Dated earned coupon and principal ownership

The same `cohort_book_pnl` input accepts optional `dated_entitlements` (default
empty). Each `CohortDatedEntitlement` names the complete native `origin` and
obligation `key`, original `proposal_id`, exact `entitlement_source`, an existing
`CohortResidualClaim` (`id`, positive `fraction <= 1`, and `cutoff`), the actual
legal `source` revision and complete `applied_points`. Its original cohort
entitlement must be owned and have no competing legacy residual. This applies
only to an already earned nonconditional fixed-bond coupon or principal, with
complete native terms, calendars, profiles and replayed boundaries. Floating,
optional and credit-dependent cash need their separately qualified claim owners.

Cash acquisition is independent of security acquisition or disposal. Before its
explicit Position point, the dated cash is excluded from this book; afterward,
the supplied fraction owns the original residual after the exact prior receipt
prefix. Every earlier payment remains visible and excluded, with its original
identity, amount, economic date, knowledge cutoff and proposal. A security split
may retire the source's custody while that source retains its explicitly supplied
later earned cash. A later actual security disposal does not move earned cash.
The supplied fraction is never derived from held face, outstanding delivery face,
a fill timestamp, a price or a synthetic payment. Excluded shares and prior cash
are explicit scope exclusions, not invented recipients.

For example, an original native coupon of100 with10 paid before cash acquisition
and a legal fraction0.6 gives owned residual54. An actual subsequent payment20
gives owned received12 and outstanding42. A later explicit claim transfer of0.25
moves10.5 of that outstanding residual; the previously received12 stays with its
recipient. Subsequent original payments follow the then-current explicit legal
shares. Use `residual_id` equal to the dated residual ID in each
`claim_transfers` fact; all original proposal, source and paid-prefix pins remain
mandatory. Receipt and outstanding allocations conserve exact native Decimal
cash, including the existing final representational remainder owner.

An actual dated entitlement starts on a Position cause and persists at every
subsequent ordered point. Independent Position experiments need the complete
available original earned proposal, acquired cash party and receipt prefix;
unrelated experiments cannot acquire it. Duplicate/foreign facts, source or
proposal revisions, fractions outside(0,1], unavailable/retroactive cutoffs,
missing or changing paid prefixes, competing residuals and exhausted claims fail
before valuation. Bounds remain128 facts,33 point indices,4096 receipt IDs and
the original4224 full product evaluations; wire resource bounds are unchanged.

Existing received-security custody and actual disposal provenance survives
original contractual coupon and bullet redemption boundaries. Separately held
original native cash-bond cohorts also support contractual amortization. The
original physical delivery invoice still requires a fixed nominal bullet;
pre-amortizing deliverables fail with its existing Unsupported outcome. Current quantities remain the
original face units used by native lifecycle; the original full profile and
ended status determine remaining economic exposure. Maturity ends model exposure
and retains separately owned coupon/principal receivables. New custody partitions
or actual disposals require active security at their actual transition. Invoice
cash, delivery/notice/security/basket/CF/accrual/rounding and every original
partial receipt remain independent and unchanged.

Run `cargo run --locked -p convex --example dated_cash_pnl`. Additive native
`prepare_cohort_pnl_with_dated_entitlements` accepts the separate dated facts; all
previous preparation signatures delegate with an empty vector. JSON callers may
omit the field; Rust `CohortBookPnlInput` literals add `dated_entitlements`. No
operation, wire version or C export changes. The full original financial checkers
and all CTD ties/switches remain required.

The scoped controls cover signed coupon, amortization and redemption, successive
custody, actual partial security receipt provenance, pre/post-entitlement disposal,
prior and subsequent real cash receipts, independent custody Position sets and
separate invoice recipients. Broader conditional/novation/amendment claims, fresh
custody after maturity, delivery options/margin financing, actual managed/browser
host execution and full scale/latency/production certificates remain open.
Authoritative cash/fill/FX postings, limits, reservations, fencing and execution
belong to CVX-025/031; this pure input API creates none of those authorities.

## Cash rights while physical delivery is pending

An explicit dated entitlement can own an original earned coupon or bullet
principal while the original delivery has no security receipts or only partial
receipts. The full proposed security face remains visible as received face plus
the outstanding delivery receivable. Cash rights use their own supplied legal
fraction and paid-prefix cutoff. Later receipt ingress, successive custody
partitions and actual disposal of received stock do not move that cash right or
the original invoice. An actual disposal can consume received allocations only.

Independent Position experiments supply a complete information set and include
its point index in the dated entitlement's `applied_points`. The original party,
earned proposal, legal source, economic/knowledge cutoff and every excluded
prior receipt must already be available in that independent set. The ordered
path's later receipts or legal prerequisites cannot be borrowed into it. Before
the acquisition, the claim is excluded; both the actual acquisition and an
independently available acquisition apply the same explicitly supplied residual.

For earned cash with a future payment date, the original full pricing curve and
spread remove embedded cash from held and receivable model exposure. Separately
owned cash uses its supplied collateral discount curve's payment-date ratio.
Actual payments require the native due-date check. A Time roll precedes cash
received on that later payment date; new ingress retains its actual economic and
knowledge clocks. Overdue cash requires the existing explicit policy. Cash and
security receipt histories remain immutable ordered prefixes, and a later legal
cash transfer preserves already paid recipients.

Bullet maturity ends the original native security model exposure. Its remaining
delivery face and subsequent actual security receipts stay visible, with
separately supplied coupon/principal and original invoice ownership. No post-
maturity partition or disposal is admitted. Pending redemption does not imply
a market price or a new cash right. Amortizing delivery invoices and post-delivery
amendments remain unsupported by this scoped qualification.

Run `cargo run --locked -p convex --example pending_cash_pnl`. The full independent
controls cover long/short acquisition before and after partial security ingress,
independent cash Position experiments, delayed coupons with distinct discount
roles, successive custody and disposal, outstanding delivery through bullet
maturity, real cash receipts and full mixed-book models/CTD switches. This uses
the existing native preparation and JSON operation without an API migration.
Managed/browser execution and production/latency certificates remain open.
## Earned cash on physical custody children

An acquired custody child can own an original fixed-bond coupon or principal
claim through an explicit `dated_entitlements` record. Supply its complete native
earned proposal, exact entitlement source, legal residual fraction and ordered
receipt cutoff. Earlier actual receipts remain excluded with their original
recipients, including receipts supplied in the child's historical replay before
security custody. Native admission verifies that the child is acquired at every
applied cash point. Security face never establishes a cash right.

An earned-claim transfer uses the same original proposal and residual identity.
Later receipts are allocated to the legal shares effective when each receipt
occurred. Consumed custody parents and children with an actual received-stock
disposal retain their separately owned cash. The delivery invoice and pending
security receivable keep their original owners and provenance.

An available independent Position experiment must supply its own complete
baseline proposal, paid prefix, custody facts and legal parties. It cannot import
later actual payments or other Event changes. Declare it unavailable when those
prerequisites cannot be supplied independently. Delayed earned cash retains the
distinct original pricing discount/spread removal and collateral cash discount,
payment due-date, overdue and FX rules.

Run `cargo run --locked -p convex --example child_cash_pnl`. The full controls
cover signed first and successive custody children, excluded historical receipts,
independent cash acquisition and transfers, subsequent actual cash, custody
consumption/disposal and pending delivery. They also retain all preceding coupon,
bullet principal and complete causal mixed-model/CTD books. Native amortizing
physical invoices, conditional/floating/credit cash and post-delivery amendments
still require their separate complete owners. This increment changes admission
of existing fields; no type, schema, operation or C ABI migration is required.

