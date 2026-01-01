// =============================================================================
// Curve Bootstrapping Types
// Core type definitions for curve calibration
// =============================================================================

// Instrument types supported for curve construction
export type InstrumentType = 'Deposit' | 'FRA' | 'Swap' | 'OIS' | 'Bond';

// Day count conventions
export type DayCountConvention = 'Act360' | 'Act365' | 'ActAct' | '30360';

// Interpolation methods
export type InterpolationType = 'Linear' | 'LogLinear' | 'CubicSpline' | 'MonotoneConvex';

// Base calibration instrument interface
export interface BaseInstrument {
  type: InstrumentType;
  description?: string;
}

// Deposit instrument (money market)
export interface DepositInstrument extends BaseInstrument {
  type: 'Deposit';
  tenor: number;        // Years to maturity
  quote: number;        // Rate as decimal (0.05 = 5%)
  dayCount?: DayCountConvention;
}

// Forward Rate Agreement
export interface FRAInstrument extends BaseInstrument {
  type: 'FRA';
  startTenor: number;   // Start date in years (e.g., 0.25 for 3M)
  endTenor: number;     // End date in years (e.g., 0.5 for 6M)
  quote: number;        // Forward rate as decimal
  dayCount?: DayCountConvention;
}

// Interest Rate Swap
export interface SwapInstrument extends BaseInstrument {
  type: 'Swap';
  tenor: number;        // Years to maturity
  quote: number;        // Fixed rate as decimal
  frequency?: number;   // Fixed leg frequency (default: 2 = semi-annual)
  dayCount?: DayCountConvention;
}

// Overnight Index Swap
export interface OISInstrument extends BaseInstrument {
  type: 'OIS';
  tenor: number;        // Years to maturity
  quote: number;        // Fixed rate as decimal
  frequency?: number;   // Fixed leg frequency (default: 1 = annual)
}

// Government Bond for curve fitting
export interface BondInstrument extends BaseInstrument {
  type: 'Bond';
  coupon: number;       // Annual coupon rate as decimal
  maturity: number;     // Years to maturity
  price: number;        // Clean price (e.g., 99.50)
  frequency?: number;   // Coupon frequency (default: 2 = semi-annual)
  dayCount?: DayCountConvention;
}

// Union type for all instruments
export type CalibrationInstrument =
  | DepositInstrument
  | FRAInstrument
  | SwapInstrument
  | OISInstrument
  | BondInstrument;

// Curve point (tenor, rate)
export interface CurvePoint {
  tenor: number;        // Years
  rate: number;         // Zero rate as decimal
}

// Bootstrapped curve representation
export interface BootstrappedCurve {
  id: string;
  referenceDate: string;
  points: CurvePoint[];
  interpolation: InterpolationType;
  valueType: 'ZeroRate' | 'DiscountFactor';
}

// Global fitter configuration
export interface FitterConfig {
  maxIterations: number;
  tolerance: number;          // Convergence tolerance (RMS error)
  initialLambda: number;      // Initial damping parameter
  lambdaFactor: number;       // Lambda adjustment factor
  minLambda: number;          // Minimum lambda
  maxLambda: number;          // Maximum lambda
  jacobianStep: number;       // Step size for numerical Jacobian
}

// Default fitter configuration
export const DEFAULT_FITTER_CONFIG: FitterConfig = {
  maxIterations: 100,
  tolerance: 1e-8,           // 0.01 bps - realistic for numerical optimization
  initialLambda: 0.001,
  lambdaFactor: 10.0,
  minLambda: 1e-12,
  maxLambda: 1e6,
  jacobianStep: 1e-6,
};

// Piecewise solver configuration
export interface SolverConfig {
  tolerance: number;          // Root-finding tolerance
  maxIterations: number;      // Max iterations per root
  lowerBound: number;         // Lower search bound for rates
  upperBound: number;         // Upper search bound for rates
}

// Default solver configuration
export const DEFAULT_SOLVER_CONFIG: SolverConfig = {
  tolerance: 1e-12,
  maxIterations: 100,
  lowerBound: -0.10,          // -10% rate
  upperBound: 0.50,           // 50% rate
};

// Calibration result
export interface CalibrationResult {
  curve: BootstrappedCurve;
  residuals: number[];            // Pricing error per instrument
  residualsBps: number[];         // Residuals in basis points
  iterations: number;
  rmsError: number;               // Root mean square error
  maxError: number;               // Maximum absolute error
  converged: boolean;
  method: 'GlobalFit' | 'Piecewise';
  convergenceHistory?: number[];  // RMS error per iteration (GlobalFit only)
  durationMs: number;             // Calibration time in milliseconds
}

// Comparison result for side-by-side analysis
export interface ComparisonResult {
  globalFit?: CalibrationResult;
  piecewise?: CalibrationResult;
  instruments: CalibrationInstrument[];
  timestamp: string;
}

// Helper to get tenor from any instrument
export function getInstrumentTenor(inst: CalibrationInstrument): number {
  switch (inst.type) {
    case 'Deposit':
    case 'Swap':
    case 'OIS':
      return inst.tenor;
    case 'FRA':
      return inst.endTenor;
    case 'Bond':
      return inst.maturity;
  }
}

// Helper to format tenor as string
export function formatTenor(years: number): string {
  if (years < 1) {
    const months = Math.round(years * 12);
    return `${months}M`;
  } else if (years === Math.floor(years)) {
    return `${years}Y`;
  } else {
    return `${years.toFixed(1)}Y`;
  }
}

// Helper to format FRA notation
export function formatFRA(start: number, end: number): string {
  const startMonths = Math.round(start * 12);
  const endMonths = Math.round(end * 12);
  return `${startMonths}x${endMonths}`;
}
