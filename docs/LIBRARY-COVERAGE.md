# Standalone library capability inventory

CVX-053 inventories actual implementation and missing work. The public
[inventory](../tools/library/library-inventory.json) preserves 30 mandatory
areas and expands them into 174 scoped combinations and 2,154 explicit
product/convention/model/operation/surface records. All remain **Planned**;
none is a library certificate. Existing callable functions are described as
reuse candidates, with inspected public paths and source/test anchors.

The initial scope includes nominal bonds/bills/zeros, existing FRN,
amortizing/sinking/stepped/optional families, IRS/OIS, single-name CDS,
government-bond/rate futures, and their required curve, inverse, risk,
lifecycle and hedge workflows. USD/EUR/GBP packs are proposals requiring
exact dated contract/document admission in their owning tasks. Retained JPY
behavior is separately labelled compatibility scope. Currency or index names
alone do not select an approved market convention.

## Run and interpret

From the repository root, using Python 3.11+ and the pinned Rust toolchain:

```sh
python3 tools/library/check.py
python3 tools/library/check.py --report /tmp/convex-library-report.json
python3 tools/library/check.py --base <review-base-sha>
python3 -m unittest discover -s tools/library -p 'test_*.py'
cargo run --locked -p convex --example inventory_probe
cargo run --locked -p convex --example inventory_probe --no-default-features
cargo run --locked -p convex --example inventory_probe --features schemars
python3 tools/library/check.py --gate LB0
python3 tools/library/check.py --gate LB1
```

Normal checking returns zero only for consistent metadata, complete pending
mapping and unchanged inspected source bytes. The two `--gate` commands
currently return **one**, with `status: incomplete` and `certified: false`.
LB1 includes LB0. A source symbol, test name, successful compilation,
passing subset or populated enum cannot award `Verified`. CVX-064/065 must
implement the actual independently reviewed standalone certificates.

The native example uses public facade and direct crate exports without
convex-engine, a server, database, MQ, provider or clock. It checks supplied
bond schedules/cashflows, clean/dirty decomposition, price/yield inversion,
finite existing risk, flat-curve analytic queries, two dated-deposit global
and piecewise calibrations, and invalid date/duplicate node/empty helper
failures. Its new reachability bounds are documented in the example; it
changes no existing financial tolerance. It also **characterizes current
incorrect/incomplete behavior**: piecewise-constant hazard interpolation
returns a linear midpoint, and four missing historic overnight observations
are forecast. Passing those characterization controls does not bless the
fallbacks. Owning fixes must update the evidence and controls together.

CVX-054 adds [shared native cashflow primitives](CASHFLOW-PRIMITIVES.md): one
anchored schedule generator in core, distinct date rules and ICMA references,
checked dated coupon/spread/gearing/notional profiles, explicit cash entitlement
and principal effects, and eligible fixing/overnight resolution. Thirty-six
new tests and a standalone consumer exercise supplied contracts; source
observations increase to 38. All 2,154 records remain Planned until their exact
market/product/surface certificates are established. The old date-only ARRC
characterization above remains a compatibility gap.

CVX-056 adds [checked native curve contracts](CURVE-CONTRACTS.md), an immutable
versioned dependency set and full dated future/bond/CDS/IRS/basis helper
descriptions above the curve layer. Direct DF/zero/survival curves, an exact
dated deposit/FRA chain, continuous zero spreads and explicit date/tenor
queries are callable through a standalone facade consumer. Forty-four new
tests exercise analytic identities, native quote repricing, economic term
roundtrips and explicit unsupported dispatch. The inventory now records 39
observations and 68 source pins. Product-backed/global calibration, rolling,
schema/host parity and numerical certification remain owning work; all 2,154
records stay Planned.

CVX-060 adds [bounded numerical fitting and transformations](CURVE-NUMERICS.md):
scaled native-unit dated deposit/FRA fitting with rank/limit diagnostics,
stable compounding conversions, exact representation views and explicit rolling.
Thirty-eight new tests include 512 deterministic generated cases. The current
inventory has 40 observations and 76 inspected source pins. All 2,154 mandatory
records remain Planned; full methods/products, admitted market packs, host
dispatch and independent LB0/LB1 certificates remain owning work.

## Evidence and missing operations

`requirement_areas` maps requirements, minimum gate, operations, surfaces and
owning tasks. `combinations` gives actual product, convention/model profile,
economic scope, build requirements and inspected family observations.
`records` explicitly names each required operation and surface with pending
task ownership. IDs are stable and include their dimensions. This simple
JSON join avoids duplicating source/provenance text in thousands of rows;
it introduces no production capability registry or pricing abstraction.

`observations` records partial, missing, type-only, approximate or missing
construction/dispatch paths. Anchors and 76 exact source hashes prove which
bytes were inspected. Family test anchors are **candidates for reuse**, not
proof that every convention/model/operation/surface record passes them.
Actual command receipts and standalone results belong to task evidence.
Source review did not establish line/branch coverage or quantitative approval.

