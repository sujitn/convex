# Supplied pure lifecycle

CVX-062 exposes `convex::lifecycle` / `convex_analytics::lifecycle` without a
server, database, broker, provider or clock. It composes actual checked native
contracts and exact recorded observations. The standalone synthetic example is:

```sh
cargo run --locked -p convex --example lifecycle
cargo test --locked -p convex-analytics -p convex --test lifecycle
python3 tests/scripts/lifecycle_reference.py check
```

## State and event contract

Build full original `BondContract`, `SwapContract`, `CdsContract`, dated
`FuturesContract`, or explicit native `ZeroCouponBond` first. Pass the original
trade ID/version, signed `LifecyclePosition`, supplied opening known-at cutoff
and actual fixing history to `LifecycleState::new`. Native swap/CDS IDs/versions
must match exactly; bonds additionally receive their internal trade identity.
Zero/bill ingress rechecks positive bounded face/redemption and actual issue/
maturity because older convenience constructors are infallible. All original
calendar/reference/curve/exercise terms remain accessible through `instrument()`.

The state starts at original inception before its first economic effect;
seasoned states are replayed from actual events. Futures have an explicit
opening trade/cleared mark instead of an invented zero level. Opening history
must be available at the supplied cutoff and belong to the exact product index.
Computed checked state cannot be deserialized unchecked. The application retains
full original terms/calendar bindings and events to recover or restate it.

`transition(&previous, &event, &inputs)` returns the exact original event, a new
checked state and separate effects. The previous state is immutable on success
and failure. Event ID, trade, expected prior transition count, nondecreasing
effective date and known-at cutoff are checked. Skipping an unprocessed original
contractual boundary fails. `view()` exposes pending boundaries, notices, local
event/receipt IDs, original units, status, fixings, obligations and clearing mark.
Same-day ordering is explicit; the native original pre/post-principal coupon
convention determines cash. Lifecycle does not regenerate simplified schedules.

Normal state/effect/history/receipt/event work is bounded to 65,536 records;
original native schedule/profile/history and futures128-member/1,024-source
bounds also apply. Decimal operations and revision increments are checked.
An exhausted bound returns no partial accepted transition. This allocating
control-path implementation has no colocation or tail-latency certificate.

## Cash and position meaning

`ContractStep` applies actual dated coupon/rate/spread/gearing/notional changes.
Bond amortization and redemption create cash obligations at the **effective**
principal boundary while retaining the actual delayed cash date. Swap notional
steps have no invented principal exchange.

`ContractualCash` crystallizes earned bond/IRS/OIS coupons on their original
accrual end, including delayed payment. Native `completed_cashflows` selectors
reuse original amount/rounding/observation math and require actual eligible
published rates for selected coupons. Future coupons are not selected and need
no fabricated forecasts. Native `due_cashflows` also supplies exact due-date cash.
CDS original upfront crystallizes at trade inception, remaining due on original
cash settlement; premium crystallizes at its original scheduled survival boundary.
Explicit CDS cash rounding is required because native pricing precision is not
an accounting convention. Actual default retains every already earned premium.

An obligation is a proposed receivable/payable, never recorded payment. Its
original amount and due date survive partial receipts. `Payment` consumes an
explicit external receipt ID, source, exact original key, signed Money and value
date. Currency/direction must agree, early cash and overpayment fail, and locally
repeated receipts fail. `settled` is explicit: even zero cash claims need a
supplied completion record. The application owns authoritative deduplication,
posting, value-date corrections, reversal/restatement and reconciliation.

Bonds use signed currency face relative to original face, CDS signed notional
relative to original notional and protection side, swaps copies of original
signed trade economics, and futures integer contracts. Native contract-rounded
amounts are scaled once. Partial swap termination applies a residual copy
fraction; cash precision for actual ledger posting remains an application policy.
A maturity/exercise/default can end future instrument exposure while cash stays
pending. There is no cash total without an explicit currency/ownership policy.

## Exercise, termination and default

Call/put `ExerciseNotice` validates the exact original provision/style, protection,
notice deadline and separately bound exercise calendar. `Exercise` requires that
actual notice, original permitted date, original redemption price per100 of
outstanding principal, settlement lag/adjustment and PayAccrued versus Included
convention. Earned completed coupons and principal remain separate receivables;
partial exercise accrual is not added twice at a coupon boundary.

