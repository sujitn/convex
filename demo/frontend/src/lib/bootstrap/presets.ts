// =============================================================================
// Instrument Presets
// Sample market data for curve bootstrapping demos
// =============================================================================

import { CalibrationInstrument } from './types';

/**
 * USD SOFR curve - Dec 2024 approximate levels
 * Clean structure: Deposits for short end, OIS/Swaps for longer tenors
 * No overlapping instruments to ensure clean calibration
 */
export const USD_SOFR_STANDARD: CalibrationInstrument[] = [
  { type: 'Deposit', tenor: 0.0833, quote: 0.0435, description: '1M SOFR' },
  { type: 'Deposit', tenor: 0.25, quote: 0.0430, description: '3M SOFR' },
  { type: 'Deposit', tenor: 0.5, quote: 0.0420, description: '6M SOFR' },
  { type: 'OIS', tenor: 1, quote: 0.0395, description: '1Y SOFR OIS' },
  { type: 'OIS', tenor: 2, quote: 0.0365, description: '2Y SOFR OIS' },
  { type: 'Swap', tenor: 3, quote: 0.0355, description: '3Y SOFR Swap' },
  { type: 'Swap', tenor: 5, quote: 0.0360, description: '5Y SOFR Swap' },
  { type: 'Swap', tenor: 7, quote: 0.0370, description: '7Y SOFR Swap' },
  { type: 'Swap', tenor: 10, quote: 0.0385, description: '10Y SOFR Swap' },
  { type: 'Swap', tenor: 20, quote: 0.0410, description: '20Y SOFR Swap' },
  { type: 'Swap', tenor: 30, quote: 0.0405, description: '30Y SOFR Swap' },
];

/**
 * USD SOFR with FRAs - demonstrates forward rate agreements
 * Uses FRAs to fill gaps between deposit and swap tenors
 */
export const USD_SOFR_WITH_FRAS: CalibrationInstrument[] = [
  { type: 'Deposit', tenor: 0.0833, quote: 0.0435, description: '1M SOFR' },
  { type: 'Deposit', tenor: 0.25, quote: 0.0430, description: '3M SOFR' },
  { type: 'FRA', startTenor: 0.25, endTenor: 0.5, quote: 0.0415, description: '3x6 FRA' },
  { type: 'FRA', startTenor: 0.5, endTenor: 0.75, quote: 0.0400, description: '6x9 FRA' },
  { type: 'FRA', startTenor: 0.75, endTenor: 1.0, quote: 0.0385, description: '9x12 FRA' },
  { type: 'OIS', tenor: 2, quote: 0.0365, description: '2Y SOFR OIS' },
  { type: 'Swap', tenor: 5, quote: 0.0360, description: '5Y SOFR Swap' },
  { type: 'Swap', tenor: 10, quote: 0.0385, description: '10Y SOFR Swap' },
];

/**
 * EUR ESTR curve
 */
export const EUR_ESTR: CalibrationInstrument[] = [
  { type: 'Deposit', tenor: 0.0833, quote: 0.0390, description: '1M ESTR' },
  { type: 'Deposit', tenor: 0.25, quote: 0.0385, description: '3M ESTR' },
  { type: 'Deposit', tenor: 0.5, quote: 0.0375, description: '6M ESTR' },
  { type: 'OIS', tenor: 1, quote: 0.0340, description: '1Y ESTR OIS' },
  { type: 'OIS', tenor: 2, quote: 0.0280, description: '2Y ESTR OIS' },
  { type: 'Swap', tenor: 3, quote: 0.0265, description: '3Y ESTR Swap' },
  { type: 'Swap', tenor: 5, quote: 0.0260, description: '5Y ESTR Swap' },
  { type: 'Swap', tenor: 10, quote: 0.0280, description: '10Y ESTR Swap' },
  { type: 'Swap', tenor: 30, quote: 0.0290, description: '30Y ESTR Swap' },
];

/**
 * GBP SONIA curve
 */
export const GBP_SONIA: CalibrationInstrument[] = [
  { type: 'Deposit', tenor: 0.0833, quote: 0.0475, description: '1M SONIA' },
  { type: 'Deposit', tenor: 0.25, quote: 0.0470, description: '3M SONIA' },
  { type: 'Deposit', tenor: 0.5, quote: 0.0455, description: '6M SONIA' },
  { type: 'OIS', tenor: 1, quote: 0.0420, description: '1Y SONIA OIS' },
  { type: 'OIS', tenor: 2, quote: 0.0380, description: '2Y SONIA OIS' },
  { type: 'Swap', tenor: 5, quote: 0.0375, description: '5Y SONIA Swap' },
  { type: 'Swap', tenor: 10, quote: 0.0400, description: '10Y SONIA Swap' },
  { type: 'Swap', tenor: 30, quote: 0.0395, description: '30Y SONIA Swap' },
];

/**
 * Upward sloping curve (normal shape)
 */
