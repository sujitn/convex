# Bounded Rust and model assurance

Convex applies a small, enforceable subset of the NASA-inspired production
rules. This is an engineering assurance baseline, not NASA certification,
product certification, a latency guarantee, or a completed production gate.
The rules live in ordinary Rust attributes, CI, and `tools/assurance`; no
runtime policy framework is added to pricing.

Nested workflow modules are included in the same critical-source inventory.
Direct contextual ICMA yield risk adds mandatory immutable reference/admission/
failure/protocol, native/adapters, feature-consumer, schema and actual C parity
commands. It preserves original mathematical owners, numeric evidence/defaults,
dependency closure and existing checks; see [the contract](ICMA-YIELD-RISK.md).

## Scope and dependency direction

The pure calculation crates are `convex`, `convex-core`, `convex-math`,
`convex-curves`, `convex-bonds`, `convex-rates`, `convex-credit`, `convex-analytics`, and `convex-portfolio`.
Each forbids unsafe code and denies unchecked `must_use` results. Math and
FFI production builds additionally deny Clippy's `unwrap_used`, `expect_used`,
and `panic`; core contracts and global curve calibration apply the same
scoped rules. Shared core cashflows, fixings and checked calendars apply the
same denial. Test assertions remain available. FFI denies undocumented
unsafe blocks, missing safety documentation, and implicit unsafe operations.
Checked curve contracts and their analytics helper composition apply the
same scoped denial and retain the pure dependency direction.
Checked [supplied bond terms/projection/valuation](BOND-CONTRACTS.md) apply the
same denial over shared primitives. Their native checked adapters retain exact
economic terms, eligible history and explicit unsupported model requirements.
Checked [optional-bond models](OPTIONAL-BONDS.md) use the same scoped denial for
HW1F, dated calibration/pricing, bounded OAS and resolved/converged factor risk.

Checked [IRS/OIS contracts](RATES-CONTRACTS.md) apply the same denial at the
entire new pure rates crate. Independent calendars reuse the existing core
generator, and all required history is checked before projection.

Checked [single-name CDS valuation and conversion](CDS-VALUATION.md) apply the
same denial throughout the new pure credit crate. Exact supplied discount/credit
roles and dates, complete premium/protection/default accrual, explicit initial
cash state and native-unit root residuals are enforced. The original USD SOFR
ISDA RFR grid and actual model capture are separately pinned and checked against
current native outputs offline; no external engine enters production dependencies.

The dependency checker traverses all-feature, target-specific **normal**
dependency edges using `cargo metadata --locked`. It rejects network,
database, async-runtime, vendor-SDK and application adapter packages, plus
any normal package absent from the reviewed allowlist. Dev-only property
tools and build-only header generation do not enter the calculation graph.
The lockfile, manifests, source inventory and CI are change-review protected.
Legacy calendar file conveniences are inventoried separately; they do not
make filesystem access part of the production calculation flow.

`inventory.json` records each critical source hash, invariants, termination
bounds, failure contract, tests and applicable exceptions. The inventory
covers math, FFI, checked dates/contracts, selected calendar/day-count paths,
curve storage/calibration, HW1F and the existing graph/runtime boundary.
It is deliberately scoped: it does not assert every product or every pure
API is already complete. Its C export list detects ABI inventory drift.

## Numerical contracts and bounds

