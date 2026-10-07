# Standalone library capability inventory

CVX-020 adds [native single-name CDS valuation and quote conversion](CDS-VALUATION.md)
through `convex::credit`, with premium/protection/default accrual and explicit
initial cash state. All 7,164 original USD SOFR ISDA 1.8.3 grid cases are checked
at the declared bounds using actual original-model outputs and current native
execution. The CDS observation now records this callable subset; calibration,
quote risk/JTD, lifecycle, host dispatch and independent certificates remain
owning work. All 2,154 mandatory records remain Planned.

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
Thirty-eight new tests include 512 deterministic generated cases. The CVX-060
inventory had 40 observations and 76 inspected source pins. All 2,154 mandatory
records remain Planned; full methods/products, admitted market packs, host
dispatch and independent LB0/LB1 certificates remain owning work.

CVX-017 adds [shared supplied bond quotes](BOND-QUOTES.md), checked fixed/zero
economics, explicit curve roles, actual government benchmarks and bounded
missing-input errors. Thirteen supplied dated contracts cover coupons, ex-coupon,
leap/stub/EOM, holiday payments and negative yields. Native/C ABI parity covers
all thirteen; REST covers eight admitted contracts and an explicit premium EUR
zero. The fresh retained comparison passes all 392 measures at unchanged
reference values/tolerances. Earlier failing captures remain historical evidence.
The CVX-017 inventory had 41 observations and 91 source pins; all 2,154 records
remain Planned pending independent market/model and full surface certificates.

CVX-055 adds [full supplied bond terms and cash](BOND-CONTRACTS.md): checked
independent stepped/amortizing profiles, term/overnight fixing-aware projection,
exact principal allocations and supported fixed-price call/put notice terms.
Nineteen new native/analytics tests include ten independent Decimal(50) cash/PV
cases and 128 seeded exact step/principal cases. A standalone consumer prices
plain cash with separate supplied projection/discount roles in three feature
profiles. Native/JSON reconstruction preserves complete supported economics;
optional pricing requires a model and is refused by the plain price route.
At that revision the inventory had 42 observations and 100 source pins. All 2,154 records
remain Planned; full model/market, inverse/risk/lifecycle and host/schema
certificates remain required.

CVX-059 adds [checked optional-bond models](OPTIONAL-BONDS.md): explicit HW1F
assumptions or dated single-curve normal/shifted-lognormal sigma calibration;
supported European/Bermudan call/put notice pricing and restricted genuine joint
term-FRN state; fractional OAS and fixed-model factor risk. Independent analytic
references and separate price/sensitivity refinements retain actual diagnostics.
Undersampled 1bp option risk, American/make-whole/triggered/cap-floor/overnight
option and off-cycle FRN combinations fail explicitly. The existing compatibility
routes remain unqualified. The inventory now has 43 observations and 113 source
pins. All 2,154 mandatory records remain Planned; full market/model, host/schema,
lifecycle and independent quantitative/library certificates remain required.

CVX-018 adds [pure supplied IRS/OIS contracts](RATES-CONTRACTS.md) through
`convex::rates`: independent dated coupon/reset/observation and exact calendar
versions, actual profile steps, strict native economics and signed expected
cash with eligible history/forecast lineage. Fifteen native tests, 15 independent
supplied-contract fixtures and 128 deterministic variations exercise positive,
boundary and failure paths. The public inventory has 121 source pins and 44
observations. Discounted trade pricing, accrued, fair rates, multi-curve fitting
and quote risk, lifecycle, shared host schemas/dispatch, exact market packs and
independent certificates remain pending. All mandatory records remain Planned.

## Evidence and missing operations

`requirement_areas` maps requirements, minimum gate, operations, surfaces and
owning tasks. `combinations` gives actual product, convention/model profile,
economic scope, build requirements and inspected family observations.
`records` explicitly names each required operation and surface with pending
task ownership. IDs are stable and include their dimensions. This simple
JSON join avoids duplicating source/provenance text in thousands of rows;
it introduces no production capability registry or pricing abstraction.

`observations` records partial, missing, type-only, approximate or missing
construction/dispatch paths. Anchors and 100 exact source hashes prove which
bytes were inspected. Family test anchors are **candidates for reuse**, not
proof that every convention/model/operation/surface record passes them.
Actual command receipts and standalone results belong to task evidence.
Source review did not establish line/branch coverage or quantitative approval.

