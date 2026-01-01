// =============================================================================
// Numerical Solvers
// Brent's method for root-finding, matrix operations for LM
// =============================================================================

import { SolverConfig, DEFAULT_SOLVER_CONFIG } from './types';

/**
 * Result from root-finding
 */
export interface RootResult {
  root: number;
  iterations: number;
  converged: boolean;
  error: number;
}

/**
 * Brent's method for root-finding
 * Combines bisection, secant, and inverse quadratic interpolation
 * Guaranteed convergence with quadratic convergence near root
 */
export function brent(
  f: (x: number) => number,
  a: number,
  b: number,
  config: Partial<SolverConfig> = {}
): RootResult {
  const { tolerance, maxIterations, lowerBound, upperBound } = {
    ...DEFAULT_SOLVER_CONFIG,
    ...config,
  };

  // Clamp initial bounds
  a = Math.max(a, lowerBound);
  b = Math.min(b, upperBound);

  let fa = f(a);
  let fb = f(b);

  // Check if root is bracketed
  if (fa * fb > 0) {
    // Try to find a bracket
    const expanded = expandBracket(f, a, b, lowerBound, upperBound);
    if (expanded) {
      a = expanded.a;
      b = expanded.b;
      fa = expanded.fa;
      fb = expanded.fb;
    } else {
      // Return best guess
      return {
        root: Math.abs(fa) < Math.abs(fb) ? a : b,
        iterations: 0,
        converged: false,
        error: Math.min(Math.abs(fa), Math.abs(fb)),
      };
    }
  }

  // Ensure |f(a)| >= |f(b)|
  if (Math.abs(fa) < Math.abs(fb)) {
    [a, b] = [b, a];
    [fa, fb] = [fb, fa];
  }

  let c = a;
  let fc = fa;

  for (let iter = 0; iter < maxIterations; iter++) {
    // Check convergence
    if (Math.abs(fb) < tolerance) {
      return { root: b, iterations: iter + 1, converged: true, error: Math.abs(fb) };
    }

    if (Math.abs(b - a) < tolerance) {
      return { root: b, iterations: iter + 1, converged: true, error: Math.abs(fb) };
    }

    // Ensure |f(a)| >= |f(b)|
    if (Math.abs(fa) < Math.abs(fb)) {
      [a, b] = [b, a];
      [fa, fb] = [fb, fa];
    }

    c = a;
    fc = fa;

    let s: number;

    // Try inverse quadratic interpolation
    if (fa !== fc && fb !== fc) {
      const r = fb / fc;
      const p = (fa - fc) / (fb - fc);
      const q = fa / fb;
      s = b - (fb / fa) * (
        (q * (q - r) * (b - a) + (1 - q) * r * (b - c)) /
        ((p - 1) * (q - 1) * (r - 1))
      );
    } else {
      // Secant method
      s = b - fb * (b - a) / (fb - fa);
    }

    // Check if bisection is needed
    const cond1 = s < (3 * a + b) / 4 || s > b;
    const cond2 = Math.abs(s - b) >= Math.abs(b - c) / 2;
    const cond3 = Math.abs(b - c) < tolerance;

    if (cond1 || cond2 || cond3) {
      // Bisection
      s = (a + b) / 2;
    }

    const fs = f(s);

    // Update bracket
    c = b;
    fc = fb;

    if (fa * fs < 0) {
      b = s;
      fb = fs;
    } else {
      a = s;
      fa = fs;
    }

    // Ensure |f(a)| >= |f(b)|
    if (Math.abs(fa) < Math.abs(fb)) {
      [a, b] = [b, a];
      [fa, fb] = [fb, fa];
    }
  }

  return {
    root: b,
    iterations: maxIterations,
    converged: false,
    error: Math.abs(fb),
  };
}

/**
 * Try to find a bracket for the root by expanding the search
 */
function expandBracket(
  f: (x: number) => number,
  a: number,
  b: number,
  lower: number,
  upper: number
): { a: number; b: number; fa: number; fb: number } | null {
  const nSteps = 10;
  const expansion = 1.5;

  let fa = f(a);
  let fb = f(b);

  for (let i = 0; i < nSteps; i++) {
    if (fa * fb < 0) {
      return { a, b, fa, fb };
    }

    if (Math.abs(fa) < Math.abs(fb)) {
      a = Math.max(lower, a - expansion * (b - a));
      fa = f(a);
    } else {
      b = Math.min(upper, b + expansion * (b - a));
      fb = f(b);
    }
  }

  return fa * fb < 0 ? { a, b, fa, fb } : null;
}

/**
 * Newton-Raphson method (for simple 1D cases)
 */
export function newton(
  f: (x: number) => number,
  df: (x: number) => number,
  x0: number,
  tolerance: number = 1e-10,
  maxIterations: number = 50
): RootResult {
  let x = x0;

  for (let iter = 0; iter < maxIterations; iter++) {
    const fx = f(x);
    if (Math.abs(fx) < tolerance) {
      return { root: x, iterations: iter + 1, converged: true, error: Math.abs(fx) };
    }

    const dfx = df(x);
    if (Math.abs(dfx) < 1e-15) {
      return { root: x, iterations: iter + 1, converged: false, error: Math.abs(fx) };
    }

    x = x - fx / dfx;
  }

  return { root: x, iterations: maxIterations, converged: false, error: Math.abs(f(x)) };
}

