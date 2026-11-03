# convex-math

Numerical routines used by the rest of Convex: root finders, interpolation,
extrapolation, a few linear algebra helpers and a small amount of statistics.

Use it directly if you want these pieces without the bond and curve types, for
example to solve for a yield or to interpolate a set of zero rates.

## Where it sits

`convex-math` has no dependencies on other Convex crates. `convex-core` depends
on it, and the curve, rates, credit, bonds and analytics crates build on top of
that.

## Install

```toml
[dependencies]
convex-math = "0.14"
```

## What's in it

- `solvers`: root finding with `newton_raphson`, `brent`, `bisection`, `secant`
  and `hybrid` (Newton with a Brent fallback). Settings go in `SolverConfig`,
  results come back as `SolverResult`.
- `interpolation`: the `Interpolator` trait with `LinearInterpolator`,
  `LogLinearInterpolator`, `CubicSpline`, `MonotoneConvex`, `FlatForward`, `PiecewiseConstant`, plus
  the `NelsonSiegel` and `Svensson` parametric curves.
- `extrapolation`: `FlatExtrapolator`, `LinearExtrapolator` and
  `UfrConvergence` (forward rates that move toward an ultimate forward rate).
- `optimization`: `golden_section` and `bounded_least_squares`.
- `linear_algebra`: `solve_tridiagonal`, `lu_decomposition` and
  `solve_linear_system`.
- `stats`: `standard_normal_cdf` and `standard_normal_inverse`.
- `decimal`: `BoundedDecimal`, exact decimal accumulation with a fixed size
  limit and a single rounding step at the end.
- `portable`: `exp`, `log`, `pow` and related functions taken from the `libm`
  crate, so results do not depend on the platform's math library.
- `bounds`: the size limits the other modules enforce, such as
  `MAX_SOLVER_ITERATIONS` and `MAX_DATA_POINTS`.

Functions that can fail return `MathResult`, with `MathError` as the error type.
Bad input (non-finite numbers, unsorted points, too many points) is reported as
an error rather than a panic.

## Examples

Solve for the yield that prices a 5-year bond with a 5% annual coupon at 95:

```rust
use convex_math::solvers::{brent, SolverConfig};

let price_error = |y: f64| {
    let coupons: f64 = (1..=5).map(|t| 5.0 / (1.0 + y).powi(t)).sum();
    coupons + 100.0 / (1.0 + y).powi(5) - 95.0
};

let result = brent(price_error, 0.0, 0.20, &SolverConfig::default()).unwrap();
// A bond priced below par yields more than its coupon.
assert!(result.root > 0.05 && result.root < 0.07);
assert!(price_error(result.root).abs() < 1e-6);
```

Interpolate a zero curve between pillars:

```rust
use convex_math::interpolation::{Interpolator, LinearInterpolator};

let years = vec![1.0, 2.0, 5.0, 10.0];
let zeros = vec![0.040, 0.042, 0.045, 0.047];

let curve = LinearInterpolator::new(years, zeros).unwrap();
let zero_3y = curve.interpolate(3.0).unwrap();
assert!((zero_3y - 0.043).abs() < 1e-12);

// Outside the pillar range is an error unless extrapolation is switched on.
assert!(curve.interpolate(15.0).is_err());
```

## Links

- API documentation: <https://docs.rs/convex-math>
- Portable math kernels: <https://github.com/sujitn/convex/blob/main/docs/PORTABLE-NUMERICS.md>

## License

MIT. See LICENSE.