| Area | Mandatory scope | Current evidence and primary gaps | Owning tasks |
|---|---|---|---|
| LC-001 | Shared calendar and schedule primitives (LB0) | checked supplied-contract primitives implemented; exact market conventions/certificate pending | CVX-054, CVX-064 |
| LC-002 | Fixings and dated coupon/principal profiles (LB0) | checked publication/revision and dated-profile APIs implemented; full product migration/certificate pending | CVX-054, CVX-064 |
| LC-003 | Fixed-rate bullet bonds (LB0) | shared supplied fixed quotes and native/C ABI/admitted REST parity; independent convention and full surface certification pending | CVX-017, CVX-054 |
| LC-004 | Bills and zero-coupon bonds (LB0) | shared explicit zero economics and admitted host parity; bill workflows, convention and full surface certification pending | CVX-017, CVX-054 |
| LC-005 | Plain floating-rate notes (LB1) | checked supplied term/overnight cash and model PV; old diagnostic shortcuts, full host/inverse/risk/lifecycle and certification pending | CVX-055, CVX-061, CVX-062, CVX-063 |
| LC-006 | Amortizing and sinking bonds (LB1) | checked dated principal/coupon and priced repayments; full host/inverse/risk/lifecycle and certification pending | CVX-055, CVX-061, CVX-062, CVX-063 |
| LC-007 | Deterministically stepped coupons and notionals (LB1) | independent checked dated profiles and native JSON terms; full host/inverse/risk/lifecycle and certification pending | CVX-055, CVX-061, CVX-062, CVX-063 |
| LC-008 | Callable and putable bonds (LB1) | checked fixed-price notice call/put HW1F pricing, OAS and resolved risk; unsupported models, full market/lifecycle/host certification pending | CVX-055, CVX-059, CVX-061, CVX-062, CVX-063 |
| LC-009 | Callable floating-rate notes (LB1) | checked aligned term-FRN common HW1F state and eligible history; off-cycle/overnight models, full market/lifecycle/host certification pending | CVX-055, CVX-059, CVX-061, CVX-062, CVX-063 |
| LC-010 | Vanilla term-rate IRS (LB1) | checked native supplied terms/reset/cash/PV/accrued/par and fitting; risk, lifecycle, hosts and certificates pending | CVX-018, CVX-019, CVX-061, CVX-062 |
| LC-011 | OIS (LB1) | checked native supplied observation/compounding/cash/PV/accrued/par and fitting; risk, lifecycle, hosts and certificates pending | CVX-018, CVX-019, CVX-057, CVX-061, CVX-062 |
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
| LC-022 | Optional-bond model inputs and calibration (LB1) | checked explicit HW1F and dated native-volatility sigma calibration; full surface/model/market certification pending | CVX-059 |
| LC-023 | Curve queries and numerical transformations (LB0) | checked queries/conversions and explicit rate roll/views; credit/model transforms and independent certificate pending | CVX-056, CVX-060 |
| LC-024 | Quote, node and scenario sensitivity construction (LB1) | optional fixed-model factor risk checked; small-bump accuracy, quote rebuild and full sensitivity certificates pending | CVX-024, CVX-057, CVX-058, CVX-059, CVX-061 |
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
- [Native CDS valuation](CDS-VALUATION.md) and [credit calibration/risk inputs](CREDIT-CALIBRATION.md)
  supply single-name premium/protection/upfront, full quote fitting, signed risk
  and immediate conditional JTD candidates. Complete seasoned IRS/OIS
  risk/lifecycle/host workflows remain incomplete. [CVX-019](SWAP-VALUATION.md) adds
  native seasoned/amortizing IRS/OIS PV/accrued/par and full-contract fitting
  candidates; its ten-case captured QuantLib comparison now passes 973 numeric
  checks within the original bounds. Independent market/model/library approval
  remains pending. CreditCurve
  queries do not provide CDS calibration; synthetic
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

