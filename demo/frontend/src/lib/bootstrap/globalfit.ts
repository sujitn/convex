// =============================================================================
// Global Fitter
// Levenberg-Marquardt optimization for curve calibration
// =============================================================================

import {
  CalibrationInstrument,
  CalibrationResult,
  BootstrappedCurve,
  FitterConfig,
  DEFAULT_FITTER_CONFIG,
  InterpolationType,
  getInstrumentTenor,
} from './types';
import { buildCurve } from './curve';
import {
  multiplyTranspose,
  vectorMultiplyTranspose,
  solveDampedSystem,
} from './solver';
import {
  pricingError,
  calculateRMSError,
  calculateMaxError,
  errorsToBps,
} from './instruments';
import { bondPricingError } from './bonds';
import { bootstrapPiecewise } from './piecewise';

/**
 * Get pricing error for any instrument type
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
 * Global fitter options
 */
export interface GlobalFitOptions {
  interpolation?: InterpolationType;
  config?: Partial<FitterConfig>;
  referenceDate?: string;
  curveId?: string;
}

/**
 * Global curve fitter using Levenberg-Marquardt optimization
 *
 * Fits all instruments simultaneously by minimizing sum of squared
 * pricing errors. Algorithm:
 *
 * 1. Initialize curve with instrument quotes
 * 2. Compute pricing residuals
 * 3. Compute Jacobian numerically
 * 4. Solve: (J^T J + λI) δ = -J^T r
 * 5. Update curve values: x = x + δ
 * 6. Adjust λ based on improvement
 * 7. Repeat until convergence
 *
 * Advantages:
 * - Better stability than piecewise
 * - Handles over-determined systems
 * - Handles instrument interdependencies
 */
export function bootstrapGlobalFit(
  instruments: CalibrationInstrument[],
  options: GlobalFitOptions = {}
): CalibrationResult {
  const startTime = performance.now();

  const {
    interpolation = 'Linear',
    config = {},
    referenceDate = new Date().toISOString().split('T')[0],
    curveId = `globalfit-${Date.now()}`,
  } = options;

  const cfg: FitterConfig = { ...DEFAULT_FITTER_CONFIG, ...config };

  // Sort instruments by maturity
  const sorted = [...instruments].sort(
    (a, b) => getInstrumentTenor(a) - getInstrumentTenor(b)
  );

  if (sorted.length === 0) {
    throw new Error('No instruments provided');
  }

  // Extract unique tenors from instruments
  const tenorSet = new Set<number>([0]);
  for (const inst of sorted) {
    const tenor = getInstrumentTenor(inst);
    tenorSet.add(tenor);

    // For FRAs, add both start and end tenors
    if (inst.type === 'FRA') {
      tenorSet.add(inst.startTenor);
      tenorSet.add(inst.endTenor);
    }
  }

  const tenors = Array.from(tenorSet).sort((a, b) => a - b);
  const m = sorted.length;

  // Initialize rates with instrument quotes as initial guess
  let rates = tenors.map(t => {
    // Find nearest instrument
    const inst = sorted.find(i => Math.abs(getInstrumentTenor(i) - t) < 0.01);
    if (inst) {
      return getInitialRate(inst);
    }
    // Interpolate from neighbors
    return 0.04; // Default 4%
  });

  // Levenberg-Marquardt iteration
  let lambda = cfg.initialLambda;
  const convergenceHistory: number[] = [];
  let prevError = Infinity;

  for (let iter = 0; iter < cfg.maxIterations; iter++) {
    // Build current curve
    const curve = buildCurve(tenors, rates, {
      id: curveId,
      interpolation,
      referenceDate,
    });

    // Compute residuals
    const residuals = sorted.map(inst => getInstrumentPricingError(inst, curve));
    const error = residuals.reduce((sum, r) => sum + r * r, 0);
    const rms = Math.sqrt(error / m);
    convergenceHistory.push(rms);

    // Check convergence
    if (rms < cfg.tolerance) {
      return createResult(
        curve,
        residuals,
        iter + 1,
        rms,
        true,
        convergenceHistory,
        startTime
      );
    }

    // Compute Jacobian numerically
    // J[i][j] = ∂(residual_i) / ∂(rate_j)
    const jacobian = computeJacobian(
      tenors,
      rates,
      sorted,
      interpolation,
      referenceDate,
      cfg.jacobianStep
    );

    // Normal equations: (J^T J + λI) δ = -J^T r
    const jtj = multiplyTranspose(jacobian);
    const jtr = vectorMultiplyTranspose(jacobian, residuals);
    const negJtr = jtr.map(v => -v);

    // Solve for step
    const delta = solveDampedSystem(jtj, negJtr, lambda);

    // Proposed new rates
    const newRates = rates.map((r, i) => {
      const newR = r + delta[i];
      // Clamp to reasonable range
      return Math.max(-0.10, Math.min(0.50, newR));
    });

    // Evaluate new error
    const newCurve = buildCurve(tenors, newRates, {
      interpolation,
      referenceDate,
    });
    const newResiduals = sorted.map(inst => getInstrumentPricingError(inst, newCurve));
    const newError = newResiduals.reduce((sum, r) => sum + r * r, 0);

    // Accept or reject step
    if (newError < error) {
      // Accept step, decrease damping
      rates = newRates;
      lambda = Math.max(cfg.minLambda, lambda / cfg.lambdaFactor);
      prevError = error;
    } else {
      // Reject step, increase damping
      lambda = Math.min(cfg.maxLambda, lambda * cfg.lambdaFactor);

      // If lambda is very large and no progress, break
      if (lambda >= cfg.maxLambda && Math.abs(error - prevError) < 1e-15) {
        break;
      }
    }
  }

  // Build final curve
  const finalCurve = buildCurve(tenors, rates, {
    id: curveId,
    interpolation,
    referenceDate,
  });
  const finalResiduals = sorted.map(inst => getInstrumentPricingError(inst, finalCurve));
  const finalRms = calculateRMSError(finalResiduals);

  return createResult(
    finalCurve,
    finalResiduals,
    cfg.maxIterations,
    finalRms,
    finalRms < cfg.tolerance,
    convergenceHistory,
    startTime
  );
}

