# Completing CVX-063

CVX-063 remains open. Its three acceptance criteria require lossless full terms,
every mandatory native/JSON/C operation, and actual parity or explicit unsupported
capability for advertised Java/Excel/MCP/WASM inputs and results. Existing shared
routes, constrained mixed hedges, complete full-term curves, lifecycle and cash
reconciliation are engineering subsets of the original required library scope.

From the repository root:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tools/library/outstanding.py
PYTHONDONTWRITEBYTECODE=1 python3 tools/library/outstanding.py --json
PYTHONDONTWRITEBYTECODE=1 python3 tools/library/outstanding.py --json --area LC-008
cargo run --locked -p convex --example settled_claims_pnl
```

The report retains all30 areas,174 product/convention/model combinations and2154
original mandatory operation/surface records. At this review,80 exact engineering
routes explicitly map192 records;1962 records lack such mappings. This does not
mean1962 native implementations are absent: many already have full family/native
examples. Each still needs exact operation-level evidence for its original scope.
No record is removed, marked Verified, or certified by this report.

| Remaining work | Concrete completion requirement |
|---|---|
| Exact operations and roundtrip | Bind each original record to actual complete native/JSON/C positive, boundary and typed negative calls. Retain original economics, calendar/revision/history/model/quantity units, curve roles, native residuals and work bounds. Rust-only primitives need their original Rust profiles; do not invent unnecessary JSON operations. |
| Optional models and quotation conventions | Complete required American and make-whole notice/exercise/redemption, joint floating/exercise and cap/floor/collar economics, required retained model compatibility, and applicable long-bill/Street/ex-coupon/workout conventions. Independent answers precede candidates; prices, inverses, diagnostics and h/h2 risk need separate convergence evidence. Existing European/Bermudan and supported term-FRN models remain the original owners. |
| Claims and physical lifecycle | Complete floating/conditional/credit/recovery/novation/amendment claim ownership, independent mature-baseline Position prerequisites, amortizing physical invoices and post-delivery amendments. Keep original proposals, historical legal/paid recipients, principal factors, actual receipt amounts and original invoice convention; no synthetic cash price or model-PV payment. |
| Hedge qualification | Retain the existing pure constrained cash/CDS/IRS/futures solver, full original signed factor columns, recovery and CTD switches. Complete original convention/model/surface packs and lot/liquidity/cost/position/infeasibility controls. Authenticated fills/orders/limits/reservations/fencing/execution remain CVX-025/031. |
| Advertised hosts and conveniences | Inventory every retained entry point and actual units. Execute fresh-native/C comparisons on real Java FFM/JNI, Windows .NET/PInvoke/Excel worksheets, wasm32/browser and MCP handlers. Remove incorrect routes with their callers/types/advertisement and document the real replacement; explicit unsupported capability does not waive required native scope. |
| Independent certificates | CVX-064/065 own full convention/model/market/library admission using original packs, independent reference provenance and unchanged tolerances. Engineering tests and agent review cannot supply independent approval. Production/distribution/product-specific latency remain separate measured release gates. |

The latest settlement controls pay all four concurrent original claims and then
all original obligations, preserving28 full preceding financial books. Selected
cash exhaustion alone leaves native Matured. Full original cash and security
settlement allows Closed. An exhausted residual cannot be newly acquired or
transferred, and a zero current share retains earlier paid cash. The original
wire/schema/routes and exact Decimal helper remain; the only production change
admits later actual receipts for explicitly excluded original child cash after
custody, without acquiring P&L ownership. Earlier paid-prefix guards remain.

The preceding actual current-host Java/C qualification executes all4975 original
complete requests. New Treasury bill host evidence is qualified separately. Actual wasm32
Node outputs pass45 complete original independent financial checks with unchanged
bounds. Strict native-envelope comparison fails on floating-point model/OIS
intermediates; it remains a separate open gate. Native-compiled WASM, actual wasm32
Node and actual browser execution must be reported separately. Supplied Windows/
Excel or browser CI declarations are not executed host evidence.

Use one scoped implementation commit per completed owner. Preserve every old
numerical row/file/default and closed physical evidence. Freeze public source and
metadata before inspectors run; keep failures with separate source-frozen repairs.
After all builders finish, bind actual final native/C/host binaries, commit public
implementation locally, then its private evidence and parent submodule pointer.
Continue on `codex/production-readiness`; no push/merge/deployment/live orders.
The private coding-harness packet retains exact record IDs, acceptance tests and
owner tasks for all remaining work.


The Treasury bill increment adds explicit issue-based365/366 short/long quotation,
price/bank-discount/investment inverses and separately labeled analytic quote
sensitivities with independently declared h/h2 controls. Complete original zero
terms and calendar metadata remain. The old short-bill route keeps its guard.
See [Treasury bill quotations](TREASURY-BILL-QUOTES.md). February29 issue anniversary,
non-US bill packs, full market/convention admission and all other pending scope
remain required; additive engineering route mappings never verify Planned records.

The checked date-free bill helper now rejects long terms and arithmetic overflow.
Two unused floating-point bill exports are removed with migration to original
dated Treasury inputs. Existing shared short-bill values/units and all80routes
remain. The remaining infallible ShortDateCalculator/ZeroCouponBond bill methods
still need a separately declared checked migration. See
[the route migration guide](ROUTE-MIGRATION.md); this cleanup does not complete
the original convention/model/host requirements or award independent approval.
