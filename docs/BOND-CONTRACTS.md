# Supplied bond terms and cash

CVX-055 adds `convex::bond_contracts` and `convex::bond_contract` over the shared
core schedule, dated profile, principal and fixing primitives. One immutable
`BondContract` composes fixed/stepped/amortizing, plain term/overnight floating
and fixed-price call/put terms. Its expected cash is separate from realized
payments and from contingent option valuation. No provider, database, broker,
server, global evaluation date or system clock participates.

```sh
cargo run --locked -p convex --example bond_contracts
cargo run --locked -p convex --example bond_contracts --no-default-features
cargo run --locked -p convex --example bond_contracts --features schemars
cargo test --locked -p convex-bonds --test bond_contracts
cargo test --locked -p convex-analytics --test bond_contracts
```

The example prepares stepped coupon/principal cash, constructs distinct
immutable discount/projection curves, prices fixed and floating cash, and
checks that a missing published fixing fails. These supplied contracts are
engineering evidence, **not independent quantitative or market certification**.

## Construction and reconstruction

`BondTerms` is the full version-1 economic input, also reexported by
`convex_analytics::dto`. It retains issue versus dated dates, anchored coupon
schedule/stubs/references, independent date rules, calendar identities,
settlement, all four independently dated profiles, term resets or overnight
mechanics, principal prices/allocations, redemption, rounding, negative-coupon
and ex-coupon choices, and complete supported exercise terms.

Serialize `bond.terms()` using Serde. Deserialize `BondTerms`, then call
`terms.build()` or `terms.build_with_calendars(&resources)` to reconstruct a
checked native object. Unknown terms/styles/versions fail. Precomputed schedules
and projected cash cannot be deserialized into a checked bond. Native/JSON
roundtrip preserves supplied economics; it does not certify a host binding.
The `schemars` feature consumer proves feature compatibility, not generated
schema coverage for these new types.

`build` resolves checked compiled calendar names, including bounded joint
calendars. `build_with_calendars` consumes actual separate payment/accrual,
reset, observation and exercise calendars, without lookup or storage. The
embedding caller must bind these values and identities to exact calendar
versions in its immutable information set. Calendar-name JSON alone does not
carry a dynamic calendar's holiday data/version or establish historical replay.

`FixedRateBond::contract_terms`, `with_profiles` and `with_amortization`,
`FloatingRateNote::contract_terms`, `SinkingFundBond::contract_terms`/
`try_contract`, `CallableBond::contract_terms` and
`CallableFloatingRateNote::contract_terms` reach this same composition. The
floating adapter requires explicit index/reset or observation mechanics and
checks conflicts with its native fixing/overnight fields. An application maps
the native index to its internal IndexId; no vendor identifier is inferred.
Older infallible flat-rate/workout helpers retain their compatibility
preconditions and qualification; they are not publication-aware valuation.

## Coupon and principal meaning

Profiles are contiguous half-open `[start,end)` intervals. Coupon, spread,
gearing and notional step independently. Fixed coupons require zero gearing.
Term effective rate is deterministic coupon + gearing × resolved index +
annualized decimal spread. A calculation splits at economic/reset boundaries
and uses each actual coupon's day count/reference periods; it never substitutes
an averaged coupon, index or factor. Native cash uses checked Decimal arithmetic.

Original face and initial outstanding principal must agree. Principal is
nonincreasing in this initial bond contract; advances/prepayment models are
unsupported. `outstanding_principal(date, Before/After)` makes same-date
reduction inclusion explicit. After full redemption it is zero; a delayed
payment remains an outstanding entitled receivable in the cash projection.
Profile coverage equals actual accrual coverage. Its independently specified
principal effective boundaries use the supplied payment date rule; these do
not assert cash was paid.