// =============================================================================
// Matrix Operations for Levenberg-Marquardt
// =============================================================================

/**
 * Multiply matrix transpose by matrix: A^T * A
 */
export function multiplyTranspose(A: number[][]): number[][] {
  const m = A.length;     // rows
  const n = A[0].length;  // cols

  const result: number[][] = Array(n).fill(null).map(() => Array(n).fill(0));

  for (let i = 0; i < n; i++) {
    for (let j = 0; j < n; j++) {
      let sum = 0;
      for (let k = 0; k < m; k++) {
        sum += A[k][i] * A[k][j];
      }
      result[i][j] = sum;
    }
  }

  return result;
}

/**
 * Multiply matrix transpose by vector: A^T * v
 */
export function vectorMultiplyTranspose(A: number[][], v: number[]): number[] {
  const m = A.length;     // rows
  const n = A[0].length;  // cols

  const result: number[] = Array(n).fill(0);

  for (let i = 0; i < n; i++) {
    let sum = 0;
    for (let k = 0; k < m; k++) {
      sum += A[k][i] * v[k];
    }
    result[i] = sum;
  }

  return result;
}

/**
 * Solve (A + λI)x = b using Cholesky decomposition
 * For Levenberg-Marquardt damped system
 */
export function solveDampedSystem(
  A: number[][],
  b: number[],
  lambda: number
): number[] {
  const n = A.length;

  // Add damping: A + λI
  const damped: number[][] = A.map((row, i) =>
    row.map((val, j) => val + (i === j ? lambda : 0))
  );

  // Cholesky decomposition: A = L * L^T
  const L: number[][] = Array(n).fill(null).map(() => Array(n).fill(0));

  for (let i = 0; i < n; i++) {
    for (let j = 0; j <= i; j++) {
      let sum = 0;
      for (let k = 0; k < j; k++) {
        sum += L[i][k] * L[j][k];
      }

      if (i === j) {
        const diag = damped[i][i] - sum;
        if (diag <= 0) {
          // Not positive definite, fall back to simple inversion
          return solveGaussianElimination(damped, b);
        }
        L[i][j] = Math.sqrt(diag);
      } else {
        L[i][j] = (damped[i][j] - sum) / L[j][j];
      }
    }
  }

  // Forward substitution: L * y = b
  const y: number[] = Array(n).fill(0);
  for (let i = 0; i < n; i++) {
    let sum = 0;
    for (let j = 0; j < i; j++) {
      sum += L[i][j] * y[j];
    }
    y[i] = (b[i] - sum) / L[i][i];
  }

  // Back substitution: L^T * x = y
  const x: number[] = Array(n).fill(0);
  for (let i = n - 1; i >= 0; i--) {
    let sum = 0;
    for (let j = i + 1; j < n; j++) {
      sum += L[j][i] * x[j];
    }
    x[i] = (y[i] - sum) / L[i][i];
  }

  return x;
}

/**
 * Gaussian elimination with partial pivoting (fallback solver)
 */
function solveGaussianElimination(A: number[][], b: number[]): number[] {
  const n = A.length;

  // Augmented matrix
  const aug: number[][] = A.map((row, i) => [...row, b[i]]);

  // Forward elimination with partial pivoting
  for (let i = 0; i < n; i++) {
    // Find pivot
    let maxIdx = i;
    for (let k = i + 1; k < n; k++) {
      if (Math.abs(aug[k][i]) > Math.abs(aug[maxIdx][i])) {
        maxIdx = k;
      }
    }

    // Swap rows
    [aug[i], aug[maxIdx]] = [aug[maxIdx], aug[i]];

    // Eliminate
    for (let k = i + 1; k < n; k++) {
      if (Math.abs(aug[i][i]) < 1e-15) continue;
      const factor = aug[k][i] / aug[i][i];
      for (let j = i; j <= n; j++) {
        aug[k][j] -= factor * aug[i][j];
      }
    }
  }

  // Back substitution
  const x: number[] = Array(n).fill(0);
  for (let i = n - 1; i >= 0; i--) {
    let sum = 0;
    for (let j = i + 1; j < n; j++) {
      sum += aug[i][j] * x[j];
    }
    x[i] = Math.abs(aug[i][i]) > 1e-15 ? (aug[i][n] - sum) / aug[i][i] : 0;
  }

  return x;
}

/**
 * Compute numerical Jacobian
 * J[i][j] = ∂f_i/∂x_j
 */
export function computeNumericalJacobian(
  f: (x: number[]) => number[],
  x: number[],
  step: number = 1e-6
): number[][] {
  const n = x.length;
  const f0 = f(x);
  const m = f0.length;

  const J: number[][] = Array(m).fill(null).map(() => Array(n).fill(0));

  for (let j = 0; j < n; j++) {
    const xPlus = [...x];
    xPlus[j] += step;
    const fPlus = f(xPlus);

    for (let i = 0; i < m; i++) {
      J[i][j] = (fPlus[i] - f0[i]) / step;
    }
  }

  return J;
}
