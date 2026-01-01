// =============================================================================
// Calibration Instruments
// Deposit, FRA, Swap, OIS pricing for curve bootstrapping
// =============================================================================

import {
  CalibrationInstrument,
  DepositInstrument,
  FRAInstrument,
  SwapInstrument,
  OISInstrument,
  BootstrappedCurve,
  DayCountConvention,
} from './types';
import { getDiscountFactor } from './curve';

/**
 * Calculate year fraction based on day count convention
 * Simplified implementation for demo
 */
export function yearFraction(
  tenor: number,
  dayCount: DayCountConvention = 'Act360'
): number {
  // For simplicity, assume tenor is already in years
  // In production, this would use actual dates
  switch (dayCount) {
    case 'Act360':
      return tenor;  // Already in years
    case 'Act365':
      return tenor;
    case 'ActAct':
      return tenor;
    case '30360':
      return tenor;
    default:
      return tenor;
  }
}

/**
 * Calculate pricing error for a Deposit instrument
 *
 * Deposit pricing: The present value of receiving (1 + r*τ) at maturity
 * should equal 1 (the notional invested today)
 *
 * PV = (1 + r*τ) * DF(T)
 * Error = (1 + r*τ) * DF(T) - 1
 */
export function depositPricingError(
  inst: DepositInstrument,
  curve: BootstrappedCurve
): number {
  const tau = yearFraction(inst.tenor, inst.dayCount || 'Act360');
  const df = getDiscountFactor(curve, inst.tenor);
  return (1 + inst.quote * tau) * df - 1;
}

/**
 * Calculate DV01 for a Deposit (sensitivity to rate)
 */
export function depositDV01(
  inst: DepositInstrument,
  curve: BootstrappedCurve
): number {
  const tau = yearFraction(inst.tenor, inst.dayCount || 'Act360');
  const df = getDiscountFactor(curve, inst.tenor);
  return -tau * df;
}

/**
 * Calculate pricing error for a FRA instrument
 *
 * FRA pricing: The implied forward rate should equal the FRA rate
 *
 * Forward = (DF(T1) / DF(T2) - 1) / τ
 * Error = Forward - Quote
 */
export function fraPricingError(
  inst: FRAInstrument,
  curve: BootstrappedCurve
): number {
  const df1 = getDiscountFactor(curve, inst.startTenor);
  const df2 = getDiscountFactor(curve, inst.endTenor);
  const tau = inst.endTenor - inst.startTenor;

  const impliedForward = (df1 / df2 - 1) / tau;
  return impliedForward - inst.quote;
}

/**
 * Calculate DV01 for a FRA
 */
export function fraDV01(
  inst: FRAInstrument,
  curve: BootstrappedCurve
): number {
  const tau = inst.endTenor - inst.startTenor;
  const df = getDiscountFactor(curve, inst.endTenor);
  return tau * df;
}

/**
 * Calculate pricing error for a Swap instrument
 *
 * Swap pricing: Par swap rate makes NPV = 0
 *
 * Par rate = (DF(effective) - DF(maturity)) / Annuity
 * Annuity = Σ τ_i * DF(T_i) for all fixed leg payment dates
 *
 * Error = Par rate - Quote
 */
export function swapPricingError(
  inst: SwapInstrument,
  curve: BootstrappedCurve
): number {
  const frequency = inst.frequency || 2;  // Default semi-annual
  const parRate = calculateSwapParRate(curve, inst.tenor, frequency);
  return parRate - inst.quote;
}

/**
 * Calculate swap par rate
 */
export function calculateSwapParRate(
  curve: BootstrappedCurve,
  tenor: number,
  frequency: number = 2
): number {
  const dfEffective = 1.0;  // Assume spot start
  const dfMaturity = getDiscountFactor(curve, tenor);
  const annuity = calculateAnnuity(curve, tenor, frequency);

  if (annuity < 1e-10) {
    return 0;
  }

  return (dfEffective - dfMaturity) / annuity;
}

