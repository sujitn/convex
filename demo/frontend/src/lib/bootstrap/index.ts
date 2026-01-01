// =============================================================================
// Curve Bootstrapping Library
// Re-exports all modules for convenient importing
// =============================================================================

// Types
export * from './types';

// Curve operations
export * from './curve';

// Interpolation methods
export * from './interpolation';

// Numerical solvers
export * from './solver';

// Instruments (Deposit, FRA, Swap, OIS)
export * from './instruments';

// Bond pricing
export * from './bonds';

// Bootstrap algorithms
export { bootstrapPiecewise, bootstrapFromBonds, type PiecewiseOptions } from './piecewise';
export { bootstrapGlobalFit, bootstrapAuto, type GlobalFitOptions } from './globalfit';

// Preset market data
export * from './presets';
