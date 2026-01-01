# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.11.0] - 2026-01-01

## [0.10.32] - 2025-12-22

## [0.10.31] - 2025-12-22

## [0.10.3] - 2025-12-21

## [0.10.2] - 2025-12-18

### Changed
- Consolidated convex-spreads, convex-risk, convex-yields into convex-analytics crate
- Updated convex-wasm to use StandardYieldEngine for consistent yield calculations

### Fixed
- Fixed f64 to Decimal conversion precision issue in WASM bindings
- Fixed YTM roundtrip inconsistency in price_from_yield calculations
- Fixed missing documentation warnings in convex-analytics

### Improved
- UI input field tracking to preserve user-entered values (YieldAnalysis, SpreadAnalysis, Benchmark components)

## [0.10.1] - 2025-12-07

## [0.1.0] - 2025-12-07

### Added
- Initial release preparation
- GitHub CI/CD workflows
- crates.io publishing configuration

## [0.1.0] - 2024-12-07

### Added

#### convex-core
- Core domain types: `Price`, `Yield`, `Spread`, `Rate`, `Currency`, `Money`, `Notional`
- Date handling: `Date`, `Tenor`, `Period`, `Frequency`, schedule generation
- Day count conventions: ACT/360, ACT/365F, ACT/365L, ACT/ACT (ICMA, ISDA, AFB), 30/360 variants, Business/252
- Business day calendars: US (SIFMA, Federal Reserve), UK, TARGET2, Japan
- Dynamic calendar support with JSON import/export
- Joint calendar support for cross-border transactions

#### convex-math
- Root-finding algorithms: Newton-Raphson, Brent's method, Secant, Bisection
- Interpolation methods: Linear, log-linear, cubic spline, monotone convex
- Extrapolation methods: Flat, linear, Smith-Wilson
- Parametric models: Nelson-Siegel, Svensson
- Linear algebra: Tridiagonal solver, LU decomposition, Cholesky decomposition

#### convex-curves
- Curve types: Discount curves, zero curves, forward curves, par curves
- Bootstrap methods: Sequential, global, and hybrid bootstrapping
- Curve instruments: Deposits, FRAs, futures, swaps, OIS, basis swaps
- Multi-curve framework support
- Curve validation and arbitrage checks

#### convex-bonds
- Government bonds: US Treasuries (T-Bills, T-Notes, T-Bonds, TIPS, FRNs), UK Gilts, German Bunds, JGBs
- Corporate bonds: Fixed rate, callable, putable, sinking fund, convertible
- Municipal bonds: General obligation, revenue bonds
- MBS pass-through securities with prepayment modeling
- Floating rate notes with caps, floors, and collars
- Cash flow generation and schedule management
- Accrued interest calculations with all day count conventions

#### convex-analytics
- Unified analytics crate consolidating spreads, risk, and yield functionality
- Cash flow generation: Schedule, accrued interest, settlement calculations
- Yield calculations: YTM solver, money market yields, current yield, simple yield
- Pricing: BondPricer with industry-standard methodology
- Spread analytics: G-Spread, I-Spread, Z-Spread, OAS, ASW, Discount Margin
- Risk metrics: Duration (Macaulay, modified, effective, key rate, spread), Convexity, DV01
- Portfolio analytics: VaR framework, hedging calculations
- Options: Hull-White model, binomial trees for callable bonds
- Industry-standard: Street convention, true yield, settlement invoice

#### convex-ffi
- C FFI bindings for cross-language integration
- Bond pricing functions
- Curve operations
- Error handling

[Unreleased]: https://github.com/sujitn/convex/compare/v0.11.0...HEAD
[0.11.0]: https://github.com/sujitn/convex/compare/v0.10.32...v0.11.0
[0.10.32]: https://github.com/sujitn/convex/compare/v0.10.31...v0.10.32
[0.10.31]: https://github.com/sujitn/convex/compare/v0.10.3...v0.10.31
[0.10.3]: https://github.com/sujitn/convex/compare/v0.10.2...v0.10.3
[0.10.2]: https://github.com/sujitn/convex/compare/v0.10.1...v0.10.2
[0.10.1]: https://github.com/sujitn/convex/compare/v0.1.0...v0.10.1
[0.1.0]: https://github.com/sujitn/convex/compare/v0.1.0...v0.1.0
[0.1.0]: https://github.com/sujitn/convex/releases/tag/v0.1.0
