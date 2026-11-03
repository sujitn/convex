# Government G-spread to price

`convex::cash_quotation::price_government_spread` prices a fixed-rate bond with
dated cash flows from the quoted prices of government securities you pass in.
The `government_spread_price` operation reaches the same function through JSON,
the `convex_library` C export and generic client calls. Providers, storage,
clocks, cash posting and order handling are outside analytics.

"Checked" below means the function validates its input and returns an error
instead of guessing.

The convention is an explicit dated-cash yield. Each benchmark price is inverted
on that security's full entitled cash. Benchmark selection uses either an exact
maturity match or linear interpolation over a closed interval of contractual
maturities. The requested spread is added to the selected annual-decimal yield.
The target is then priced on every entitled coupon and principal payment date,
with the same compounding and settlement-to-payment day count. This is a
quotation composition. No government zero curve, par coupon, nominal tenor or
weighted average life replaces the securities you pass in.

The [CFA Institute discussion of yield measures](https://www.cfainstitute.org/insights/professional-learning/refresher-readings/2026/yield-and-yield-spread-measures-for-fixed-rate-bonds)
supports separating a benchmark yield from a yield spread and keeping the
compounding explicit. The exact/linear selection and the dated-cash convention
are Convex API choices. The source was used as design research. It has not been
run as a comparator, and it does not establish any market-specific Street
convention.

## Inputs

| Field | Required meaning |
|---|---|
| `target.target.security` | Security identity and independent reference version |
| `target.target.terms` | Full `BondTerms`, including independent coupon/notional profiles, separately priced principal allocations, payment delays and ex-coupon rules |
| `target.target.trade_date`, `settlement`, `on_payment` | Purchase state; settlement must match the lag/calendar/adjustment rule |
| `target.calendars` | Independent payment/reset/observation/exercise calendar pins and their resources |
| `benchmarks` | 1..128 `GovernmentSecurityInput` values, each with its full cash helper, quote version, reference version and calendar bindings |
| `reference_date` | Information origin, on or after the trade date and on or before settlement |
| `settings.government` | Benchmark identity, dated yield convention, `BondInverseSettings` and exact/linear selection policy |
| `settings.decimal_spread_tolerance` | Final absolute annual-decimal spread bound in (0,1e-6], separate from price tolerance and parameter stopping precision |
| `quote` | Independent nonzero `quote_version` and `spread` with `DecimalRate` or `BasisPoints` units; a fractional spread lies within +/-1 annual decimal |
| `position` | Signed notional, original-face reference quantity and matching currency |

No observed target price is required or synthesized. The target's reference
version and the spread quote version are distinct, and every benchmark keeps its
own reference and quote versions. Exact Decimal values are strings on the wire.
No security classification, benchmark, calendar, convention or endpoint is
inferred.

All benchmarks are validated and inverted, including those not selected. These
inputs fail:

- A duplicate identity or contractual maturity.
- A wrong benchmark, currency, settlement or entitlement.
- A target outside the interval.
- A missing exact maturity.

Source order is kept, and selection indices refer to that order. The target's
contractual maturity is distinct from any delayed cash payment date.

## Output and postcondition

The output keeps the target, quote, settings and position. For every benchmark
it gives the conversion and yield inverse. It also gives the selected source
indices and day weights, the benchmark yield, the target yield, the
annual-decimal spread and the spread in fractional basis points. `valuation`
holds the computed clean, dirty and accrued prices per 100 face, the unrounded PV
and the full entitled cash. `consideration` scales the dirty price by the signed
notional once. It is separate from ledger cash, settlement fees, posting
precision and execution.

The target pricer then inverts the computed dirty price within the same yield
domain and work budget. Its `yield_inverse` result keeps the iterations, price
evaluations and price residual. `roundtrip_decimal_spread` subtracts the selected
benchmark yield from that solved target yield. `decimal_spread_residual` must
also satisfy the spread bound you declared, so passing the price tolerance alone
is not enough. If the spread postcondition fails, the result is `no_convergence`
with the annual-decimal residual, its unit, the bound and the work done. No price
is returned.

Domain, resource, convention, projection, inverse, Decimal/finite and position
errors propagate. None of the following produces a guessed result,
extrapolation, relaxed retry or partial success:

- A failed unselected security.
- Missing resources.
- Cash that carries no information, or mixed-sign cash.
- Invalid periodic or simple discount bases.
- No root, or exhausted work.

Quotes are accepted when the dirty price is positive and the clean price is
nonnegative. Floating, optional, Street, workout and contextual ICMA yield models
are not part of this fixed dated-cash operation.

## Run and validate

```sh
cargo run --locked -p convex --example government_inverse
cargo run --locked -p convex --example government_inverse --no-default-features
cargo run --locked -p convex --example government_inverse --features schemars
python3 tests/scripts/government_inverse_reference.py
cargo test --locked --all-features -p convex --test government_inverse -p convex-ffi --test government_inverse -p convex-mcp --test government_inverse -p convex-wasm --test government_inverse
```

Independent Decimal60 controls were written before the Rust code was run against
them. Their fixture bytes, price/rate/currency bounds and negative-input controls
are preserved. Results from the Rust and client paths show only that the
transports agree. Not yet covered: Windows/.NET/Excel/Java/JNI/wasm32/browser
execution, and independent market, model and production validation.

The ACT/ACT ICMA version of this calculation is available as
`icma_government_spread` / `icma_government_price`; see
[ICMA government spreads](ICMA-GOVERNMENT-SPREADS.md).
