# Actual-security G-spread-to-price

`convex::cash_quotation::price_government_spread` prices full fixed dated cash
from actual supplied government-security quotations. The additive shared
`government_spread_price` operation reaches the same pure function through
JSON, the existing `convex_library` C export and generic client calls. Provider,
storage, clock, cash posting and order authority remain outside analytics.

The common convention is an explicit dated-cash yield. Each actual benchmark
price is inverted on its complete original entitled cash; selection uses the
existing exact maturity or closed-interval linear contractual-maturity policy.
The requested spread is added to that selected annual-decimal yield. The target
is priced on every original entitled coupon and principal payment date, using
the same compounding and settlement-to-payment day count. This is a quotation
composition; no government zero curve, par coupon, nominal tenor or weighted
average life replaces the supplied securities.

The [CFA Institute discussion of yield measures](https://www.cfainstitute.org/insights/professional-learning/refresher-readings/2026/yield-and-yield-spread-measures-for-fixed-rate-bonds)
supports separating a benchmark yield and yield spread and retaining explicit
compounding. Convex's exact/linear selection and dated-cash convention are
explicit API choices. That source is design research, not an executed comparator
or an admission of a market-specific Street convention.

## Inputs

| Field | Required meaning |
|---|---|
| `target.target.security` | Actual security identity and independent reference version |
| `target.target.terms` | Full original `BondTerms`, including independent coupon/notional profiles, separately priced principal allocations, payment delays and ex-coupon rules |
| `target.target.trade_date`, `settlement`, `on_payment` | Actual purchase state; settlement must match the original lag/calendar/adjustment rule |
| `target.calendars` | Independent exact payment/reset/observation/exercise calendar pins and supplied resources |
| `benchmarks` | 1..128 actual `GovernmentSecurityInput` values, each with its full original cash helper, quote version, reference version and calendar bindings |
| `reference_date` | Supplied information origin, on/after trade and on/before settlement |
| `settings.government` | Existing benchmark identity, dated yield convention, original `BondInverseSettings` and exact/linear selection policy |
| `settings.decimal_spread_tolerance` | Explicit final absolute annual-decimal spread bound in (0,1e-6], separate from price tolerance and parameter stopping precision |
| `quote` | Independent nonzero `quote_version` and `spread` with `DecimalRate` or `BasisPoints` units; fractional spread lies within +/-1 annual decimal |
| `position` | Exact signed notional, original-face reference quantity and matching currency |

No observed target price is required or synthesized. Target reference and spread
quote revisions are distinct; every benchmark retains its own original reference
and quote revision. Exact economic Decimal values use strings on the wire.
No security classification, benchmark, calendar, convention or endpoint is inferred.

All original benchmarks are validated and inverted, including unselected quotes.
Duplicate identity or contractual maturity, wrong benchmark/currency/settlement/
entitlement, an out-of-interval target or missing exact maturity fails. Source
order is retained; selection indices refer to that original order. A target's
contractual maturity remains distinct from any delayed cash payment date.

## Output and acceptance

The output retains original target/quote/settings/position, every benchmark's
complete conversion and yield inverse, selected source indices and day weights,
benchmark yield, target yield, annual-decimal spread and fractional basis points.
`valuation` contains the original computed clean/dirty/accrued per100 face,
unrounded PV and complete entitled cash. `consideration` scales that original
dirty price by the signed notional once. It is separate from ledger cash,
settlement fees, posting precision and execution authority.

The same original target pricer then inverts the computed dirty price within
the unchanged supplied yield domain and work budget. Its complete `yield_inverse`
retains actual iterations, price evaluations and native price residual.
`roundtrip_decimal_spread` subtracts the same selected benchmark yield from that
solved target yield. `decimal_spread_residual` must also satisfy the independently
declared spread bound. Price acceptance alone cannot bypass this postcondition.
A failed decimal spread postcondition returns `no_convergence` with its explicit
annual-decimal residual unit, bound and actual work; it returns no accepted price.

Original domain, resource, convention, projection, inverse, Decimal/finite and
position guards propagate. Failed unselected securities, absent resources,
uninformative/mixed-sign cash, invalid periodic/simple discount bases, no root
or exhausted work never trigger a guessed result, extrapolation, relaxed retry
or partial success. Positive dirty and nonnegative clean quote admission remains.
Floating, optional, Street, workout and contextual ICMA yield models retain
their separate owners and are not implied by this fixed dated-cash operation.

## Run and validation

```sh
cargo run --locked -p convex --example government_inverse
cargo run --locked -p convex --example government_inverse --no-default-features
cargo run --locked -p convex --example government_inverse --features schemars
python3 tests/scripts/government_inverse_reference.py
cargo test --locked --all-features -p convex --test government_inverse -p convex-ffi --test government_inverse -p convex-mcp --test government_inverse -p convex-wasm --test government_inverse
```

Independent Decimal60 controls are declared before candidate execution. Their
exact fixture bytes, price/rate/currency bounds and negative-input controls are
preserved. Fresh native/client results establish transport agreement only.
Actual Windows/.NET/Excel/Java/JNI/wasm32/browser qualification and independent
market/model/library/production/latency certificates remain open. CVX-063 remains
partial, unaccepted and uncertified; mandatory library records remain Planned.