/**
 * Compute numerical Jacobian
 */
function computeJacobian(
  tenors: number[],
  rates: number[],
  instruments: CalibrationInstrument[],
  interpolation: InterpolationType,
  referenceDate: string,
  step: number
): number[][] {
  const n = rates.length;
  const m = instruments.length;
  const jacobian: number[][] = Array(m).fill(null).map(() => Array(n).fill(0));

  // Base residuals
  const baseCurve = buildCurve(tenors, rates, { interpolation, referenceDate });
  const baseResiduals = instruments.map(inst => getInstrumentPricingError(inst, baseCurve));

  // Perturb each rate
  for (let j = 0; j < n; j++) {
    const perturbedRates = [...rates];
    perturbedRates[j] += step;

    const perturbedCurve = buildCurve(tenors, perturbedRates, {
      interpolation,
      referenceDate,
    });
    const perturbedResiduals = instruments.map(inst =>
      getInstrumentPricingError(inst, perturbedCurve)
    );

    for (let i = 0; i < m; i++) {
      jacobian[i][j] = (perturbedResiduals[i] - baseResiduals[i]) / step;
    }
  }

  return jacobian;
}

/**
 * Get initial rate from instrument
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
      if (inst.coupon === 0) {
        return -Math.log(inst.price / 100) / inst.maturity;
      }
      return inst.coupon + (100 - inst.price) / (inst.maturity * inst.price);
  }
}

/**
 * Create calibration result
 */
function createResult(
  curve: BootstrappedCurve,
  residuals: number[],
  iterations: number,
  rmsError: number,
  converged: boolean,
  convergenceHistory: number[],
  startTime: number
): CalibrationResult {
  return {
    curve,
    residuals,
    residualsBps: errorsToBps(residuals),
    iterations,
    rmsError,
    maxError: calculateMaxError(residuals),
    converged,
    method: 'GlobalFit',
    convergenceHistory,
    durationMs: performance.now() - startTime,
  };
}

/**
 * Bootstrap with automatic method selection
 * Uses global fit by default, falls back to piecewise if needed
 */
export function bootstrapAuto(
  instruments: CalibrationInstrument[],
  options: GlobalFitOptions = {}
): CalibrationResult {
  // Try global fit first
  const globalResult = bootstrapGlobalFit(instruments, options);

  // If converged well, return
  if (globalResult.converged && globalResult.rmsError < 1e-8) {
    return globalResult;
  }

  // Try piecewise as fallback
  const piecewiseResult = bootstrapPiecewise(instruments, {
    interpolation: options.interpolation,
    referenceDate: options.referenceDate,
    curveId: options.curveId,
  });

  // Return better result
  if (piecewiseResult.rmsError < globalResult.rmsError) {
    return piecewiseResult;
  }

  return globalResult;
}
