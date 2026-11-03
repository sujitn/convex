# Contract lifecycle

`convex::lifecycle` (also `convex_analytics::lifecycle`) tracks a single trade
through its events: coupons, fixings, amortization, exercise, termination,
default and futures settlement. It needs no server, database, broker, provider or
clock. It works on contracts you pass in and on observations you record.

"Checked" below means the function validates its input and returns an error
instead of guessing. The standalone example and its checks are:

```sh
cargo run --locked -p convex --example lifecycle
cargo test --locked -p convex-analytics -p convex --test lifecycle
python3 tests/scripts/lifecycle_reference.py check
```

## State and events

Start by building the full contract: `BondContract`, `SwapContract`,
`CdsContract`, a dated `FuturesContract`, or a `ZeroCouponBond`. Pass it to
`LifecycleState::new` together with the trade ID and version, a signed
`LifecyclePosition`, the known-at cutoff for the opening state and the actual
fixing history.

- Swap and CDS IDs and versions must match the contract exactly.
- Bonds also receive their internal trade identity.
- Zero-coupon and bill input is rechecked for positive, bounded face and
  redemption and for the issue and maturity dates, because the older convenience
  constructors cannot fail.
- All calendar, reference, curve and exercise terms stay reachable through
  `instrument()`.

The state starts at inception, before the first economic effect. A seasoned
state is built by replaying actual events. A futures contract takes an explicit
opening trade or cleared mark instead of an invented zero level. Opening history
must be available at the cutoff and must belong to the contract's index. A
computed state cannot be deserialized without its checks. To recover or restate a
state, the application keeps the full contract terms, calendar bindings and
events.

`transition(&previous, &event, &inputs)` returns the event, a new checked state
and the separate effects. The previous state is never modified, whether the call
succeeds or fails. The call checks the event ID, the trade, the expected prior
transition count, that the effective date does not decrease, and the known-at
cutoff. Skipping a contractual boundary that has not been processed fails.

`view()` exposes pending boundaries, notices, local event and receipt IDs, units,
status, fixings, obligations and the clearing mark. Ordering within a day is
explicit, and the bond's pre- or post-principal coupon convention determines the
cash. Lifecycle does not regenerate simplified schedules.

Normal state, effect, history, receipt and event work is limited to 65,536
records. The schedule, profile and history limits of the underlying contracts
also apply, as do 128 members and 1,024 sources for futures. Decimal operations
and revision increments are checked. If a limit is reached, no partial
transition is accepted. The implementation allocates memory and has not been
measured for colocation or tail latency.

## Cash and position meaning

`ContractStep` applies dated changes to coupon, rate, spread, gearing and
notional. Bond amortization and redemption create cash obligations at the
**effective** principal boundary, and the delayed cash date is kept. Swap notional
steps do not invent a principal exchange.

`ContractualCash` records earned bond, IRS and OIS coupons at their accrual end,
including delayed payment. The `completed_cashflows` selectors reuse the
contract's amount, rounding and observation math. They require published rates
for the selected coupons. Future coupons are not selected, so no forecasts are
needed. `due_cashflows` gives the cash due on a date.

For a CDS:

- The upfront amount is recorded at trade inception, and any remainder is due on
  the original cash settlement date.
- Premium is recorded at its scheduled survival boundary.
- Cash rounding must be set explicitly, because the pricing precision is not an
  accounting convention.
- On default, every premium already earned is kept.

An obligation is a proposed receivable or payable, never a recorded payment. Its
amount and due date survive partial receipts. A `Payment` consumes an external
receipt ID, a source, the obligation key, signed `Money` and a value date.
Currency and direction must agree. Early cash, overpayment and a receipt ID
already used locally all fail. `settled` is explicit: even a zero-cash claim
needs a completion record. The application owns deduplication, posting,
value-date corrections, reversal, restatement and reconciliation.

Position scaling depends on the product:

- Bonds use signed currency face relative to the original face.
- CDS uses signed notional relative to the original notional and the protection
  side.
- Swaps use copies of the original signed trade economics.
- Futures use integer contracts.

Amounts rounded by the contract are scaled once. A partial swap termination
applies a residual copy fraction. The cash precision used for ledger posting is
an application policy. A maturity, exercise or default can end future exposure
while cash stays pending. There is no cash total without an explicit currency and
ownership policy.

## Exercise, termination and default

A call or put `ExerciseNotice` validates the exercise style and provision, the
protection period, the notice deadline and the separately bound exercise
calendar. `Exercise` requires:

- That notice.
- The permitted date.
- The redemption price per 100 of outstanding principal.
- The settlement lag and adjustment.
- The PayAccrued or Included convention.