| Area | Mandatory scope | Current evidence and primary gaps | Owning tasks |
|---|---|---|---|
| LC-001 | Shared calendar and schedule primitives (LB0) | checked supplied-contract primitives implemented; exact market conventions/certificate pending | CVX-054, CVX-064 |
| LC-002 | Fixings and dated coupon/principal profiles (LB0) | checked publication/revision and dated-profile APIs implemented; full product migration/certificate pending | CVX-054, CVX-064 |
| LC-003 | Fixed-rate bullet bonds (LB0) | failed comparison, lossy schema, missing dispatch, unverified conventions | CVX-017, CVX-054 |
| LC-004 | Bills and zero-coupon bonds (LB0) | incomplete contract, incomplete facade, missing dispatch, unverified conventions | CVX-017, CVX-054 |
| LC-005 | Plain floating-rate notes (LB1) | incomplete contract, incomplete facade, missing dispatch, missing input substitution, missing option model | CVX-055, CVX-061, CVX-062 |
| LC-006 | Amortizing and sinking bonds (LB1) | incomplete contract, missing lifecycle, type only | CVX-055, CVX-061, CVX-062 |
| LC-007 | Deterministically stepped coupons and notionals (LB1) | lossy schema, missing implementation | CVX-055, CVX-061, CVX-062 |
| LC-008 | Callable and putable bonds (LB1) | incomplete contract, lossy schema, missing dispatch, missing lifecycle, unverified model | CVX-055, CVX-059, CVX-061, CVX-062 |
| LC-009 | Callable floating-rate notes (LB1) | lossy schema, missing dispatch, missing option model | CVX-055, CVX-059, CVX-061, CVX-062, CVX-063 |
| LC-010 | Vanilla term-rate IRS (LB1) | approximation, missing lifecycle, missing trade pricer | CVX-018, CVX-019, CVX-061, CVX-062 |
| LC-011 | OIS (LB1) | incomplete contract, missing lifecycle, missing trade pricer | CVX-018, CVX-019, CVX-057, CVX-061, CVX-062 |
| LC-012 | Single-name CDS (LB1) | missing dispatch, missing lifecycle, missing trade pricer | CVX-020, CVX-021, CVX-061, CVX-062 |
| LC-013 | Government-bond futures (LB1) | approximation, incomplete contract, missing lifecycle | CVX-023, CVX-058, CVX-061, CVX-062 |
| LC-014 | Rate futures (LB1) | incomplete contract, missing lifecycle, missing trade pricer | CVX-023, CVX-057, CVX-061, CVX-062 |
| LC-015 | Core yield curve construction (LB0) | checked direct/exact-chain and scaled dated core global fit; full model/product/host and independent certificate pending | CVX-056, CVX-060 |
| LC-016 | OIS discount curve calibration (LB1) | approximation, incomplete contract, missing dispatch, missing quote rebuild | CVX-057 |
| LC-017 | Index projection and tenor-basis curves (LB1) | approximation, lossy schema, missing calibration, missing quote rebuild, type only | CVX-057 |
| LC-018 | Government benchmark curves (LB1) | missing calibration, missing quote rebuild, type only | CVX-058 |
| LC-019 | Credit survival and hazard curves (LB1) | checked supplied survival/domain queries; CDS calibration, quote rebuild and certificate pending | CVX-020, CVX-021 |
| LC-020 | Issuer spread and bond-basis curves (LB1) | missing calibration, missing quote rebuild | CVX-058 |
| LC-021 | Repo and carry term structures (LB1) | incomplete contract, missing calibration | CVX-058 |
| LC-022 | Optional-bond model inputs and calibration (LB1) | incomplete contract, unverified model | CVX-059 |
| LC-023 | Curve queries and numerical transformations (LB0) | checked queries/conversions and explicit rate roll/views; credit/model transforms and independent certificate pending | CVX-056, CVX-060 |
| LC-024 | Quote, node and scenario sensitivity construction (LB1) | incomplete contract, missing quote rebuild | CVX-024, CVX-057, CVX-058, CVX-059, CVX-061 |
| LC-025 | Complete native valuation and inverse facade (LB1) | incomplete facade, missing requirements, missing trade pricer | CVX-061 |
| LC-026 | Pure cross-product lifecycle functions (LB1) | missing dispatch, missing lifecycle | CVX-062 |
| LC-027 | Signed aggregation and cross-instrument hedge mathematics (LB1) | approximation, incomplete constraints, incomplete facade, missing credit hedge, missing dispatch | CVX-024, CVX-031, CVX-061 |
| LC-028 | Lossless shared JSON and C ABI (LB1) | incomplete provenance, lossy schema, missing dispatch | CVX-063 |
| LC-029 | Advertised Java Excel MCP and WASM parity (LB1) | incomplete features, lossy schema, unexecuted host parity | CVX-063 |
| LC-030 | Capability inventory and evidence gate (LB0) | certification pending | CVX-053, CVX-064 |

The highest-impact concrete findings are:

- `InstrumentType::BasisSwap`, government coupon/zero and TBill helper labels
  lack corresponding complete construction/pricing routes in the reviewed
  calibration module. Existing Swap/Ois/Future helpers are not complete trades.
