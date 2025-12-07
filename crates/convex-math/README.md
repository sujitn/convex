# convex-math

Mathematical utilities for the Convex fixed income analytics library.

## Overview

`convex-math` provides numerical algorithms commonly used in fixed income calculations:

- **Solvers**: Root-finding algorithms for yield calculations
- **Interpolation**: Methods for yield curve interpolation
- **Optimization**: Function optimization for curve fitting
- **Linear Algebra**: Matrix operations for financial calculations

## Features

### Root-Finding Algorithms

```rust
use convex_math::solvers::{newton_raphson, brent, bisection, SolverConfig};

// Newton-Raphson (requires derivative)
let f = |x: f64| x * x - 2.0;
let df = |x: f64| 2.0 * x;
let result = newton_raphson(f, df, 1.5, &SolverConfig::default()).unwrap();
// result.root ≈ √2

// Brent's method (bracketing, no derivative needed)
let result = brent(f, 1.0, 2.0, &SolverConfig::default()).unwrap();

// Bisection (simple but reliable)
let result = bisection(f, 1.0, 2.0, &SolverConfig::default()).unwrap();
```

### Interpolation

```rust
use convex_math::interpolation::{LinearInterpolator, CubicSpline, Interpolator};

// Linear interpolation
let xs = vec![0.0, 1.0, 2.0, 3.0];
let ys = vec![0.0, 1.0, 4.0, 9.0];

let linear = LinearInterpolator::new(xs.clone(), ys.clone()).unwrap();
let y = linear.interpolate(1.5).unwrap();

// Cubic spline (smoother)
let spline = CubicSpline::new(xs, ys).unwrap();
let y = spline.interpolate(1.5).unwrap();
```

### Linear Algebra

```rust
use convex_math::linear_algebra::{solve_tridiagonal, solve_linear_system};
use nalgebra::{DMatrix, DVector};

// Efficient tridiagonal solver (common in spline fitting)
let a = vec![1.0, 1.0];       // Lower diagonal
let b = vec![2.0, 2.0, 2.0];  // Main diagonal
let c = vec![1.0, 1.0];       // Upper diagonal
let d = vec![1.0, 2.0, 3.0];  // RHS

let x = solve_tridiagonal(&a, &b, &c, &d).unwrap();

// General linear system
let a = DMatrix::from_row_slice(2, 2, &[2.0, 1.0, 1.0, 3.0]);
let b = DVector::from_vec(vec![5.0, 5.0]);
let x = solve_linear_system(&a, &b).unwrap();
```

## Performance

- Root-finding: Typically converges in < 10 iterations
- Interpolation: O(log n) lookup with binary search
- Tridiagonal solver: O(n) complexity

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
convex-math = "0.1"
```

## License

This project is licensed under the MIT License - see the [LICENSE](../../LICENSE) file for details.