| Path | Enforced bound | Success/failure contract |
|---|---|---|
| Root solvers | 1–10,000 iterations per configured phase | Finite configuration, input, evaluations and iterates; successful residual below the requested tolerance; stagnation/nonconvergence is an error. |
| Hybrid solver | Up to 20 Newton steps, 50 bracket expansion steps, then the configured Brent budget | Normal zero-derivative/divergence recovery is allowed; nonfinite arithmetic is an error. Caller callbacks must terminate and must not panic. |
| Interpolation/tridiagonal | At most 65,536 points | Matched dimensions and finite values; strictly increasing finite interpolation nodes; invalid query/output is an error. Empty tridiagonal systems return the empty solution. |
| Dense LU | At most 256 × 256 | Finite arithmetic and explicit singular-system failure; existing pivot threshold retained. |
| Global calibration | At most 128 helpers; 1–10,000 LM iterations | Helper failures propagate, no zero residual or zero step substitutes; final finite RMS must satisfy the requested tolerance. |
| Piecewise calibration | At most 128 helpers, configured solver bound for each pillar, at most 32 full-curve refinement sweeps | Failed solves cannot become quote values. Full-curve repricing satisfies the requested tolerance or returns an error. |
| HW1F calibration | At most 128 helpers; combined expiry/tail ≤150 years, 200 minimizer iterations | Finite valid inputs and positive market values; missing curve values and failed Jamshidian roots return errors. |
| Checked HW1F swaption | At most 150 annual payments and 100 root iterations | Finite price or typed error; no zero-rate fallback. |
| Shared schedules/profiles | At most 4,096 entries per validated schedule/profile; years 1–9999, span ≤150×366 days | Anchored date grids, reference periods, independent dated terms and explicit entitlement; invalid/overlapping/gapped profiles fail. |
| Eligible fixings/overnight | At most 65,536 revisions and 4,096 requests/compound terms; offsets ≤366 business/calendar days, calendar scans ≤3,660 steps | Publication and receipt cutoffs, immutable revisions, complete missing-history failure before any forecast; checked positive Decimal product. |
| Supplied IRS/OIS contracts | Core grids/profiles ≤4,096; swap reset/observation/requirement terms ≤65,536; conservative profile/join visits ≤16,777,216 | Exact native economic/calendar/curve pins, dated reset and overnight weights, complete missing-history preflight and signed expected coupon cash. Unknown/unsupported payoffs, bad mappings, bounds and Decimal failures return errors. |
| Supplied single-name CDS | Core schedule/calendar/horizon limits; curves ≤1,024 nodes each; joint timeline ≤4,096 points; conversion ≤256 iterations, closed hazard bracket within [0,5] | Exact entity/seniority/recovery and discount/credit versions; complete leg integration and explicit cash state. Default conversion is [0,1], 1e-12 native residual, 100 iterations; unsupported/overdue cash, bad roles, failed native repricing or nonidentified hazard return errors. |
| Checked curve contracts | At most 256 supplied/built curves, 1,024 nodes or 1,023 helpers plus origin; time ≤150 years; inverse ≤16 bisections | Exact dependency versions, finite typed queries and native quote repricing; unsupported methods/products, cycles, invalid domains and failed residuals return errors without partial output. |
| Supplied bond composition | At most 4,096 entries per coupon/profile/exercise grid; 65,536 reset slices or overnight observations and unique fixing requirements per bond | Exact independent dated coupon/principal and eligible-history cash; all missing published history fails before forecasts. Plain PV requires separate declared curve roles and refuses optional valuation without a model. |
| Checked optional-bond model | At most 2,048 events/intervals, 2 million nodes/tree, 8,193 per row; 128 exercise rights, 512 future FRN coupons, 32 million visits/grid | Explicit immutable HW1F/volatility/curve/calendar/history inputs; conserving finite probabilities and supplied DF fit; actual notice cash and stochastic aligned term resets; unsupported structures, residual/grid failures or unresolved risk return errors. |
| Checked optional-bond calibration/inverse | At most 128 quotes, 512 dated payments each; 128 Jamshidian/64 implied-vol attempts, caller LM/SVD bounds ≤10,000; OAS closed bracket and ≤10,000 attempts | Strict native normal/shifted-lognormal residuals and rank; failed fitting yields no model. OAS holds parameters and projected index inputs fixed; risk refines duration/convexity separately with an explicit bump-resolution screen. |
| Checked date arithmetic | Chrono's representable calendar; checked day/month/year addition | Overflow/out-of-range returns an error; signed month arithmetic uses Euclidean division. |

Solver defaults remain `1e-10` and 100 iterations. Piecewise defaults remain
`1e-12` and 100 iterations. HW1F retains its existing minimizer bracket,
`1e-9` width tolerance and 200 iterations. The existing financial fixtures
and test tolerances are not widened. Root success now requires a residual
postcondition; a narrow bracket alone no longer certifies a root.

These are resource ceilings, not measured capacity or latency targets.
Caller allocations, arbitrary custom closures and allocator failure cannot
be made safe by an iteration count. Product-specific convergence studies
and comparator coverage remain separate work.

For externally supplied input use `Date::checked_add_days`,
`UfrConvergence::try_new`, `Extrapolator::try_extrapolate`, parametric
`try_forward_rate`, `golden_section_checked`, and `try_payer_swaption_hw1f`.
Older infallible signatures remain compatible under registered preconditions;
golden-section and swaption compatibility functions return NaN on failure,
never a plausible last iterate or invented rate. Do not consume those NaNs
as successful analytics.

Negative rates remain valid for the supported rate/log-discount paths;
discount factors above one are allowed. The existing monotone-convex model
has a nonnegative-forward constraint. UFR convergence remains a heuristic,
not Smith-Wilson. The approximate `SequentialBootstrapper` builds a quote
curve for diagnostics; its historical completion flag does not attest
repricing. Use the checked piecewise/global fitter in production ingress.
These limitations are explicit, time-bounded inventory exceptions.

## Host ABI

Build host libraries with `cargo build --locked --profile excel -p convex-ffi`.
The `excel` profile uses `panic=unwind`; an abort build of the FFI crate is
rejected at compile time. Invalid input uses normal errors. `catch_unwind`
is an unexpected-panic backstop, not an input validator and not protection
against abort, allocator termination or undefined behavior.