CVX-040's recorded 392-measure comparison had 287 passes and 105 failures.
CVX-017 repaired shared payment-step pricing; the current CVX-059 replay passes
392/392 with unchanged expectations, tolerances and retained-unqualified status.
Historical failures remain evidence. See the [comparison contract](../reconciliation/COMPARISON.md) and
[assurance policy](ASSURANCE.md). Passing this inventory task completes its
review infrastructure only. Product models, actual exact convention packs,
full positive/boundary/failure fixtures, host builds, numerical certificates,
production gates and latency/colocation remain their owning tasks.

Version 9 adds inspected native swap pricing/common-PV and product-fit source
and test anchors, including explicit discount/index dependencies and zero-basis
overnight projection modeling. Twenty manual independent reference cases and
bounded positive/failure calibration tests do not establish an executed
ORE/QuantLib comparison. All 2,154 mandatory records and their byte-preserved
pending specification remain Planned; risk, lifecycle, market/convention packs,
hosts and independent library/production certificates remain owning work.

Version 13 adds inspected full CDS calibration and risk APIs through the native
facade, preserving every mandatory requirement, owner, surface and status.
Actual original ISDA 1.8.3 execution provides 23 supplied engineering scenarios,
276 valuations and 1,150 new numeric checks. Piecewise/global native fits enforce
original quote units, residuals, nonnegative hazards and exact source lineage.
Frozen/rebuilt rates and recovery remain distinct; running CS01, upfront and
recovery derivatives have explicit units. Immediate unknown recovery remains
Pending. Bond/CDS admission requires actual legal mapping and dated basis/liquidity
inputs; it is not a complete bond credit pricing or executable hedge workflow.
All 2,154 mandatory records remain Planned; market/model packs, lifecycle,
host/schema dispatch and independent certificates remain owning work.

Version 14 records [dated futures and cash reference inputs](FUTURES-REFERENCE.md)
through the pure facade and existing authoritative repository. Exact published
December ZN identities/factors, contract/basket/security pins, dated ticks and
disjoint rate observation windows survive import, correction, replay and redb
reopen. Source quality/freshness and actual cash size/lot/funding/borrow/cost
prerequisites have explicit blocking reasons. The approximate advisor labels
all proposals indicative only. This extends reference admission rather than
certifying CTD/carry, partly fixed rate valuation, margin/lifecycle or executable
hedging. All 2,154 mandatory records remain Planned; CVX-023 owns the complete
futures valuation and risk work, with host and certificate gates still pending.

Version15 records [supplied futures/cash valuation](FUTURES-VALUATION.md): full
fixed-bond deterministic carry/CF/invoice and independent CTD rankings; all-member
zero/repo/basis repricing with switches; publication-aware arithmetic/compounded
overnight and single-term pricing with remaining forward risk; actual cash hedge
model/side/cost estimates; normal monetary-level settlement rounding and explicit
tick/integer/supplied roll diagnostics. Source-quality/time blocks remain visible
and every pure output is indicative. Independent published hypothetical settlement
and Decimal cash/risk fixtures qualify only this scoped engineering behavior.
Legacy approximate advisor formulas remain unchanged and unqualified. Full
market/convexity/optional-delivery packs, canonical portfolio risk, lifecycle/P&L,
host/schema dispatch, execution and independent certificates remain owning work.
Every mandatory record, combination, exclusion and gate remains unchanged and
all2154 required records remain Planned.

Version16 records [complete supplied mixed rate calibration](RATE-CALIBRATION.md)
and two-floating-leg tenor-basis construction/pricing. Original dated deposit,
FRA, partly-fixed/arithmetic/compounded/single-term futures, IRS/OIS and basis
quotes fit separate exact discount/projection graphs. Linear piecewise and
Linear/CubicSpline global fits retain every original native residual, complete
known/forecast cash and measured work/rank diagnostics. Explicit global overlap
retains all quotes; inconsistent inputs, missing history/versions, insufficient
pillars and cycles fail. Rebuilds preserve original fixing revisions and the
caller's last-good set. Independent Decimal synthetic references and native
failure/property controls qualify this route as engineering work; no new ORE
execution or full convention/model/host/quantitative certificate is implied.
Every required area/combination/record/exclusion/gate and all2154 Planned
mandatory records remain byte-preserved. CVX-058 government/issuer/repo/bond
basis, canonical risk, lifecycle/P&L, hosts and release certificates remain open.
