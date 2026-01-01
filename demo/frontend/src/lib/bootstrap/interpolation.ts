// =============================================================================
// Interpolation Methods
// Linear, log-linear, and cubic spline interpolation
// =============================================================================

/**
 * Linear interpolation between two points
 */
export function interpolateRate(
  t1: number,
  r1: number,
  t2: number,
  r2: number,
  t: number
): number {
  if (t2 === t1) return r1;
  const alpha = (t - t1) / (t2 - t1);
  return r1 + alpha * (r2 - r1);
}

/**
 * Log-linear interpolation (for discount factors)
 * Interpolates in log space then converts back
 * This is better for discount factor monotonicity
 */
export function interpolateLogLinear(
  t1: number,
  r1: number,
  t2: number,
  r2: number,
  t: number
): number {
  if (t2 === t1) return r1;

  // Convert to discount factors
  const df1 = Math.exp(-r1 * t1);
  const df2 = Math.exp(-r2 * t2);

  // Interpolate in log space
  const logDf1 = Math.log(df1);
  const logDf2 = Math.log(df2);
  const alpha = (t - t1) / (t2 - t1);
  const logDf = logDf1 + alpha * (logDf2 - logDf1);

  // Convert back to rate
  const df = Math.exp(logDf);
  return -Math.log(df) / t;
}

/**
 * Cubic spline interpolation coefficients
 * Uses natural boundary conditions (second derivative = 0 at endpoints)
 */
export interface SplineCoefficients {
  tenors: number[];
  values: number[];
  a: number[];  // Constant term
  b: number[];  // Linear term
  c: number[];  // Quadratic term
  d: number[];  // Cubic term
}

/**
 * Build cubic spline coefficients for a set of points
 */
export function buildCubicSpline(
  tenors: number[],
  values: number[]
): SplineCoefficients {
  const n = tenors.length;
  if (n < 2) {
    throw new Error('Need at least 2 points for spline');
  }

  // For n points, we have n-1 segments
  const h: number[] = [];
  for (let i = 0; i < n - 1; i++) {
    h.push(tenors[i + 1] - tenors[i]);
  }

  // Build tridiagonal system for second derivatives
  // Natural spline: s''(0) = s''(n-1) = 0
  const alpha: number[] = new Array(n).fill(0);
  for (let i = 1; i < n - 1; i++) {
    alpha[i] = (3 / h[i]) * (values[i + 1] - values[i]) -
               (3 / h[i - 1]) * (values[i] - values[i - 1]);
  }

  // Solve tridiagonal system using Thomas algorithm
  const l: number[] = new Array(n).fill(1);
  const mu: number[] = new Array(n).fill(0);
  const z: number[] = new Array(n).fill(0);

  for (let i = 1; i < n - 1; i++) {
    l[i] = 2 * (tenors[i + 1] - tenors[i - 1]) - h[i - 1] * mu[i - 1];
    mu[i] = h[i] / l[i];
    z[i] = (alpha[i] - h[i - 1] * z[i - 1]) / l[i];
  }

  // Back substitution for c coefficients (second derivatives / 2)
  const c: number[] = new Array(n).fill(0);
  for (let j = n - 2; j >= 0; j--) {
    c[j] = z[j] - mu[j] * c[j + 1];
  }

  // Compute remaining coefficients
  const a: number[] = values.slice();
  const b: number[] = new Array(n - 1).fill(0);
  const d: number[] = new Array(n - 1).fill(0);

  for (let i = 0; i < n - 1; i++) {
    b[i] = (values[i + 1] - values[i]) / h[i] - h[i] * (c[i + 1] + 2 * c[i]) / 3;
    d[i] = (c[i + 1] - c[i]) / (3 * h[i]);
  }

  return { tenors, values, a, b, c, d };
}

/**
 * Evaluate cubic spline at a point
 */
export function evaluateSpline(
  spline: SplineCoefficients,
  t: number
): number {
  const { tenors, a, b, c, d } = spline;
  const n = tenors.length;

  // Handle extrapolation (linear beyond endpoints)
  if (t <= tenors[0]) {
    const dt = t - tenors[0];
    return a[0] + b[0] * dt;
  }
  if (t >= tenors[n - 1]) {
    const i = n - 2;
    const dt = t - tenors[i];
    return a[i] + b[i] * dt + c[i] * dt * dt + d[i] * dt * dt * dt;
  }

  // Find segment
  let i = 0;
  while (i < n - 2 && tenors[i + 1] < t) {
    i++;
  }

  const dt = t - tenors[i];
  return a[i] + b[i] * dt + c[i] * dt * dt + d[i] * dt * dt * dt;
}

/**
 * Monotone convex interpolation
 * Ensures forward rates are well-behaved
 * Reference: Hagan & West (2006) "Methods for Constructing a Yield Curve"
 */
export function interpolateMonotoneConvex(
  tenors: number[],
  values: number[],
  t: number
): number {
  const n = tenors.length;
  if (n < 2) return values[0];

  // Find segment
  if (t <= tenors[0]) return values[0];
  if (t >= tenors[n - 1]) return values[n - 1];

  let i = 0;
  while (i < n - 2 && tenors[i + 1] < t) {
    i++;
  }

  const t0 = tenors[i];
  const t1 = tenors[i + 1];
  const f0 = values[i];
  const f1 = values[i + 1];

  // Calculate forward rates at segment endpoints
  const fwd0 = i > 0
    ? (values[i] - values[i - 1]) / (tenors[i] - tenors[i - 1])
    : (f1 - f0) / (t1 - t0);
  const fwd1 = i < n - 2
    ? (values[i + 2] - values[i + 1]) / (tenors[i + 2] - tenors[i + 1])
    : (f1 - f0) / (t1 - t0);

  // Linear interpolation with adjusted slopes for monotonicity
  const alpha = (t - t0) / (t1 - t0);

  // Hermite basis
  const h00 = 2 * alpha * alpha * alpha - 3 * alpha * alpha + 1;
  const h10 = alpha * alpha * alpha - 2 * alpha * alpha + alpha;
  const h01 = -2 * alpha * alpha * alpha + 3 * alpha * alpha;
  const h11 = alpha * alpha * alpha - alpha * alpha;

  const dt = t1 - t0;
  return h00 * f0 + h10 * dt * fwd0 + h01 * f1 + h11 * dt * fwd1;
}
