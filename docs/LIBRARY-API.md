# Supplied-input library API (CVX-063)

`convex::library` exposes checked native analytics through one closed request
contract. JSON, C, Java, Excel, MCP and WASM adapters call the same dispatcher.
The dispatcher reconstructs full native contracts and calls their owning product
modules. It contains no database, provider lookup, MQ, clock, order submission or
ledger update. The typed Rust product APIs remain available for prepared, repeated
calculations and application-specific composition.

**Acceptance is partial.** The new adapters and engineering corpus do not make
Convex function complete. Every original required LB0/LB1 operation stays in the
[capability report](../tools/library/api-capabilities.json). Unsupported native
models and operations remain required work; none have been waived. AC2 cannot be
accepted until each mandatory operation has real dispatch and its own positive,
boundary and failure examples. LB0/LB1 and production gates remain separate.

## Start and inspect a request

```sh
cargo run --locked -p convex --example library_workflow
cargo run --locked -p convex --example library_workflow --no-default-features
cargo run --locked -p convex --example library_workflow --features schemars
cargo run --locked -p convex --example library_workflow --features schemars -- --emit-schema
python3 tools/library/api.py
```

The example executes 110 complete requests: 96 admitted operations and 14 native
financial failures. [The input corpus](../tests/fixtures/library/cases.json)
contains original terms and explicit resources; it contains no financial expected
outputs. Select a request by its `id` and pass its `request` object as JSON text.
For example, `bond.construct`, `calibration.mixed_rates`, `credit.cs01`,
`optional.quote_risk`, `future.roll` and `lifecycle.zero` demonstrate the shapes.
Use the exact IDs listed in the corpus when scripting a consumer.

```rust,ignore
let request = convex::library::decode_request(request_json)?;
let value = convex::library::execute(&request)?;
let envelope_json = convex::library::execute_json(request_json);
```

`decode_request` performs strict, bounded decoding. `execute` performs native
construction, market admission and financial validation. Serializing a complete
typed request roundtrips its economic data. Do not replace checked construction
with deserialization of cached native objects or lifecycle state.

## Wire contract

A request has exactly `schema_version` (currently 1), `inputs` and `operation`.
`operation.kind` selects one of the closed operations below. Native instrument,
curve, market and event schema revisions remain independent of the library wire
version. An unsupported revision is an error, never a guessed migration.

`inputs` supplies these immutable values:

| Field | Meaning |
| --- | --- |
| `calendars` | Exact calendar identity/version, actual sorted unique holidays and weekday weekend rules; no calendar-name inference |
| `helper_calendars` | Explicit mapping from an older helper's opaque revision to an actual supplied calendar pin; no revision parsing or guessing |
| `curves` | Complete dated curve recipes, roles, domains, nodes/quotes, interpolation, extrapolation and numerical controls |
| `calibration` | Optional complete rate/credit/cash quote graph, original helpers, resource revisions and fit settings; actual rebuild replaces supplied baselines only through native graph admission |
| `fixings` | Full observations, publication times, sources and revisions; caller supplies eligible-history selection |
| `publications` | Actual publication requirements used to distinguish missing due history, awaiting publication and future observations |

Each bond supplies full `BondTerms` and four independent payment/reset/observation/
exercise pins. Each IRS/OIS/basis leg retains its own calendar, schedule, profile,
reset/observation, projection, collateral and sign conventions. CDS retains actual
entity, seniority, legal documents, recovery, settlement and survival mapping.
Futures retain the real contract, clearing, basket, conversion factors, delivery
window, source quality, carry and economic cutoffs. Model assumptions or dated
swaption calibration inputs are explicit. Calendar or curve IDs never select a
provider or authorize a lookup.

Success is `{"ok":"true","schema_version":1,"result":{"data":...,"market_calibration":...}}`.
`data` retains the operation's native output. `market_calibration` is null without
a graph, otherwise it retains the actual graph diagnostics and original helper
repricing. Solver roots, native residuals, limits, rank/condition diagnostics and
model/grid refinements remain visible. No partial graph becomes a successful
curve. Missing auction recovery or portfolio coverage stays explicitly pending
or incomplete when that is the native result's semantics.