Coupon principal is explicitly `AccrualProfile`, `BeforeCouponBoundary` or
`AfterCouponBoundary`. Pre/post selection uses the **contractual coupon end**,
not its delayed payment. After the final boundary the latter convention has
zero coupon principal. A profile lacking that contractual boundary fails.

Internal reductions pay at par unless explicitly listed in `principal_prices`.
The final residual uses the explicit 0..10 redemption multiplier. A priced
allocation retires its absolute amount at its own multiplier and returns the
remaining cash as a separate component at par/final residual price. There is no
averaged redemption price. The original sinking schedule's percentages are
converted against original face, with dates, totals and prices validated;
delivery, acceleration and optional sinks fail this deterministic cash route.

`AmortizationSchedule::notional_profile` admits actual deterministic entries
without sorting or clipping. Supplied remaining factors must reconcile within
1e-12 (the old input type stores f64). Pass-through/prepayment is unsupported.
Use Decimal `BondTerms` profiles/prices for full contractual precision; the
older f64 construction helpers cannot recover precision already lost on input.

Cash rounding is explicit and occurs once after coupon/reset segments sum, once
per principal allocation, and once per coupon accrued/rebate. Unrounded Decimal
has finite precision (28 fractional places, 96-bit coefficient). The reference
fixtures retain 50-digit expectations; the native reader representation differs
by less than the declared 1e-22 absolute cash comparison bound. Existing
quantitative values/tolerances and solver defaults remain unchanged.

## Fixings and overnight coupons

`fixing_requirements` returns the actual unpaid reset/observation dates and
requires caller-supplied administrator publication instants. Term reset grids
are independent of coupon/payment frequency. Their complete actual intervals
must cover coupon accrual; the forecast receives the actual reset interval,
index and output day count even when the coupon uses only part of that reset.
ICMA term projection quotation needs an independent reference convention and
fails the current default curve policy; coupon ICMA references remain supported.

`project` consumes a supplied `BondProjectionContext`, immutable `FixingSeries`,
publication callback and explicit `BondProjectionPolicy`. It resolves the
**entire missing published history list before any forecast callback**. Core
resolution uses batches of at most 4,096; an already-due missing fixing cannot
become zero, a current-rate estimate or a forecast. Highest eligible revisions,
publication/known-at cutoff, preliminary/final policy, FutureObservation and
AwaitingPublication origins remain visible. Future projections never rewrite
known coupon amounts or accrued cash.

`RequirePublished` refuses future/permitted unpublished observations.
`Supplied` calls only the permitted pure projection policy; errors propagate.
`project_from_curve` provides an explicit simple-forward policy on a checked
immutable curve. It verifies currency/index/projection role and queries the
actual reset/overnight interval/day count. A discount curve cannot substitute
for an index projection curve. Custom policy source/version lineage belongs in
the embedding input set; the generic callback does not invent curve provenance.

Overnight coupons compound the actual daily index factors, apply the constant
coupon-period gearing, and add simple deterministic coupon/spread cash outside
compounding. The prepared lookback/observation-shift/lockout and weekend/holiday
weights come from shared core. Rate intervals are preserved, including full
weekend projection intervals. Lockout reuses its actual preceding observation
and does not regenerate requirements or rates. No cap/floor is inferred.

Intracoupon principal/gearing changes require an explicit segmented compounding
contract and return Unsupported here. Coupon-boundary changes and independently
dated simple coupon/spread steps are supported. Forecast clipping is not an
option valuation. Caps/floors require their model and are refused by the plain
floating adapter; unknown cap/floor fields are refused by the new strict schema.

Unshifted overnight accrued uses the actual prefix product, including partial
weekend weights, rather than prorating a future full coupon. Shifted partial
accrual requires an explicit `OvernightAccrualCutoff` in prepared weight dates;
there is no universal inverse mapping for nonbusiness settlement. Missing,
unused, duplicate or out-of-range cutoffs fail. Full accrued during payment
lag uses the full actual product. Ex-coupon treatment then subtracts the full
unrounded coupon or returns zero according to the supplied contract.

