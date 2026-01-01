// =============================================================================
// Curve Representation
// Curve building and querying utilities
// =============================================================================

import { CurvePoint, BootstrappedCurve, InterpolationType } from './types';
import { interpolateRate, interpolateLogLinear } from './interpolation';

/**
 * Build a curve from tenor/rate pairs
 */
export function buildCurve(
  tenors: number[],
  rates: number[],
  options: {
    id?: string;
    referenceDate?: string;
    interpolation?: InterpolationType;
  } = {}
): BootstrappedCurve {
  if (tenors.length !== rates.length) {
    throw new Error('Tenors and rates arrays must have same length');
  }

  // Sort by tenor
  const points: CurvePoint[] = tenors
    .map((tenor, i) => ({ tenor, rate: rates[i] }))
    .sort((a, b) => a.tenor - b.tenor);

  return {
    id: options.id || `curve-${Date.now()}`,
    referenceDate: options.referenceDate || new Date().toISOString().split('T')[0],
    points,
    interpolation: options.interpolation || 'Linear',
    valueType: 'ZeroRate',
  };
}

/**
 * Get zero rate at a specific tenor
 */
export function getZeroRate(curve: BootstrappedCurve, tenor: number): number {
  if (curve.points.length === 0) {
    throw new Error('Curve has no points');
  }

  // Handle extrapolation
  if (tenor <= curve.points[0].tenor) {
    return curve.points[0].rate;
  }
  if (tenor >= curve.points[curve.points.length - 1].tenor) {
    return curve.points[curve.points.length - 1].rate;
  }

  // Find bracketing points
  let i = 0;
  while (i < curve.points.length - 1 && curve.points[i + 1].tenor < tenor) {
    i++;
  }

  const p1 = curve.points[i];
  const p2 = curve.points[i + 1];

  // Interpolate based on method
  switch (curve.interpolation) {
    case 'LogLinear':
      return interpolateLogLinear(p1.tenor, p1.rate, p2.tenor, p2.rate, tenor);
    case 'Linear':
    default:
      return interpolateRate(p1.tenor, p1.rate, p2.tenor, p2.rate, tenor);
  }
}

/**
 * Get discount factor at a specific tenor
 * DF(t) = exp(-r * t) for continuous compounding
 */
export function getDiscountFactor(curve: BootstrappedCurve, tenor: number): number {
  if (tenor <= 0) return 1.0;
  const rate = getZeroRate(curve, tenor);
  return Math.exp(-rate * tenor);
}

/**
 * Get forward rate between two tenors
 * F(t1, t2) = (DF(t1) / DF(t2) - 1) / (t2 - t1)
 */
export function getForwardRate(
  curve: BootstrappedCurve,
  t1: number,
  t2: number
): number {
  if (t2 <= t1) {
    throw new Error('t2 must be greater than t1');
  }

  const df1 = getDiscountFactor(curve, t1);
  const df2 = getDiscountFactor(curve, t2);
  const tau = t2 - t1;

  return (df1 / df2 - 1) / tau;
}

/**
 * Get instantaneous forward rate at a tenor
 * f(t) = -d(ln DF)/dt ≈ (r(t+dt) * (t+dt) - r(t) * t) / dt
 */
export function getInstantaneousForward(
  curve: BootstrappedCurve,
  tenor: number,
  dt: number = 0.01
): number {
  const r1 = getZeroRate(curve, tenor);
  const r2 = getZeroRate(curve, tenor + dt);
  return (r2 * (tenor + dt) - r1 * tenor) / dt;
}

/**
 * Generate curve points for charting at regular intervals
 */
export function generateCurvePoints(
  curve: BootstrappedCurve,
  minTenor: number = 0,
  maxTenor: number = 30,
  step: number = 0.25
): CurvePoint[] {
  const points: CurvePoint[] = [];

  for (let t = minTenor; t <= maxTenor; t += step) {
    points.push({
      tenor: t,
      rate: getZeroRate(curve, t),
    });
  }

  return points;
}

/**
 * Generate forward curve points for charting
 */
export function generateForwardCurvePoints(
  curve: BootstrappedCurve,
  minTenor: number = 0,
  maxTenor: number = 30,
  forwardPeriod: number = 0.25,
  step: number = 0.25
): CurvePoint[] {
  const points: CurvePoint[] = [];

  for (let t = minTenor; t <= maxTenor - forwardPeriod; t += step) {
    points.push({
      tenor: t,
      rate: getForwardRate(curve, t, t + forwardPeriod),
    });
  }

  return points;
}

/**
 * Generate discount factor curve points for charting
 */
export function generateDiscountCurvePoints(
  curve: BootstrappedCurve,
  minTenor: number = 0,
  maxTenor: number = 30,
  step: number = 0.25
): CurvePoint[] {
  const points: CurvePoint[] = [];

  for (let t = minTenor; t <= maxTenor; t += step) {
    points.push({
      tenor: t,
      rate: getDiscountFactor(curve, t),
    });
  }

  return points;
}

/**
 * Clone a curve with new points
 */
export function cloneCurve(
  curve: BootstrappedCurve,
  newPoints?: CurvePoint[]
): BootstrappedCurve {
  return {
    ...curve,
    id: `${curve.id}-clone`,
    points: newPoints ? [...newPoints] : curve.points.map(p => ({ ...p })),
  };
}

/**
 * Shift curve by parallel amount (in basis points)
 */
export function shiftCurve(
  curve: BootstrappedCurve,
  shiftBps: number
): BootstrappedCurve {
  const shift = shiftBps / 10000;
  return {
    ...curve,
    id: `${curve.id}-shifted-${shiftBps}bp`,
    points: curve.points.map(p => ({
      tenor: p.tenor,
      rate: p.rate + shift,
    })),
  };
}