export const UPWARD_SLOPING: CalibrationInstrument[] = [
  { type: 'Deposit', tenor: 0.25, quote: 0.0300, description: '3M Deposit' },
  { type: 'Deposit', tenor: 0.5, quote: 0.0330, description: '6M Deposit' },
  { type: 'OIS', tenor: 1, quote: 0.0370, description: '1Y OIS' },
  { type: 'Swap', tenor: 2, quote: 0.0410, description: '2Y Swap' },
  { type: 'Swap', tenor: 5, quote: 0.0470, description: '5Y Swap' },
  { type: 'Swap', tenor: 10, quote: 0.0520, description: '10Y Swap' },
  { type: 'Swap', tenor: 30, quote: 0.0550, description: '30Y Swap' },
];

/**
 * Inverted curve
 */
export const INVERTED_CURVE: CalibrationInstrument[] = [
  { type: 'Deposit', tenor: 0.25, quote: 0.0550, description: '3M Deposit' },
  { type: 'Deposit', tenor: 0.5, quote: 0.0530, description: '6M Deposit' },
  { type: 'OIS', tenor: 1, quote: 0.0500, description: '1Y OIS' },
  { type: 'Swap', tenor: 2, quote: 0.0460, description: '2Y Swap' },
  { type: 'Swap', tenor: 5, quote: 0.0420, description: '5Y Swap' },
  { type: 'Swap', tenor: 10, quote: 0.0400, description: '10Y Swap' },
  { type: 'Swap', tenor: 30, quote: 0.0380, description: '30Y Swap' },
];

/**
 * Flat curve
 */
export const FLAT_CURVE: CalibrationInstrument[] = [
  { type: 'Deposit', tenor: 0.25, quote: 0.0400, description: '3M Deposit' },
  { type: 'Deposit', tenor: 0.5, quote: 0.0400, description: '6M Deposit' },
  { type: 'OIS', tenor: 1, quote: 0.0400, description: '1Y OIS' },
  { type: 'Swap', tenor: 2, quote: 0.0400, description: '2Y Swap' },
  { type: 'Swap', tenor: 5, quote: 0.0400, description: '5Y Swap' },
  { type: 'Swap', tenor: 10, quote: 0.0400, description: '10Y Swap' },
  { type: 'Swap', tenor: 30, quote: 0.0400, description: '30Y Swap' },
];

/**
 * US Treasury bonds for curve fitting
 */
export const US_TREASURIES: CalibrationInstrument[] = [
  { type: 'Bond', coupon: 0, maturity: 0.0833, price: 99.56, frequency: 0, description: '1M T-Bill' },
  { type: 'Bond', coupon: 0, maturity: 0.25, price: 98.68, frequency: 0, description: '3M T-Bill' },
  { type: 'Bond', coupon: 0, maturity: 0.5, price: 97.35, frequency: 0, description: '6M T-Bill' },
  { type: 'Bond', coupon: 0, maturity: 1.0, price: 96.52, frequency: 0, description: '1Y T-Bill' },
  { type: 'Bond', coupon: 0.0375, maturity: 2.0, price: 99.25, frequency: 2, description: '2Y T-Note' },
  { type: 'Bond', coupon: 0.0350, maturity: 3.0, price: 98.80, frequency: 2, description: '3Y T-Note' },
  { type: 'Bond', coupon: 0.0400, maturity: 5.0, price: 100.50, frequency: 2, description: '5Y T-Note' },
  { type: 'Bond', coupon: 0.0375, maturity: 7.0, price: 99.20, frequency: 2, description: '7Y T-Note' },
  { type: 'Bond', coupon: 0.0425, maturity: 10.0, price: 101.20, frequency: 2, description: '10Y T-Note' },
  { type: 'Bond', coupon: 0.0450, maturity: 20.0, price: 100.80, frequency: 2, description: '20Y T-Bond' },
  { type: 'Bond', coupon: 0.0475, maturity: 30.0, price: 102.80, frequency: 2, description: '30Y T-Bond' },
];

/**
 * All preset names with descriptions
 */
export const PRESET_OPTIONS = [
  { id: 'usd-sofr', name: 'USD SOFR (Standard)', instruments: USD_SOFR_STANDARD },
  { id: 'usd-sofr-fras', name: 'USD SOFR (with FRAs)', instruments: USD_SOFR_WITH_FRAS },
  { id: 'eur-estr', name: 'EUR €STR', instruments: EUR_ESTR },
  { id: 'gbp-sonia', name: 'GBP SONIA', instruments: GBP_SONIA },
  { id: 'upward', name: 'Upward Sloping', instruments: UPWARD_SLOPING },
  { id: 'inverted', name: 'Inverted Curve', instruments: INVERTED_CURVE },
  { id: 'flat', name: 'Flat Curve', instruments: FLAT_CURVE },
  { id: 'treasuries', name: 'US Treasuries', instruments: US_TREASURIES },
] as const;

/**
 * Get preset by ID
 */
export function getPreset(id: string): CalibrationInstrument[] | undefined {
  const preset = PRESET_OPTIONS.find(p => p.id === id);
  return preset ? [...preset.instruments] : undefined;
}