Native `exercise_accrued` consumes only the earned term/overnight prefix. Shifted
overnight exercise needs an explicit original weight cutoff. Shared `prefix`
retains the original lookback/lockout observations instead of generating a shorter
schedule with different dates. Due eligible history cannot be replaced by a
forecast. These events describe actual supplied exercise; no lifecycle function
chooses exercise, values an option or relaxes a native model/risk guard.

`Terminate` requires an actual agreement revision, explicit signed cash including
zero, cash date and fraction. Full termination cancels future contractual exposure
and retains crystallized arrears. Agreement cash covers the explicitly supplied
partial-period/termination economics. Partial termination is supported at a joint
IRS/OIS accrual boundary; mid-period residual economics need complete amended
terms/replay and fail Unsupported. Partial bond termination, notice cancellation,
novation/successor and accelerated/coupon recovery conventions remain explicit
extension work, not assumed zero effects.

`CreditDefault` validates actual entity, seniority, document, protection dates,
contractual elapsed fraction of the default day and cash rounding. Actual day
fraction is separate from the standard pricing model's expected half-day
integration. Single-name CDS protection terminates and earned default premium
remains separate. Bond recovery requires an explicit legal mapping and the
scoped outstanding-principal-only assumption. Already matured principal/coupon
claims remain intact; no accelerated future coupon recovery is inferred.

`DefaultPending` has **no auction recovery amount or assumed settlement date**.
`Recovery` needs the exact original default ID, actual fractional result/source
revision and bounded contractual cash date. It creates signed protection or bond
principal recovery and separate CDS default-accrual obligations; status becomes
`DefaultCashPending`. Actual receipts are still required to close. Zero/full
recovery is legitimate data and does not imply that settlement happened. Vanilla
surviving-contract pricing cannot value this post-default state; an application
must price the explicit remaining receivables and preserve unknown recovery.

## Futures

Official daily/final rate marks delegate to original `futures_settlement_effect`:
round each per-contract monetary level, subtract, then scale integer contracts.
State advances the cleared level once and retains each separate margin obligation;
no cumulative mark is booked again as cash. Native exact contract/calendar/source,
trade tick versus settlement precision, final date/rate lattice and report guards
remain. Rate final settlement ends exposure and keeps margin pending.

Physical bond delivery requires an actual selected security/notice, complete
exact basket/CF, original delivery-calendar binding and official last cleared
invoice points. Selected native fixed-bullet accrued and original
`bond_delivery_invoice` calculate actual rounded per-contract cash; CTD choice,
repo/carry, margin financing and delivery option value are separate valuation
work. Delivery after last trading uses the actual last cleared official mark.
Unavailable/expired/mismatched delivery data fails; historical/synthetic quality
remains visible through exact supplied stamps and does not establish execution
eligibility. The library proposes signed security face and invoice cash. Cash
receipts and an exact `SecurityReceipt` are separate; both must arrive to close.
Partial receipts of this same actual security conserve signed face cumulatively;
`delivery_pending` contains the exact remaining face. Every checked receipt is
available through `security_receipt_events()`, and cash remains independent.
Multiple actual delivery securities require distinct original positions and full
legal invoice owners. No approximate asset substitutes for an original security.

## Evidence and remaining scope

`tests/fixtures/lifecycle/inputs.json` declares synthetic full economics and
calendar/rounding assumptions. `lifecycle_reference.py` independently uses
Decimal60 and explicit original dates/weights; it never executes Convex or reads
candidate outputs. Contract-rounded cash compares **exactly**, without widened
floating tolerances. Native boundary/error tests, three feature consumers,
strict lints/docs and current native comparisons against unchanged retained
QuantLib/ISDA captures supplement that engineering evidence.

All prior numerical policies, captures, mandatory coverage records, schemas and
25 C exports remain protected. Native event JSON does not add generated shared
schemas or FFI/Java/Excel/MCP/WASM dispatch; CVX-063 owns that integration.
Authoritative ledger/outbox/recovery, P&L, data/model/market/convention qualification
and independent quantitative/library/production/multiple-server/latency
certificates remain separate. No passing subset awards those certificates.