Failure is `{"ok":"false","schema_version":1,"error":{"code":...,"message":...,"details":...}}`.
There is no successful zero substitute. Codes distinguish `invalid_input`,
`missing_input`, `not_applicable`, `unsupported`, `no_root`, `non_unique`,
`no_convergence`, `resource_limit`, `analytics` and `internal`. Details retain the
native missing manifest or numerical diagnostic when available. The existing C
boundary also reports pointer/UTF-8/admission/panic errors in its original error
envelope; those boundary failures can precede request decoding.

## Operations and financial units

| Operation | Native behavior and principal limits |
| --- | --- |
| `capabilities`, `roundtrip` | Complete required surface report; checked immutable input reconstruction/echo |
| `schedule`, `fixings` | Independent actual date rules and eligible-history requirements |
| `curves`, `curve_query`, `curve_fit` | Native construction, fallible date/tenor/DF/forward/survival/hazard queries and bounded dated money-market fitting |
| `calibration` | Original full rate/CDS/cash graph build, product price, quote scenario and stable quote risk; preserves original helpers and history |
| `bond` | Construction, requirements, full entitled cash, fixed/floating model PV, dated fixed-cash yield or spread/DM inverse, guarded raw quote risk |
| `swap`, `basis_swap` | Original full IRS/OIS or two-floating-leg basis construction, requirements, projected cash and signed valuation |
| `cds`, `credit_risk` | Original full single-name pricing/conversion, calibrated scenarios, signed quote/recovery risk and pending or supplied JTD |
| `helper`, `repo_forward` | Native helper descriptions/build where admitted, original cash quote conversions/basis-bond model and all dated repo cash horizons |
| `zero` | Full original zero terms, checked per100/yield pricing/risk; checked short-bill bank discount/BEY only for at most 182 actual days with face equal to absolute redemption |
| `optional_bond` | Original HW1F assumptions or dated swaption sigma calibration; supported price, OAS, scenario and separately refined risk |
| `rate_future`, `bond_future` | Remaining fixing/projection/convexity model, quote risk and all-member bond delivery/CTD valuation/risk |
| `cash_hedge`, `futures_math`, `round_hedge`, `bond_cds_hedge` | Native indicative cash hedge, clearing settlement/tick/roll, integer hedge residual, and legal/recovery/basis admission; no constrained optimizer or execution authority |
| `aggregate` | Expected trade/factor manifest and same-snapshot FX; missing outcomes and unknown gross remain visible |
| `lifecycle` | Deterministic reconstruction/replay over full original terms, actual event inputs, receipts and exact signed quantities; proposed cash/security effects carry no posting authority |

Decimal terms, monetary values and exact native quotations use JSON **strings**.
Send `"99999999999999999.00000000001"` as text. Numeric JSON cash is rejected;
more precision than the native Decimal can retain is rejected. Native floating
model rates, computed per100 quote metrics and unrounded numerical diagnostics
retain their original number representation; the adapter does not claim new
precision for them. Native percent, basis-point, decimal-rate, futures-point,
original-face and absolute currency units remain distinct. In particular, the
short-bill conversion returns percent, while `BondMethod` yield/Z/DM and OAS are
annualized decimal rates. Risk keys and quantity/reference units must agree with
the actual native product. Do not infer semantics from a field called `value`.

Optional instruments cannot use plain bond pricing. Floating instruments cannot
use a contractual fixed-cash yield. Raw futures quote sensitivity is distinct
from remaining-rate risk and posted variation margin. OAS risk must pass native
price and duration/convexity refinement at both h and h/2. Price convergence
alone does not admit risk.

## Clients and qualification

| Client | Entry point | Current qualification |
| --- | --- | --- |
| Rust | `convex::library::{execute,execute_json,decode_request}` | Actual engineering corpus and native error/roundtrip tests |
| C | `convex_library(const char*)` | Actual owned UTF-8 response parity; free every returned pointer with `convex_string_free` |
| Java | `Convex.library(String)` | Actual FFM/C transport parity on the recorded macOS ARM64 JDK host; other platform packaging remains a host gate |
| Excel | `CX.LIBRARY` and `CX.LIBRARY.RANGE` | Actual P/Invoke tests are wired into Windows CI; .NET and Excel host execution are unavailable locally and remain unverified |
| MCP | `library({request_json})` and `library_schema` | Actual tool handler text parity; remote transport and server authority remain separate |
| WASM | `library(request_json): string` and `library_schema(): string` | Native Rust export parity; wasm32/browser execution remains unverified on this host |