- Native `PiecewiseBootstrapper` is callable through convex-curves; shared
  `BootstrapMethod::Piecewise` is accepted by the schema but rejected in FFI.
  The FRA wire helper derives start from `tenor.fract()`, losing a dated start.
  The separate CVX-056 native recipe preserves actual dates and reprices an
  exact deposit/FRA chain; it does not change that older wire dispatch.
- `InterpolationMethod::PiecewiseConstant` selects `LinearInterpolator`.
  Direct Nelson–Siegel/Svensson math exists; discrete construction rejects
  those selections, and a complete fitted recipe/residual workflow is pending.
  The CVX-056 checked recipe explicitly rejects those unsupported selections
  and approximate fallbacks.
- Shared `CouponSpec` omits first/last/dated coupon dates, EOM, supplied calendar,
  ex/settlement days and redemption. FFI construction also ignores its accepted
  business-day convention and `CallableSpec.put_schedule`. These terms cannot
  be certified by a serde roundtrip of the fields that happen to exist.
- Native `CallableFloatingRateNote` and DM-to-workout/worst are callable, but
  there is no shared construction variant or complete joint floating/exercise
  model workflow. Forecast cap/floor clipping is not an option price.
- CVX-054 supplies checked pure publication/revision-aware fixing and overnight
  APIs, with complete missing history rejected before projection. Existing FRN
  and date-only ARRC product routes still require migration; they can use zero
  current rate or forecast missing past observations. Those compatibility
  routes are not certified by the new primitives.
- Native CDS premium/protection/upfront/JTD and full seasoned IRS/OIS workflows
  are missing. CreditCurve queries do not provide CDS calibration; synthetic
  swap hedge risk assumes floating-leg DV01 zero. Risk across projection,
  discount, basis and credit factors requires the actual product workflow.
- Generic FFI position risk rejects zero-coupon and FRN dispatch; existing
  price/basic risk support is a separate capability. Callable OAS has explicit
  spread/risk APIs, while the generic PricingRequest lacks volatility input.
- Futures CTD and basket overrides exist, with synthetic schedule/CF/carry
  assumptions. Actual dated contracts, realized fixings, margin/delivery and
  switches are not supplied by those approximations. No CDS hedge candidate
  or complete common-factor constrained cross-product solver is present.
- Java and Excel use generic JSON/C calls and their builders. MCP and WASM
  have **separate DTOs/direct native routes**, including percent conventions.
  The earlier memory claim that every host shares one identical wire schema
  does not describe current source. No client is silently removed to pass.
- Pure state/event economic lifecycle, complete resolved input discovery,
  quote-bump/rebuild dependency risk and full cross-product facade remain
  incomplete. Cached server values cannot satisfy those records.

The umbrella facade omits some flat reexports, including sinking/callable FRN,
credit/segmented/bumped curves and piecewise calibration. Their direct crate
paths remain valid native entry points. The inventory distinguishes facade
convenience gaps from missing native mathematics; no redundant reexports or
new runtime types are added merely to produce this review.

## Scope and review rules

Three economical non-applicable combinations carry reasons: CDS bond YTM,
CTD for a cash-settled rate future, and exercise valuation for an explicitly
non-optional bullet. Nine excluded product groups remain outside the initial
certificate: inflation, cross-currency, MBS/ABS/prepayment, convertibles,
perpetual/hybrid, CDS indices/tranches/options and broader exotics. Their
`Deferred` records cannot replace any mandatory operation. A retained type
label does not authorize accepting these terms as a vanilla approximation.

The checker rejects missing areas/records/surfaces/profiles, duplicate IDs/keys,
nonfinite JSON, absent owners, stale hashes/anchors, path escapes and mandatory
NotApplicable/Deferred/Verified claims. `ImplementedUnverified` additionally
needs an inspected candidate route/test; partial or declaration-only records
cannot acquire that status. Validation remains enabled under Python `-O`.

Against a git base, version refresh cannot remove existing combinations,
operations, surfaces, owners or mandatory semantics, delay their gate, or
reinterpret an existing convention/model/economic profile. Add reviewed
profiles instead. A future certificate/scoped migration requires explicit
engineering and quantitative evidence; do not bypass the guard to mark a row
complete. Source hash updates need source review and an inventory version
increase; hashes are not binary attestation or independent price oracles.

The private specification references this public inventory by exact hash and
checks all 30 area/operation/owner/combination mappings. Public CI runs without
that private submodule. Assurance invokes inventory validation and base
conservation, protects the tooling in candidate impact review, and requires
mutation tests plus all three standalone consumer feature builds.

CVX-040's 392-measure retained comparison still has 287 passes and 105 failures;
its existing expectations, tolerances, qualification and failing CI remain
unchanged. See the [comparison contract](../reconciliation/COMPARISON.md) and
[assurance policy](ASSURANCE.md). Passing this inventory task completes its
review infrastructure only. Product models, actual exact convention packs,
full positive/boundary/failure fixtures, host builds, numerical certificates,
production gates and latency/colocation remain their owning tasks.
