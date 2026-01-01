// =============================================================================
// Bond Pricing for Curve Fitting
// Price government bonds to extract zero rates
// =============================================================================

import {
  BondInstrument,
  BootstrappedCurve,
} from './types';
import { getDiscountFactor } from './curve';

/**
 * Bond cash flow
 */
export interface BondCashFlow {
  time: number;       // Years from settlement
  amount: number;     // Cash flow amount (coupon or principal)
  type: 'coupon' | 'principal';
}

/**
 * Generate cash flows for a bond
 */
export function generateBondCashFlows(
  inst: BondInstrument,
  faceValue: number = 100
): BondCashFlow[] {
  const cashFlows: BondCashFlow[] = [];
  const frequency = inst.frequency || 2;  // Default semi-annual
  const period = 1 / frequency;
  const couponAmount = (inst.coupon * faceValue) / frequency;

  // Generate coupon payments
  let t = period;
  while (t < inst.maturity) {
    cashFlows.push({
      time: t,
      amount: couponAmount,
      type: 'coupon',
    });
    t += period;
  }

  // Final payment (coupon + principal)
  if (inst.coupon > 0) {
    cashFlows.push({
      time: inst.maturity,
      amount: couponAmount + faceValue,
      type: 'principal',
    });
  } else {
    // Zero coupon bond
    cashFlows.push({
      time: inst.maturity,
      amount: faceValue,
      type: 'principal',
    });
  }

  return cashFlows;
}

/**
 * Calculate model price of a bond given a curve
 * Model Price = Σ CF_i * DF(T_i)
 */
export function calculateBondModelPrice(
  inst: BondInstrument,
  curve: BootstrappedCurve,
  faceValue: number = 100
): number {
  const cashFlows = generateBondCashFlows(inst, faceValue);

  let modelPrice = 0;
  for (const cf of cashFlows) {
    const df = getDiscountFactor(curve, cf.time);
    modelPrice += cf.amount * df;
  }

  return modelPrice;
}

/**
 * Calculate pricing error for a bond
 * Error = Model Price - Market Price
 *
 * For curve fitting, we want to minimize (Model Price - Market Price)^2
 */
export function bondPricingError(
  inst: BondInstrument,
  curve: BootstrappedCurve,
  faceValue: number = 100
): number {
  const modelPrice = calculateBondModelPrice(inst, curve, faceValue);
  return modelPrice - inst.price;
}

/**
 * Calculate bond DV01 (dollar value of 1bp)
 * Approximated by finite difference
 */
export function bondDV01(
  inst: BondInstrument,
  curve: BootstrappedCurve,
  faceValue: number = 100
): number {
  // This is a simplified calculation
  // Full DV01 requires bumping the curve
  const cashFlows = generateBondCashFlows(inst, faceValue);

  let weightedDuration = 0;
  let price = 0;

  for (const cf of cashFlows) {
    const df = getDiscountFactor(curve, cf.time);
    const pv = cf.amount * df;
    price += pv;
    weightedDuration += cf.time * pv;
  }

  // Modified duration * price / 10000 (for 1bp)
  if (price < 0.01) return 0;
  const modDuration = weightedDuration / price;
  return modDuration * price / 10000;
}

/**
 * Calculate yield to maturity for a bond
 * Uses Newton-Raphson iteration
 */
export function calculateBondYTM(
  inst: BondInstrument,
  faceValue: number = 100,
  tolerance: number = 1e-8,
  maxIterations: number = 100
): number {
  const cashFlows = generateBondCashFlows(inst, faceValue);

  // Initial guess based on current yield
  let y = inst.coupon > 0
    ? (inst.coupon * faceValue) / inst.price
    : Math.log(faceValue / inst.price) / inst.maturity;

  for (let iter = 0; iter < maxIterations; iter++) {
    let pv = 0;
    let dpv = 0;

    for (const cf of cashFlows) {
      const df = Math.exp(-y * cf.time);
      pv += cf.amount * df;
      dpv -= cf.time * cf.amount * df;
    }

    const error = pv - inst.price;
    if (Math.abs(error) < tolerance) {
      return y;
    }

    if (Math.abs(dpv) < 1e-15) {
      break;
    }

    y = y - error / dpv;

    // Clamp to reasonable range
    y = Math.max(-0.1, Math.min(0.5, y));
  }

  return y;
}

/**
 * Calculate Macaulay duration for a bond
 */
export function calculateBondDuration(
  inst: BondInstrument,
  curve: BootstrappedCurve,
  faceValue: number = 100
): number {
  const cashFlows = generateBondCashFlows(inst, faceValue);

  let weightedTime = 0;
  let totalPV = 0;

  for (const cf of cashFlows) {
    const df = getDiscountFactor(curve, cf.time);
    const pv = cf.amount * df;
    weightedTime += cf.time * pv;
    totalPV += pv;
  }

  return totalPV > 0 ? weightedTime / totalPV : 0;
}

/**
 * Calculate modified duration for a bond
 */
export function calculateBondModifiedDuration(
  inst: BondInstrument,
  curve: BootstrappedCurve,
  faceValue: number = 100
): number {
  const macDuration = calculateBondDuration(inst, curve, faceValue);
  const ytm = calculateBondYTM(inst, faceValue);
  const frequency = inst.frequency || 2;

  return macDuration / (1 + ytm / frequency);
}

/**
 * Price a zero-coupon bond (T-Bill)
 * For T-Bills, the market quote is often a discount yield
 * Price = Face / (1 + y * t) for simple yield
 * or Price = Face * exp(-y * t) for continuous
 */
export function calculateZeroCouponPrice(
  maturity: number,
  discountYield: number,
  faceValue: number = 100,
  useSimpleInterest: boolean = true
): number {
  if (useSimpleInterest) {
    return faceValue / (1 + discountYield * maturity);
  }
  return faceValue * Math.exp(-discountYield * maturity);
}

/**
 * Extract zero rate from a T-Bill price
 */
export function extractZeroRateFromTBill(
  price: number,
  maturity: number,
  faceValue: number = 100
): number {
  if (maturity <= 0) return 0;
  // Continuous compounding
  return -Math.log(price / faceValue) / maturity;
}
