# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

#### convex-spreads
- G-Spread calculation
- I-Spread calculation
- Z-Spread calculation with curve bootstrapping
- Asset swap spread (par-par and proceeds methods)
- Discount margin for floating rate notes
- OAS framework for callable bonds

#### convex-risk
- Duration metrics: Macaulay, modified, effective, key rate, spread duration
- Convexity: Analytical and effective
- DV01/PV01/PVBP calculations
- Portfolio risk aggregation
- VaR framework (historical and parametric)

#### convex-yas
- Bloomberg YAS-compatible analytics
- Street convention yield
- True yield
- Current yield
- Simple yield
- Money market yields
- Settlement invoice calculations

#### convex-ffi
- C FFI bindings for cross-language integration
- Bond pricing functions
- Curve operations
- Error handling

[Unreleased]: https://github.com/sujitn/convex/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/sujitn/convex/compare/v0.1.0...v0.1.0
[0.1.0]: https://github.com/sujitn/convex/releases/tag/v0.1.0