All JSON constructors and RPCs validate input before building or dispatching:

- At most 2 MiB of UTF-8 input, depth 64, 32,768 nodes, 4,096 entries per
  array/object, and 8,192 bytes per key/string. Duplicate fields at any depth
  and trailing tokens are rejected.
- At most 128 calibration instruments, 64 key-rate/scenario/proposal entries
  and 32 futures basket overrides. Numeric magnitudes are bounded by `1e18`;
  tenors by 150 years, date years by 1–9999, instrument spans by 150 × 366 days,
  rates/coupons by −10 to 10 and volatility by 0 to 10.
- At most 32 simultaneous constructor/RPC calls and 16,384 registered objects.
  Overload returns an error. Admission is released during unwinding. A registry
  read clones an `Arc` before analytics runs, so eviction does not invalidate
  an in-flight calculation and nested lookup cannot retain a registry lock.

Exact decimal strings pass to the original DTO parser after bounds checking;
the check does not rewrite financial digits. Simple/Daily compounding for
zero-coupon construction is explicitly unsupported, rather than substituted
with another convention. Poisoned registry locks fail closed. An unavailable
object count is `-1`, not a false empty registry. Unexpected panic envelopes
contain a fixed diagnostic; the host's Rust panic hook may still log locally.

Every input pointer must reference an immutable, NUL-terminated allocation
valid for the entire call. The bounded scan prevents unbounded valid input;
it cannot validate dangling pointers or reads outside a caller allocation.
Free Rust-owned outputs exactly once with `convex_string_free`; never free
borrowed diagnostics/static version pointers. Safety contracts on each export
and unsafe block follow Rust's FFI and C-string requirements.

## Review and exceptions

`exceptions.json` requires scope, rationale, mitigations, owner, review date,
expiration and tracking task. An exception lasts at most 90 days. An expired,
future, malformed or duplicate exception rejects assurance. Exceptions do
not allow network/database runtimes in pure crates, invented financial
results, wider financial tolerances, or skipped required checks. Owners are
declared maintenance assignments, not evidence of independent human approval.

`numeric-evidence.json` pins comparator code/workflow, fixtures and the
existing numerical test modules. Changes to those bytes fail the normal
check. Refreshing/removing an existing pin requires a version increase and
a declared independent quantitative review against the PR base. Changing
an existing pinned fixture also requires that review, even if its hash has
been refreshed. New product evidence must describe units, settings, reference
version and justification; a hash is not proof of quantitative accuracy.

CVX-040 adds eight pins for the offline runner, pure comparison adapter,
catalog, retained pack, source/license record and pending ORE/ISDA/exchange
templates. [The comparison contract](../reconciliation/COMPARISON.md) adds
per-measure native units, complete contexts, future-known input rejection,
evidence-scoped expiring mismatch records and a base guard that forbids wider
absolute/relative bounds or removal of existing compared cases/measures.
Tightening or refreshing existing evidence still requires independent review.
The CVX-040 replay retained 105 failed measures. CVX-017 repaired the shared
bond quote path; the fresh replay passes all 392 unchanged retained measures.
The earlier failed captures remain evidence.
Comparator capture and library/production certification remain explicit.

CVX-053 adds a separate [library coverage inventory](LIBRARY-COVERAGE.md):
30 mandatory areas, 174 scoped combinations and 2,154 operation/surface
records, with inspected source pins and owning implementation tasks. Assurance
checks this public inventory and its base conservation; its tooling and
metadata require exact candidate impact review. CI requires coverage rejection
tests and a pure standalone consumer with default, no-default and schema
features. Metadata/source inspection cannot award `Verified`; LB0/LB1 remain
explicitly incomplete. Existing quantitative pins and tolerances are unchanged.

CVX-054 adds five new [shared-primitives](CASHFLOW-PRIMITIVES.md) artifact pins
while preserving all 55 previous rows, bytes and solver defaults. New fixtures
state exact supplied contracts, reference identities, units and analytic Decimal
bounds; no independent quantitative approval or market pack is inferred. CI
requires the pure consumer in default, no-default and schema builds. Checked
bond construction validates schedules up front. Older infallible/date-only
compatibility routes retain registered preconditions; full product/schema
migration and LB0/LB1 certification remain owning tasks. Exception expirations
remain 2026-12-31.

CVX-056 adds five [checked-curve](CURVE-CONTRACTS.md) artifact pins while
preserving all 60 earlier rows, bytes, expectations and solver defaults.
The new native API supplies full helper economics, explicit query conventions
and immutable dependencies. It implements direct DF/zero/survival curves,
an exact dated deposit/FRA chain and a continuous zero spread; full product
and global calibration remains explicit owning work. Forty-four new tests,
including 256 deterministic generated cases, exercise supplied contracts.
CI requires its standalone default, no-default and schema-feature consumers.
The workspace enables serde_json `float_roundtrip` to preserve original f64
quotes; locked package versions and normal dependency scope remain unchanged.
Actual host/parser compatibility checks do not certify new schema dispatch.
All library records remain Planned. The 105 failures at that revision remain
historical evidence; the current retained replay is recorded below.