/**
 * Calculate fixed leg annuity
 * Annuity = Σ τ_i * DF(T_i)
 */
export function calculateAnnuity(
  curve: BootstrappedCurve,
  tenor: number,
  frequency: number = 2
): number {
  let annuity = 0;
  const period = 1 / frequency;
  const numPeriods = Math.ceil(tenor * frequency);

  for (let i = 1; i <= numPeriods; i++) {
    const t = Math.min(i * period, tenor);
    const df = getDiscountFactor(curve, t);
    const tau = i === numPeriods ? (tenor - (i - 1) * period) : period;
    annuity += tau * df;
  }

  return annuity;
}

/**
 * Calculate DV01 for a Swap (approximately equal to annuity)
 */
export function swapDV01(
  inst: SwapInstrument,
  curve: BootstrappedCurve
): number {
  const frequency = inst.frequency || 2;
  return calculateAnnuity(curve, inst.tenor, frequency);
}

/**
 * Calculate pricing error for an OIS instrument
 *
 * OIS pricing similar to Swap but typically:
 * - Annual fixed leg frequency (for simplicity)
 * - Can have more complex averaging for floating leg
 *
 * For this implementation, we use same logic as Swap
 */
export function oisPricingError(
  inst: OISInstrument,
  curve: BootstrappedCurve
): number {
  const frequency = inst.frequency || 1;  // Default annual for OIS
  const parRate = calculateSwapParRate(curve, inst.tenor, frequency);
  return parRate - inst.quote;
}

/**
 * Calculate DV01 for OIS
 */
export function oisDV01(
  inst: OISInstrument,
  curve: BootstrappedCurve
): number {
  const frequency = inst.frequency || 1;
  return calculateAnnuity(curve, inst.tenor, frequency);
}

/**
 * Generic pricing error function for any instrument type
 */
export function pricingError(
  inst: CalibrationInstrument,
  curve: BootstrappedCurve
): number {
  switch (inst.type) {
    case 'Deposit':
      return depositPricingError(inst, curve);
    case 'FRA':
      return fraPricingError(inst, curve);
    case 'Swap':
      return swapPricingError(inst, curve);
    case 'OIS':
      return oisPricingError(inst, curve);
    case 'Bond':
      // Bond pricing is in bonds.ts
      throw new Error('Use bondPricingError for Bond instruments');
  }
}

/**
 * Generic DV01 function for any instrument type
 */
export function instrumentDV01(
  inst: CalibrationInstrument,
  curve: BootstrappedCurve
): number {
  switch (inst.type) {
    case 'Deposit':
      return depositDV01(inst, curve);
    case 'FRA':
      return fraDV01(inst, curve);
    case 'Swap':
      return swapDV01(inst, curve);
    case 'OIS':
      return oisDV01(inst, curve);
    case 'Bond':
      throw new Error('Use bondDV01 for Bond instruments');
  }
}

/**
 * Calculate pricing errors for all instruments
 */
export function calculatePricingErrors(
  instruments: CalibrationInstrument[],
  curve: BootstrappedCurve
): number[] {
  return instruments.map(inst => {
    if (inst.type === 'Bond') {
      // Import bondPricingError dynamically to avoid circular dependency
      // For now, throw error
      throw new Error('Bond instruments should use bonds.ts');
    }
    return pricingError(inst, curve);
  });
}

/**
 * Calculate RMS error
 */
export function calculateRMSError(errors: number[]): number {
  if (errors.length === 0) return 0;
  const sumSquared = errors.reduce((sum, e) => sum + e * e, 0);
  return Math.sqrt(sumSquared / errors.length);
}

/**
 * Calculate max absolute error
 */
export function calculateMaxError(errors: number[]): number {
  if (errors.length === 0) return 0;
  return Math.max(...errors.map(Math.abs));
}

/**
 * Convert errors to basis points
 */
export function errorsToBps(errors: number[]): number[] {
  return errors.map(e => e * 10000);
}