## Entitlement and valuation

`fixed_projection` and `project` return still-unpaid coupon amounts/origins,
actual accrued, outstanding principal and entitled principal cash. Coupon
entitlement, ex-coupon accrued and principal entitlement are separate. Payment
inclusion is explicitly pre/post payment state. Mature post-payment instruments
return empty cash/requirements, not an assumed realized cash ledger.

`bond_contract::price_fixed` and `price_floating` discount entitled actual cash
with settlement-normalized DF ratios. Required currency/collateral/discount role
must match. The result carries actual cash/fixing origins and the exact discount
curve version. PV is absolute contract-currency cash; clean/dirty/accrued are
**per 100 original face**, not an inferred outstanding-factor quotation.
Negative coupon cash and rates are allowed only under the declared policy.

Supported fixed-price calls/puts retain European dates, strictly ordered
Bermudan dates or inclusive American windows, price per 100 outstanding
principal, protection end, notice offset, exercise date, settlement lag/rule
and accrued-price treatment. Same-right provisions must be sorted and
nonoverlapping; call and put rights can coexist. American windows are not
collapsed to coupon dates. `ExerciseTerms::dates` computes supplied exercise
consequences without asserting exercise occurred or choosing it optimally.

Cash projections with options are labelled `conditional_on_no_exercise`.
Plain valuation rejects optional terms instead of pricing them as a bullet.
Make-whole/mandatory/change-of-control/death triggers require additional complete
terms/state/model; unknown styles or unsupported existing provisions fail.
[CVX-059 optional-bond models](OPTIONAL-BONDS.md) provide separate checked
HW1F calibration, fixed-price notice exercise and restricted aligned joint
term-FRN pricing, fractional OAS and resolved/converged factor risk. General
caps/floors, American/make-whole/triggered/overnight option models, small-bump
accuracy and full market/lifecycle/client certificates remain owning work.

## Bounds and qualification

Core limits remain: 4,096 schedule/profile entries, ±366-day offsets, ≤150×366
contract days, principal 0..1e18, annualized rate/spread ±10, gearing ±1e6 and
65,536 supplied fixing revisions. One bond has at most 65,536 reset slices or
overnight observations, and at most 65,536 unique required rates; core history
resolution remains batched at 4,096. Coupon and principal grids retain their
separate core bounds. All arithmetic/curve errors propagate. Bounds are resource
ceilings, not measured latency, allocation or capacity guarantees. Caller
calendar/publication/projection implementations must terminate and stay pure.

Ten new independent supplied Decimal(50) fixtures and native/analytics tests
cover dated steps, partial/priced redemption, long ICMA/leap references, monthly
anchor drift, joint holiday/ex-coupon entitlement, delayed payment state,
negative cash, eligible term revisions, missing history, overnight products and
exercise reconstruction. A 128-case seeded step campaign checks exact analytic
cash/principal conservation. Research references are the [NY Fed methodology](https://www.newyorkfed.org/markets/reference-rates/additional-information-about-reference-rates)
and [ARRC 2019 FRN convention matrix](https://www.newyorkfed.org/medialibrary/Microsites/arrc/files/2019/ARRC_SOFR_FRN_Conventions_Matrix.pdf),
inspected 2026-10-06. These inform declared choices; fixtures use synthetic
inputs and do not claim administrator/market convention certification.

The v1 C ABI/clients and production service do not acquire these new fixing
inputs automatically. Complete schemas/generated schemas and native-to-host
construction/dispatch parity remain CVX-063/033. Service product admission still
uses its reviewed fixed/zero scope. No new symbol, provider, storage or execution
integration is introduced. All 30 mandatory areas, 174 combinations and 2,154
Planned records remain; full model/market, LB0/LB1 and production certificates
are pending. See [library coverage](LIBRARY-COVERAGE.md) and
[assurance](ASSURANCE.md).