CVX-060 adds [numerical fitting and transformation](CURVE-NUMERICS.md) candidates
while preserving all 65 earlier quantitative rows/bytes/defaults. The new dense
scaled solver admits at most 128 parameters/observations and 10,000 attempts;
each SVD has a separate positive bounded iteration count. Native thresholds and
weighted rank are mandatory. Exact immutable representation/roll/spread views
have a 32-level depth ceiling across calls. Stable log/exponential conversions
improve checked short-period paths without changing older numerical kernels.
Four new analytic test/fixture/consumer pins and mandatory consumer feature
checks do not certify pending products, market packs, host dispatch or LB0/LB1.

`change-reviews.json` ties engineering impact to a base commit and candidate
file hashes, with actor, rationale, review date and test evidence. It includes
untracked candidate modules and detects changes after the review. An old
record cannot authorize a new candidate. For each coding task:

1. Select the actual PR base and review the diff and relevant inventory entries.
2. Record the engineering impact and exact candidate hashes after the final edit.
   Update affected inventory entries deliberately; normal checks never refresh
   hashes. If an existing quantitative artifact changes, obtain independent
   quantitative review and record its evidence; an agent cannot attest that
   human approval occurred.
3. Run both `python3 tools/assurance/check.py` and
   `python3 tools/assurance/check.py --base <base-sha>`, plus the applicable
   checks below. Keep actual failures and unexecuted checks in task evidence.

The initial impact record covers CVX-039 against its immediate parent commit.
Before opening a PR for the whole extension branch, add an impact record against
that PR's actual base; earlier branch changes are not implicitly authorized by
the task record.

The workflow validator checks the committed CI structure and exact required
commands, rejects conditional/skipped checks and ignored exits, and permits
only the PR change-impact step to be conditional. It is a narrow structure
check, not a general YAML parser or an authorization system. Repository files
cannot authenticate declared reviewer identities or configure GitHub branch
protection. Administrators must require these checks and independent review
on protected branches; this task does not claim that remote policy is active.

## Executable verification

```sh
python3 tools/assurance/check.py
python3 tools/assurance/check.py --base <base-sha>
python3 -m unittest discover -s tools/assurance -p 'test_*.py'
python3 tools/library/check.py --base <base-sha>
python3 -m unittest discover -s tools/library -p 'test_*.py'
cargo run --locked -p convex --example inventory_probe
cargo run --locked -p convex --example curve_contracts
cargo run --locked -p convex --example curve_contracts --no-default-features
cargo run --locked -p convex --example curve_contracts --features schemars
cargo run --locked -p convex --example curve_numerics
cargo run --locked -p convex --example curve_numerics --no-default-features
cargo run --locked -p convex --example curve_numerics --features schemars
cargo run --locked -p convex --example optional_bonds
cargo run --locked -p convex --example optional_bonds --no-default-features
cargo run --locked -p convex --example optional_bonds --features schemars
python3 reconciliation/comparison.py check --base <base-sha>
python3 -m unittest discover -s reconciliation -p 'test_comparison.py'
cargo fmt --all -- --check
cargo test --locked -p convex-math -p convex-core -p convex-ffi
cargo test --locked -p convex-curves -p convex-bonds -p convex-analytics
cargo clippy --locked --all-features --workspace -- -D warnings
cargo check --locked -p convex -p convex-core -p convex-math -p convex-curves -p convex-bonds -p convex-analytics -p convex-portfolio --no-default-features
cargo test --locked -p convex-math -p convex-core -p convex-ffi --all-features
cargo test --locked -p convex-engine --test graph_regressions
cargo build --locked --profile excel -p convex-ffi
python3 tools/assurance/abi_smoke.py
```

The assurance tests include deterministic 512-case root/tridiagonal/date
properties, arbitrary floating-point patterns, arbitrary bytes and JSON
mutations through real FFI exports, and synchronized registry/admission
concurrency. The saved proptest seed reproduces a premature root-stop defect.
The ABI smoke script loads the optimized cdylib in its own consumer process.
These are bounded fuzz/property campaigns, not coverage-guided fuzzing or a
soak certificate. CI also retains full workspace tests, docs, strict Clippy,
format and the deliberately enforced production probes.

Where supported, an additional interpreter target is:

```sh
cargo +nightly miri test -p convex-ffi --lib registry::tests
```

