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
routes and12 exact native routes explicitly map228 records;1926 records lack
such mappings. This does not mean1926 native implementations are absent: many already have full family/native
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

Actual native/C/JDK26FFM/wasm32Node/browser parity now executes complete response
envelopes, including floating-point model/OIS/CTD diagnostics. The committed
directional-parameter snapshot compares5686complete requests and400additional
JavaTreasury controls; its original reference answers and tolerances remain.
Each later source-bound packet records its own complete request count. Native-
compiled WASM, actual wasm32Node and an actual browser remain distinct checks.
Windows .NET/PInvoke/Excel execution remains unavailable on this machine and
explicitly unvalidated. CI declarations do not substitute for actual execution.
The user has no independent approved convention/model packs; CVX064/065 retain
independent approval and no agent-issued certificate is implied.

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
remain. The ShortDateCalculator/ZeroCouponBond bill methods now return checked
results over one Decimal owner; the remaining floating-point discount exports
are removed. Broader conventions, models, claims and host gates still need their
original qualification. See
[the route migration guide](ROUTE-MIGRATION.md); this cleanup does not complete
the original convention/model/host requirements or award independent approval.

The direct zero-coupon pricing/yield/continuous-rate and compounding APIs now
return checked Results, retain contractual redemption cash and reject invalid
dates/domains/arithmetic rather than inventing zero or face outputs. Run
`cargo run --locked -p convex --example zero_methods`. This completes the scoped
infallible-method migration; full original operation evidence, conventions,
models, claims, hedges and available host qualifications still remain required.
Unexecutable platform checks stay explicitly deferred under the user's latest
instruction, with their original required economics and completion criteria.

Direct zero-coupon terms now preserve exact Decimal strings and reject unknown
day-count substitutions or ignored fields. The common identifier container also
rejects unknown keys. This closes the scoped raw-term losslessness gap; it does
not complete the remaining original operations, models or host requirements.

The portable numerical owner replaces platform-dependent primitives with the
existing locked pure Rust kernels, including the existing linear-algebra library's
explicit portable feature. It retains every original financial operator, parameter,
reference answer and bound. The full WASM census runner compares every current
native field, including calibration diagnostics and mixed-book CTD/OIS/CDS output.
See [portable numerical qualification](PORTABLE-NUMERICS.md). The earlier failed
host captures remain evidence of the pre-repair behavior; fresh source-bound
qualification is required for the replacement.

Thirty-six stale hedge assessments now identify the implemented pure constrained
solver as a candidate. All sixty original supplied hedge operation records already
have exact aggregate/solve mappings. Their certificate status remains Planned;
authenticated application limits, fills, reservations and orders remain with
CVX-025/031.

The current term-coupon optional owner prices full segmented normal/shifted-Black
and joint whole-coupon Gaussian laws. Exact curve-state admission, central and
explicit directional parameter derivatives, selected-bracket price inversion,
canonical central risk and complete graph/parameter hedge columns retain their
original native precision and history/curve/source guards. Signed directional
risk has a dedicated full-native proof and separate first/second position policies.
Directional hedge/aggregation integration, full quote/surface identification,
whole shifted/overnight/path/rounding/intraday models and mixed-book parameter
scenario/lifecycle integration remain genuine required work. Remaining infallible
yield/YAS conveniences need checked genuine replacements with preserved financial
cases, answers and bounds. Use the actual outstanding report and the latest
source-bound evidence for current engineering scope.
