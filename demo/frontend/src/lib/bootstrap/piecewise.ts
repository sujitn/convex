// =============================================================================
// Piecewise Bootstrapper
// Iterative bootstrap using Brent's method for root-finding
// =============================================================================

import {
  CalibrationInstrument,
  CalibrationResult,
  BootstrappedCurve,
  SolverConfig,
  DEFAULT_SOLVER_CONFIG,
  InterpolationType,
  getInstrumentTenor,
} from './types';
import { buildCurve } from './curve';
import { brent } from './solver';
import { pricingError, calculateRMSError, calculateMaxError, errorsToBps } from './instruments';
import { bondPricingError } from './bonds';

/**
 * Get pricing error for any instrument type (including bonds)
 */
function getInstrumentPricingError(
  inst: CalibrationInstrument,
  curve: BootstrappedCurve
): number {
  if (inst.type === 'Bond') {
    return bondPricingError(inst, curve);
  }
  return pricingError(inst, curve);
}

/**
 * Piecewise bootstrap options
 */
export interface PiecewiseOptions {
  interpolation?: InterpolationType;
  solverConfig?: Partial<SolverConfig>;
  referenceDate?: string;
  curveId?: string;
}

/**
 * Piecewise bootstrapper using Brent's method
 *
 * For each instrument in maturity order:
 * 1. Build temporary curve from previously solved points
 * 2. Use Brent's method to find zero rate that reprices instrument
 * 3. Add solved point to curve
 *
 * This method provides exact fit to market quotes (within solver tolerance)
 * but is sensitive to instrument ordering.
 */
export function bootstrapPiecewise(
  instruments: CalibrationInstrument[],
  options: PiecewiseOptions = {}
): CalibrationResult {
  const startTime = performance.now();

  const {
    interpolation = 'Linear',
    solverConfig = {},
    referenceDate = new Date().toISOString().split('T')[0],
    curveId = `piecewise-${Date.now()}`,
  } = options;

  const config = { ...DEFAULT_SOLVER_CONFIG, ...solverConfig };

  // Sort instruments by maturity
  const sorted = [...instruments].sort(
    (a, b) => getInstrumentTenor(a) - getInstrumentTenor(b)
  );

  if (sorted.length === 0) {
    throw new Error('No instruments provided');
  }

  // Initialize with first point at t=0
  const tenors: number[] = [0];
  const rates: number[] = [getInitialRate(sorted[0])];
  let totalIterations = 0;

  // Bootstrap each instrument
  for (const inst of sorted) {
    const targetTenor = getInstrumentTenor(inst);

    // Skip if we already have this tenor
    if (tenors.includes(targetTenor)) {
      continue;
    }

    // Objective function: find rate that makes pricing error = 0
    const objective = (rate: number): number => {
      // Build temporary curve with new point
      const tempTenors = [...tenors, targetTenor];
      const tempRates = [...rates, rate];
      const tempCurve = buildCurve(tempTenors, tempRates, {
        interpolation,
        referenceDate,
      });

      return getInstrumentPricingError(inst, tempCurve);
    };

    // Initial guess from instrument quote
    const initialGuess = getInitialRate(inst);

    // Use Brent's method to find root
    const result = brent(
      objective,
      initialGuess - 0.05,  // Lower bound: -5% from initial
      initialGuess + 0.05,  // Upper bound: +5% from initial
      config
    );

    // Add solved point
    tenors.push(targetTenor);
    rates.push(result.root);
    totalIterations += result.iterations;
  }

  // Build final curve
  const curve = buildCurve(tenors, rates, {
    id: curveId,
    interpolation,
    referenceDate,
  });

  // Calculate final residuals
  const residuals = sorted.map(inst => getInstrumentPricingError(inst, curve));
  const rmsError = calculateRMSError(residuals);
  const maxError = calculateMaxError(residuals);

  return {
    curve,
    residuals,
    residualsBps: errorsToBps(residuals),
    iterations: totalIterations,
    rmsError,
    maxError,
    converged: rmsError < 1e-6,
    method: 'Piecewise',
    durationMs: performance.now() - startTime,
  };
}

/**
 * Get initial rate guess from instrument
 */
function getInitialRate(inst: CalibrationInstrument): number {
  switch (inst.type) {
    case 'Deposit':
    case 'Swap':
    case 'OIS':
      return inst.quote;
    case 'FRA':
      return inst.quote;
    case 'Bond':
      // Estimate zero rate from bond price
      if (inst.coupon === 0) {
        // Zero coupon: r = -ln(P/100) / T
        return -Math.log(inst.price / 100) / inst.maturity;
      }
      // Coupon bond: use approximate YTM
      return inst.coupon + (100 - inst.price) / (inst.maturity * inst.price);
  }
}

/**
 * Bootstrap with bond stripping
 * Specialized version for bootstrapping from government bonds
 */
export function bootstrapFromBonds(
  bonds: CalibrationInstrument[],
  options: PiecewiseOptions = {}
): CalibrationResult {
  // Filter to only bonds
  const bondInsts = bonds.filter(b => b.type === 'Bond');

  if (bondInsts.length === 0) {
    throw new Error('No bond instruments provided');
  }

  // Sort by maturity
  const sorted = [...bondInsts].sort(
    (a, b) => getInstrumentTenor(a) - getInstrumentTenor(b)
  );

  const startTime = performance.now();
  const {
    interpolation = 'Linear',
    solverConfig = {},
    referenceDate = new Date().toISOString().split('T')[0],
    curveId = `bond-curve-${Date.now()}`,
  } = options;

  const config = { ...DEFAULT_SOLVER_CONFIG, ...solverConfig };

  // Initialize curve points
  const tenors: number[] = [0];
  const rates: number[] = [0.03]; // Initial guess: 3%
  let totalIterations = 0;

  // Bootstrap each bond
  for (const bond of sorted) {
    if (bond.type !== 'Bond') continue;

    const targetTenor = bond.maturity;

    // Skip if we already have this tenor
    if (tenors.includes(targetTenor)) {
      continue;
    }

    // Objective: find zero rate at maturity that prices the bond correctly
    const objective = (rate: number): number => {
      const tempTenors = [...tenors, targetTenor];
      const tempRates = [...rates, rate];
      const tempCurve = buildCurve(tempTenors, tempRates, {
        interpolation,
        referenceDate,
      });

      return bondPricingError(bond, tempCurve);
    };

    // Initial guess
    const initialGuess = getInitialRate(bond);

    // Solve
    const result = brent(
      objective,
      Math.max(-0.05, initialGuess - 0.10),
      Math.min(0.30, initialGuess + 0.10),
      config
    );

    tenors.push(targetTenor);
    rates.push(result.root);
    totalIterations += result.iterations;
  }

  // Build final curve
  const curve = buildCurve(tenors, rates, {
    id: curveId,
    interpolation,
    referenceDate,
  });

  // Calculate residuals
  const residuals = sorted.map(bond => bondPricingError(bond as any, curve));
  const rmsError = calculateRMSError(residuals);
  const maxError = calculateMaxError(residuals);

  return {
    curve,
    residuals,
    residualsBps: errorsToBps(residuals),
    iterations: totalIterations,
    rmsError,
    maxError,
    converged: rmsError < 1e-4,  // 1bp tolerance for bonds
    method: 'Piecewise',
    durationMs: performance.now() - startTime,
  };
}
