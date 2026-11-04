# Convex documentation

These guides explain how each part of the library works: what goes in, what
comes out, the conventions used, and the limits. For the API itself, see the
crate docs on docs.rs (start with
[convex-analytics](https://docs.rs/convex-analytics)). For an overview and
install instructions, see the [main README](../README.md).

Most guides list example programs and tests you can run. Run them from the
repository root after `python3 tools/fixtures.py materialize` (see
[Regression test data](TEST-DATA.md)).

## Start here

- [Library API](LIBRARY-API.md): the JSON `library()` request format shared by
  Rust, C, Java, Excel, WebAssembly and MCP.
- [Valuation, inverse and risk workflows](STANDALONE-WORKFLOWS.md): bonds,
  swaps, CDS and futures valued together in one call.

## Bonds

- [Bond terms and cash](BOND-CONTRACTS.md)
- [Bond quotes](BOND-QUOTES.md)
- [Shared cashflow primitives](CASHFLOW-PRIMITIVES.md)
- [Accrued interest for constant-coupon bonds](ACCRUED-METHODS.md)
- [Yield pricing from cash flows](YIELD-PRICING.md)
- [Dated cash discount time](DATED-YIELD.md)
- [Yield scalar measures](YIELD-SCALARS.md)
- [Treasury bill price, yield and quote risk](TREASURY-BILL-QUOTES.md)
- [Reinvestment projection](REINVESTMENT.md)
- [Callable and putable bond models](OPTIONAL-BONDS.md)
- [Term coupon options](FLOATING-COUPON-OPTIONS.md) (caps and floors on floating coupons)

### Workouts and spreads

- [Workout quotations for fixed-rate bonds](WORKOUT-QUOTATIONS.md)
- [Floating-rate workout price and discount margin](FLOATING-WORKOUTS.md)
- [Government G-spread to price](GOVERNMENT-SPREAD-INVERSE.md)

### ACT/ACT ICMA conventions

- [ACT/ACT ICMA quotations](ICMA-QUOTATIONS.md)
- [ACT/ACT ICMA cash helpers](ICMA-CASH-HELPERS.md)
- [ICMA workout quotations](ICMA-WORKOUTS.md)
- [ICMA government spread (G-spread)](ICMA-GOVERNMENT-SPREADS.md)
- [ICMA yield risk and hedge columns](ICMA-YIELD-RISK.md)

## Curves

- [Curve recipes and queries](CURVE-CONTRACTS.md): describing a curve as data
- [Step-zero and flat-forward curve methods](CURVE-METHODS.md)
- [Parametric, segmented and delegated curves](CURVE-FAMILIES.md)
- [Monotone convex curves](CURVE-MONOTONE-CONVEX.md)
- [Explicit UFR convergence](CURVE-UFR.md)
- [Curve fitting, conversions and rolling](CURVE-NUMERICS.md)
- [Mixed rate calibration](RATE-CALIBRATION.md): deposits, FRAs, futures, swaps, OIS and basis
- [Cash curves and dated repo carry](CASH-CURVES.md)
- [Nelson-Siegel and Svensson calibration to product prices](PRODUCT-PARAMETRIC-CALIBRATION.md)
- [Exponential UFR calibration to product prices](PRODUCT-UFR-CALIBRATION.md)
- [Curve recipes in immutable snapshots](CURVE-SNAPSHOT-REPLAY.md)
- [Calibration snapshots and replay](CALIBRATION-SNAPSHOT-REPLAY.md)

## Swaps

- [IRS and OIS contracts](RATES-CONTRACTS.md)
- [Multi-curve swap valuation and calibration](SWAP-VALUATION.md)
- [Swap comparison against QuantLib](SWAP-COMPARISON.md)

## Credit

- [CDS valuation](CDS-VALUATION.md)
- [Credit curve calibration and risk](CREDIT-CALIBRATION.md)

## Futures

- [Futures and cash hedge reference data](FUTURES-REFERENCE.md)
- [Futures and cash hedge valuation](FUTURES-VALUATION.md)

## Risk and hedging

- [Scalar risk and weekend arithmetic](RISK-CONTRACTS.md)
- [Signed quote risk and portfolio coverage](QUOTE-RISK.md)
- [Portfolio key-rate exposure](PORTFOLIO-KEY-RATE-RISK.md)
- [Graph risk and hedge columns](GRAPH-HEDGE-RISK.md)
- [Optional bonds: graph risk and hedge columns](OPTIONAL-GRAPH-HEDGE-RISK.md)
- [Constrained hedge proposals](CONSTRAINED-HEDGES.md)
- [Credit and issuer-basis cash hedge risk](CASH-BASIS-HEDGE-RISK.md)
- [CDS recovery scenarios and a five-factor mixed hedge book](CDS-RECOVERY-HEDGE-RISK.md)
- [Full delivery-futures basket risk and hedges](DELIVERY-FUTURES-HEDGE-RISK.md)
- [Mixed-book scenarios and P&L attribution](MIXED-BOOK-SCENARIOS.md)

## Lifecycle and P&L

- [Contract lifecycle](LIFECYCLE.md)
- [Mixed-book P&L attribution](CAUSAL-BOOK-PNL.md)
- [Cohort P&L: lots, cash entitlements and physical delivery](COHORT-BOOK-PNL.md)

## Engineering

- [Portable numerical kernels](PORTABLE-NUMERICS.md): why results match across platforms
- [Regression test data](TEST-DATA.md)
- [Comparison with QuantLib and the ISDA CDS model](../reconciliation/COMPARISON.md)
- [Project tracking notes](internal/README.md): CI rules, coverage inventory, removed APIs

## Elsewhere in the repository

- Pricing runtime: [crates/convex-engine/RUNTIME.md](../crates/convex-engine/RUNTIME.md)
- Server: [crates/convex-server/README.md](../crates/convex-server/README.md)
- Excel add-in: [excel/README.md](../excel/README.md)
- Java binding: [bindings/java/README.md](../bindings/java/README.md)
- Browser app: [www/README.md](../www/README.md)
- WebAssembly package: [crates/convex-wasm/README.md](../crates/convex-wasm/README.md)