All generic client entry points accept and return raw JSON text. The generated
request schema is available with the Rust `schemars` feature, C
`convex_schema("LibraryRequest")`, MCP `library_schema` and WASM `library_schema`.
JSON Schema checks structure; actual native admission still checks financial
meaning, exact resource revisions, history, numerical limits and missing data.

For C/Java/Excel build the `excel` profile, which unwinds across the guarded host
boundary: `cargo build --locked --profile excel -p convex-ffi`. Rebuild/repackage
client native libraries together with the wrapper; an older library without
`convex_library` is incompatible and must not silently fall back.

Excel's one-cell function allows at most 32,760 UTF-16 characters.
`CX.LIBRARY.RANGE` concatenates textual cells in row order and spills the full raw
response in a single column. It preserves digits and Unicode pairs; it rejects
numeric cells and never truncates a long response. Combine returned chunks before
parsing JSON. Existing convenience functions retain their original documented
units/limitations. In particular, the older FRN entry points cannot supply full
history/publication/projection resources and fail explicitly. They do not gain
complete FRN support from the addition of a generic route.

The older MCP `g_spread` operation now reports `unsupported`: its request cannot
supply actual government benchmark securities/yields. Its tenor-only bootstrap
and synthetic hedge conveniences remain `compatibility_unverified`; they cannot
establish AC3 for complete multi-curve or hedge behavior. The generic route does
not silently upgrade them. Earlier quantitative fixtures and bounds stay protected.

## Bounds and deployment

Strict JSON admission limits input to 2 MiB, 64 levels, 32,768 nodes, 4,096 items
per collection and 8,192 UTF-8 bytes per text value. Unknown/discarded nested
fields, duplicate keys, trailing values and lossy numeric/string conversions
fail. Calendars and supplied curves are each limited to 256; calendar holidays
and replay events to 4,096. Output above 16 MiB is a typed failure with no partial
success. Native per-product/calendar/solver/helper limits apply as well. These
are admission ceilings, not a latency SLA or total CPU bound for nested work.

The pure request API can run on any process and server supplied the same immutable
inputs. Storage, messaging, provider integration, cache ownership, snapshot
selection and authorization belong to the application. Co-location and hot hedge
loops should use prepared typed native objects, retain their immutable source
pins and avoid serializing/recalibrating the full graph on each tick. Host timing,
resource budgets and fault handling require their own measurements and gates.

## Validation and remaining work

```sh
cargo test --locked -p convex --all-features --test library -p convex-ffi --test library -p convex-mcp --test library -p convex-wasm --test library
cargo test --locked -p convex-ffi -p convex-wasm -p convex-mcp
python3 -m unittest discover -s tools/library -p 'test_*.py'
python3 -m venv /tmp/convex-library-schema
/tmp/convex-library-schema/bin/python -m pip install -r tools/library/requirements-schema.txt
/tmp/convex-library-schema/bin/python tests/scripts/library_schema.py --binary target/debug/examples/library_workflow
```

Build `library_workflow` with `schemars` before the schema command. To run real
Java/native parity, build both the example and FFI library in the `excel` profile,
stage the library under `bindings/java/src/main/resources/native/<platform>/`,
then run:

```sh
python3 tests/scripts/library_parity.py prepare --binary target/excel/examples/library_workflow --output bindings/java/target/library-parity.json
mvn -B -f bindings/java/pom.xml verify
```

Fresh native parity outputs test transport only. They are never independent
financial oracles. Original quantitative fixtures, captures and tolerances remain
unchanged. The capability audit retains all 2,154 original required records,
174 combinations and 30 areas with their owners/surfaces/gates. Family examples
do not establish every model/convention/operation certificate.

AC2 still needs full profile/notice-aware workout yields, admitted parametric and
segmented/delegated constructions, required American/make-whole and FRN cap/floor/
collar models, complete per-operation helper/aggregation/reconciliation coverage,
and constrained cross-instrument hedge optimization (CVX-031). Every exact
combination still needs its original independent contract/model/convention pack.
Excel and actual browser/WASM qualification need executable hosts. See each
report record for its remaining owner, and use the private CVX-063 follow-up
harness prompt. CVX-064/065 may certify only actual completed mandatory scope.