Earned coupons and principal remain separate receivables. Partial-exercise
accrual is not added twice at a coupon boundary.

`exercise_accrued` uses only the earned term or overnight prefix. Shifted
overnight exercise needs an explicit weight cutoff. The shared `prefix` keeps
the lookback and lockout observations instead of generating a shorter schedule
with different dates. Due history cannot be replaced by a forecast. These events
record an exercise that already happened. No lifecycle function chooses to
exercise, values an option or relaxes a model or risk check.

`Terminate` requires an agreement revision, explicit signed cash (zero is
allowed), a cash date and a fraction. Full termination cancels future contractual
exposure and keeps the arrears already recorded. The agreement cash covers the
partial-period and termination economics you supply. Partial termination is
supported at a boundary shared by the IRS or OIS accrual periods. A mid-period
residual needs complete amended terms and a replay, and fails as Unsupported.
These are not supported, and no zero effect is assumed for them:

- Partial bond termination.
- Notice cancellation.
- Novation or a successor.
- Accelerated coupon recovery conventions.

`CreditDefault` validates the entity, seniority, document, protection dates, the
contractual elapsed fraction of the default day, and cash rounding. The actual
day fraction is separate from the standard pricing model's expected half-day
integration. Single-name CDS protection terminates, and the earned default
premium is kept separately. Bond recovery requires an explicit legal mapping and
applies to outstanding principal only. Principal and coupon claims that have
already matured are unchanged, and no accelerated future coupon recovery is
inferred.

`DefaultPending` has **no auction recovery amount and no assumed settlement
date**. `Recovery` needs the default ID, the fractional result and its source
revision, and a bounded contractual cash date. It creates signed protection or
bond principal recovery, plus separate CDS default-accrual obligations, and the
status becomes `DefaultCashPending`. Actual receipts are still required to close.
Zero or full recovery is valid data and does not imply that settlement happened.
Vanilla pricing of a surviving contract cannot value this post-default state. An
application must price the remaining receivables explicitly and keep the
recovery unknown until it is known.

## Futures

Official daily and final rate marks go through `futures_settlement_effect`: round
each per-contract monetary level, subtract, then scale by integer contracts. The
state advances the cleared level once and keeps each margin obligation separate,
so a cumulative mark is never booked again as cash. The contract, calendar and
source checks, the trade tick versus settlement precision, the final date and
rate lattice, and the report checks all remain. Final settlement of a rate future
ends exposure and keeps margin pending.

Physical bond delivery requires:

- A selected security and its notice.
- The full basket and conversion factors.
- The delivery-calendar binding.
- The official last cleared invoice points.

The selected fixed-bullet bond's accrued interest and `bond_delivery_invoice`
give the rounded per-contract cash. The following are separate valuation work:
cheapest-to-deliver (CTD) choice, repo and carry, margin financing and the value
of the delivery option. Delivery after the last trading day uses the last cleared
official mark. Unavailable, expired or mismatched delivery data fails.
Historical or synthetic data quality stays visible through the stamps you supply
and does not establish execution eligibility.

The library proposes signed security face and invoice cash. The cash receipts and
an exact `SecurityReceipt` are separate, and both must arrive to close. Partial
receipts of the same security conserve signed face cumulatively, and
`delivery_pending` holds the remaining face. Every checked receipt is available
through `security_receipt_events()`, and cash stays independent. Several delivery
securities need distinct positions and full legal invoice owners. An approximate
asset cannot stand in for the actual security.

## Evidence and remaining scope

`tests/fixtures/lifecycle/inputs.json` declares synthetic economics and
calendar and rounding assumptions. `lifecycle_reference.py` uses Decimal60 and
explicit dates and weights. It never runs Convex or reads its outputs. Cash that
the contract rounds is compared **exactly**, with no widened floating tolerance.
Rust boundary and error tests, three feature consumers, strict lints and docs, and
comparisons against the retained QuantLib and ISDA captures add to that evidence.

Existing numerical policies, captures, schemas and the 25 C exports are
unchanged. Lifecycle event JSON does not add generated shared schemas or
FFI, Java, Excel, MCP or WASM dispatch. Not yet covered: ledger, outbox and
recovery, P&L, market and convention validation, and independent validation of
the quantitative models.

Fixed-bond boundary amendments keep the opening replay and earned cash. Pass the
complete successor contract, with the same calendars, in
`LifecycleInputs.amended_bond`; the shared API builds it from the event terms and
calendar revisions. The state keeps every amendment and its legal version. See
[cohort reconciliation](COHORT-BOOK-PNL.md) for input validation, ownership,
supported profile changes and the cases that fail.