This requires a Miri-enabled nightly and a compatible build-script setup;
an unavailable command is not a pass. Sanitizers, longer coverage-guided fuzz
campaigns, independent comparator refresh, distributed state exploration,
allocation profiling and live host/transport certification require separate
recorded runs. None is inferred from the default unit suite.

References: [Cargo metadata](https://doc.rust-lang.org/cargo/commands/cargo-metadata.html),
[Clippy unwrap scope](https://rust-lang.github.io/rust-clippy/master/index.html#unwrap_used),
[Rust FFI](https://doc.rust-lang.org/nomicon/ffi.html),
[C string safety](https://doc.rust-lang.org/std/ffi/struct.CStr.html).

CVX-059 adds [checked optional-bond model/calibration/pricing](OPTIONAL-BONDS.md)
and five new engineering pins while preserving all 78 earlier artifact rows,
bytes, expectations, tolerances and solver defaults. Actual-dated normal and
shifted-lognormal swaption calibration holds mean reversion fixed and reports
native residuals/identification. Fixed/stepped/amortizing notice branches and
aligned term-FRN stochastic reset ratios share the supplied BondContract route.
Independent analytic ZCB, dated swaption, notice, FRN and per-bump risk references
exercise positive, boundary and explicit failure cases. Price and sensitivity
refinement are independent postconditions; undersampled 1bp option-risk requests
fail. The facade example runs in default/minimal/schema profiles without adding
dependencies or v1 host dispatch. The CVX-059 state had 100 critical source files,
83 quantitative pins and 43 candidate observations with 113 library source pins.
All 2,154 mandatory records remain Planned; unsupported model combinations,
small-bump option risk, full market/host/lifecycle and independent certificates
remain pending. No low-latency or production/library certificate is inferred.


CVX-018 adds pure supplied fixed/term IRS and fixed/overnight OIS contracts,
independent exact-version calendars, actual reset/value/observation intervals,
profile steps, eligible-history preflight and signed expected coupon cash.
Fifteen new contract tests consume 15 independent dated/cash/factor fixtures
and 128 deterministic date variations. The new facade consumer executes without
external services in default/minimal/schemars profiles. The new rates package
uses existing locked normal dependencies; all prior 83 quantitative artifact
rows/bytes/defaults and 25 C exports remain unchanged. Three new engineering
artifact pins bring the total to 86; critical sources are 104 and the library
inventory has 121 source pins and 44 observations. [The guide](RATES-CONTRACTS.md)
states actual units, bounds, callback and unsupported-economics contracts.
Discounted pricing, accrued, fair rates, multi-curve calibration/rebuild, risk,
lifecycle, host dispatch and independent market/library/production/latency
certificates remain owning work. All 2,154 mandatory records remain Planned.

CVX-019 adds [native multi-curve swap valuation and full-contract fitting](SWAP-VALUATION.md).
Actual independent payment/reset/observation schedules, published fixes,
amortizing and stepped coupons feed a single signed pricer. Product fitting
composes exact dependencies through bounded Brent/least-squares kernels and
retains original native quote units, tolerances and final helper cash. OIS
discount fitting requires an explicit separate zero-basis index projection
view. The core overnight prefix validates represented weights without shifting
the valuation cutoff a second time. The common price bridge checks actual
consumed revisions and its existing 256-record manifest ceiling.

Eighteen new native tests and twenty independent manual Decimal(60) reference
cases are engineering evidence, not a captured ORE/QuantLib comparison. AC2 is
pending because a comparator runtime is unavailable; CVX-019 is not in the
accepted harness set. Nine additional artifact pins preserve all prior 86 rows,
bytes, expectations, tolerances and solver defaults. Critical sources are 108;
the v9 library inventory records the new native route without changing any
mandatory record's status, economics, surfaces or owner. Existing external lock
identities/checksums and all 25 C exports are preserved. Only local dependency
edges are added. Host/schema, calibrated quote risk, lifecycle, market packs,
independent quantitative approval and production/latency gates remain pending.

CVX-021 adds [pure CDS calibration and risk inputs](CREDIT-CALIBRATION.md).
Piecewise and globally identified fits use the full native pricer with original
native quote tolerances, exact helper/dependency/calendar versions and final
repricing. Central rate, running quote, upfront and recovery risk retain distinct
signed units and response semantics. Immediate default cash is conditional on
supplied auction data; unknown recovery stays Pending. Exact legal bond/CDS
mapping and independently supplied residual basis/liquidity inputs are mandatory.

The new actual original ISDA 1.8.3 capture supplies 23 deterministic engineering
scenarios and 276 valuations, with 1,150 native numeric comparisons at bounds
declared before execution: hazard 1e-9, survival 1e-10, upfront 1e-10 and PV 1e-3
currency; sensitivity 0.1 currency absolute plus 1e-6 relative. Eleven new native
tests include mixed quote units, actual rank/identification failure, 65 seeded
nonflat/zero-hazard cases, signed bump stability, default cash and mapping failures.
Ten comparator controls reject missing/mislabeled/nonfinite/reordered results
without relying on Python assertions. Offline CI executes a fresh native adapter
and always uploads its report and raw output. This supplements the unchanged
original 7,164-case/35,820-measure CDS grid and earlier swap comparisons.

Preserve all 128 earlier quantitative artifact rows/bytes/defaults and all 2,154
Planned library requirements. The only new normal dependency is the local
analytics-to-credit edge; external Cargo identities/checksums and all 25 C exports
are unchanged. Resource limits of 128 helpers/parameters, 8,192 actual helper
valuations, 256 curves, 150-year horizons and shared bounded solvers are not a
latency certificate. Actual source/command/raw output evidence is engineering
review, not independent human quantitative approval or a complete market/model,
host, lifecycle, library, production or colocation certificate.

CVX-022 adds [dated futures and cash reference inputs](FUTURES-REFERENCE.md),
with strict disjoint bond/rate terms, exact contract/basket/security revisions,
dated tick/multiplier units and explicit source-quality/freshness gates. Pure
core futures code forbids unsafe and denies unwrap/expect/panic. Existing
memory/redb ports retain atomic corrections, known-at replay and exact receipts;
the approximate advisor always labels proposals indicative only.

Eight new engineering artifact pins preserve all 144 earlier quantitative rows,
bytes, expectations and solver defaults. The published CME 2026-09-24 workbook
supplies ten actual December ZN factors/identities, with original capture bytes
and cell-level extraction evidence. Calendar/registration assumptions and
historical-only eligibility remain explicit. Exact reference/invoice/tick/rate
checks and 512 deterministic Decimal control cases are engineering evidence,
not futures valuation, CTD, actual market, host or quantitative certification.
CI requires new import/replay/gate tests, their strict Clippy checks and all
three facade feature consumers. The critical inventory now covers the pure
futures domain, reference admission/backends and indicative advisor paths.
All 2,154 mandatory library records remain Planned; dependencies and all
25 C exports remain unchanged. CVX-023 owns full futures valuation/risk.

CVX-023 adds [supplied futures/cash valuation and risk](FUTURES-VALUATION.md)
inside the existing pure analytics library. Full fixed-bond carry and independent
CTD criteria retain actual security cash, published CF, explicit funding/haircut/
coupon treatment and all alternatives. Weighted overnight/term rates retain due
history before forecasts, explicit first-publication rules, known/unfixed risk
and separate final precision. Normal monetary-level settlement rounding, exact
ticks, integer residuals and supplied roll facts never post cash or authorize
orders. All results remain indicative; deterministic delivery optionality, margin
financing, cash carry estimates and raw risk bases retain their limitations.

Five new production modules forbid unsafe and deny unwrap/expect/panic, with
bounded inputs and checked arithmetic. Dedicated tests compare independent
Decimal cash/carry and half/base/double risk scenarios, published hypothetical
CME SOFR settlements and margin examples, and failure/lineage controls. New
artifact pins are additive to all152 existing quantitative rows/bytes/defaults;
no existing quantitative result or tolerance is changed. Strict CI requires
new tests/Clippy and all three facade feature consumers. All2154 mandatory
records remain Planned; no human, market/model/host/library/production/latency
certificate or complete optional-delivery model is inferred. Pure dependencies,
manifests/lock and all25 C exports remain unchanged.

CVX-057 adds [complete supplied mixed rate calibration](RATE-CALIBRATION.md),
including full dated deposit/FRA/futures/IRS/OIS/basis helpers, independent exact
discount/index versions and deterministic dependency rebuilds. The two-leg basis
composition reuses the existing floating-leg engine. Curves retain original
native futures price points without assuming a FRA conversion or convexity sign.
Full graph history preflight, publication freezing, actual horizons, positive
native scales/weights, rank/iteration/work bounds and final original repricing
remain fail-closed. Linear piecewise and Linear/CubicSpline per-curve global
fits are explicitly supported; cyclic simultaneous fits are unsupported.

New fixture/test/consumer/builder pins are additive to all157 earlier numerical
rows, bytes and solver defaults. Independent Decimal(60) synthetic cash/quotes
cover nonflat positive/negative multi-curve graphs and separately derived
arithmetic/single-term futures. Dedicated rebuild/failure tests, strict source
and consumer checks, required curve/rates/math suites and all three facade
feature consumers are enforced by CI. This is scoped engineering evidence;
all2154 mandatory records remain Planned, with full model/market/host/lifecycle/
quantitative/library/production/latency certificates pending. No dependency,
manifest, Cargo lock or C export is added.


CVX-058 adds [full supplied cash curves and dated repo carry](CASH-CURVES.md).
Original government cash/bill/par quotations, residual issuer basis and true
repo loan factors reuse full BondContract economics and shared bounded solvers.
Declared reference-only versus credit-survival/immediate-par-recovery models
keep recovery and residual basis separate. Exact role/model/graph preflight,
full original native-unit repricing and government/CDS dependent rebuilds
return all-or-error outputs. Dated carry retains separate haircut and coupon
funding with actual payment/delivery dates.

Five new reference/test/consumer pins append to all162 earlier numerical rows,
bytes and solver defaults. Independent Decimal(60) references include nonflat
positive/negative government/repo curves, stepped/amortizing/par cash and
separate promised/recovery PV. CI requires read-only reference verification,
focused failures/rebuilds, strict affected all-target Clippy, required curves/
analytics/credit suites and three facade feature consumers. The scope has145
critical sources and167 quantitative pins. No dependency, manifest, Cargo lock
or C export is added; all2154 mandatory records remain Planned and independent
human/market/model/host/library/production/latency certification remains open.

CVX-024 adds [signed quote risk and portfolio coverage](QUOTE-RISK.md). Complete
factor keys retain original helper or actual grid coordinates, native units,
conventions/model revisions and held versus dependent-rebuilt response. The
reference route keeps five actual currency PVs, exact signed quantity applied
once, Central normalization and an explicit versioned half-step stability
policy. Full rate/CDS/cash graphs rebuild every original helper at unchanged
native tolerances with frozen calibration history/publications. Optional OAS
requires the existing model resolution and separate duration/convexity refinement
at both bump sizes; scalar price convergence cannot bypass those controls.

Expected trade manifests retain missing factor outcomes, unknown gross exposure,
per-currency coverage and exact same-snapshot FX pins. Partial monetary sums stay
Incomplete, all-missing values stay absent, and distinct quote bases/units never
net. Quote bucket membership and actual node-grid completeness are required
before explicit bucket/parallel checks. Bounds, independent Decimal full-cash
references, native cross-product tests, coverage failures and four standalone
consumers are mandatory CI checks. Seven new quantitative artifacts append to
all167 prior rows/bytes/defaults. No dependency, manifest, lock or C export
changes; all2154 mandatory records and independent certificates remain open.

CVX-061 adds [standalone valuation, inverse and mixed-book workflows](STANDALONE-WORKFLOWS.md).
The closed pure facade composes full native construction, resource/fixing
requirements, cash projection, pricing, checked inverses, full scenarios and
signed quote risk. Futures mark levels, economic PV and expected versus posted
cash remain distinct. NonUnique, NoRoot, missing inputs, NotApplicable and
unsupported model routes are explicit; original numerical/model guards remain
mandatory. The mixed-book example builds actual rate/cash/CDS curves and shows
complete and incomplete coverage, pinned FX and an indicative integer hedge.

Six new quantitative artifact pins append to all174 previous rows/bytes/defaults.
Independent Decimal(60) full cash/yield/Z references, eleven focused workflow
tests, two facade integration tests, required product suites and three feature
consumers are mandatory CI checks. Six workflow modules enter critical source
review. The facade adds one existing local `convex-portfolio` dependency; all
nine pure roots remain provider/runtime/storage neutral. No new external
dependency or C export is added. Mandatory library records and independent
quantitative/market/model/host/production/latency gates remain open.

CVX-062 adds scoped pure native lifecycle transitions and original earned/due cash selectors. [Lifecycle semantics](LIFECYCLE.md) distinguish proposed obligations, recorded receipts, pending auction/recovery and actual futures cash/security delivery. Exact new synthetic cash controls supplement every preserved prior numerical pin and mandatory Planned record; they award no independent quantitative, market/model/host/library/production or latency certificate.

CVX-063 adds [full supplied-term JSON/C ABI and generic clients](LIBRARY-API.md).
Thirteen pure dispatcher modules and three schema modules enter critical review;
the C export count becomes 26. Optional schema dependencies use existing locked
packages; all nine pure roots retain the reviewed normal closure. Eleven new
engineering request/adapter/schema pins append to all 187 original numerical
rows/bytes/defaults. Fresh native parity outputs are transport controls, never
financial oracles. Full schemas, strict ingress, actual adapter tests, three
feature consumers and capability mutation checks are mandatory CI checks.
Required operations, unsupported models and unavailable hosts remain explicit;
AC2 and independent quantitative/library/production/latency gates stay open.

The CVX-063 operation continuation adds checked date/tenor and compounding
conversions plus computed native quote/node bucket reconciliation to the same
dispatcher. Exact roll/representation/spread views retain original native
recipes. All 198 previous numerical/transport rows, bytes and defaults remain
unchanged; twelve supplemental input/consumer/adapter controls are additive,
bringing the total to 210. No native financial source, Cargo manifest/lock,
dependency or C export changes. Original 2,154 Planned requirements and historical
evidence are preserved. Mandatory supplemental schema/adapter/feature checks and
operation-map mutation controls supplement the original checks. Available host
execution is recorded separately from unverified Windows/Excel/browser gates;
no independent financial or production certificate is inferred.

The CVX-063 cash continuation adds pure full-security quotation/invoice and
actual-security government spread, with explicit dated yield and actual-maturity
selection. Original conversion, cash, inverse and quantity guards remain in
their native owners. Two new pure modules enter critical review, bringing the
critical inventory to 183 files;26 C exports and the locked dependency closure
are unchanged. Thirteen new supplied-input, independent Decimal-reference and
adapter/consumer controls append to all 210 previous artifact rows/bytes/defaults,
bringing the total to 223. Six new mandatory cash-reference/adapter/feature/schema
CI checks supplement the existing checks. The 34 independent synthetic values
were declared before native execution; fresh host outputs remain transport
controls. Four new exact routes and64 supplied requests do not promote any of
the original 2,154 Planned records. CVX-063 and unavailable host/full-market
certificates remain open.

Base-commit quantitative comparison also checks Git's declared checkout bytes,
so an unchanged C# file with required CRLF is compared correctly with its stored
LF blob. Physical artifact hashes remain exact; actual content edits still
require independent quantitative review. A mutation control rejects a changed
price and an unavailable checkout conversion.


Complete original product graph capture, fitted-output/solver/helper diagnostic
comparison and original quote-risk replay are documented in
[the schema-3 replay contract](CALIBRATION-SNAPSHOT-REPLAY.md). Pure native calculations remain provider/storage/MQ/clock neutral.
Existing schema1/2 identities and original numerical controls remain; CVX-063
and all mandatory library/market/model/host/production/latency certificates stay open.


Source pins match exact physical bytes or the exact LF/CRLF representations
under Git's explicitly declared text/eol=crlf checkout policy. The shared
`tools/source_pins.py` verifier also protects original source-test pins recorded
before their first checkout. It rejects mixed endings, undeclared/binary
conversion and every content change. Original pin rows/hashes, raw numerical
JSON, test-section bytes, licenses and solver defaults are preserved; this does
not normalize financial inputs or weaken numerical tolerances.

Complete contextual fixed legal-workout path quotation reuses the same checked
ICMA cash/time/inverse/invoice and original legal enumeration owners. Every
source/path is prepared before numerical evaluation, with explicit date, total
reference-entry and original cash-work reservations. Independent full-path
controls retain original failed answers and closed domains; see
[ICMA workouts](ICMA-WORKOUTS.md). Actual-host and independent certificates stay
open; pure bounds do not constitute latency or allocation qualification.


Contextual ICMA observed/model cash helpers reuse the same checked original cash/time/inverse/invoice owner through complete native quote calibration, dependent five-PV risk and file snapshot replay. All contexts are admitted before fitting; explicit original reference/cash reservations precede numerical valuation, and original native quote plus inverse price residuals remain separate. New declared full financial controls retain raw failures and separate admission records. [The contract](ICMA-CASH-HELPERS.md) gives bounds and scope; actual hosts and independent certificates remain open.


## Complete common-factor hedge columns

The distinct `graph_hedge_risk` operation composes the original full product and
graph revaluation owners into complete bond/CDS/IRS/OIS/rates-futures columns
for the existing hedge aggregator and proposal solver. See
[quantity binding, inputs, controls and remaining limits](GRAPH-HEDGE-RISK.md).
Own-security ICMA yield remains a separate factor. Optional models, credit-survival
cash products and delivery-futures all-member risk remain owning work; actual
managed/browser and production/latency qualification remain open.


Credit-survival and issuer-basis cash now have a distinct full original-owner
`cash_basis_hedge_risk` route. See [cash basis hedge risk](CASH-BASIS-HEDGE-RISK.md)
for the complete security/mapping/factor contract, ICMA and rebuild controls,
actual mixed-book scope and remaining recovery/host/production qualification.


Mixed-book full revaluation and model P&L attribution compose the original pure
product and graph owners, retaining every delivery CTD switch and separate cash
PV/futures quote-level meanings. See [the supplied-input contract](MIXED-BOOK-SCENARIOS.md)
and run `cargo run --locked -p convex --example mixed_scenario`. Explicit path,
quantity, FX and work admission, complete independent controls and dedicated
CI supplement existing checks; CVX-063 remains partial and uncertified.


The additive `book_pnl` route reconstructs complete causal information sets,
replays original lifecycle/cash facts and retains original nonlinear product
values and all-member CTD switches. Read [CAUSAL-BOOK-PNL.md](CAUSAL-BOOK-PNL.md)
for full inputs, distinct quote/cash bases, actual fills/receipts, attribution,
work bounds and explicit remaining limits. CVX-063 remains partial.
