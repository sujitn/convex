// Scalar yield measures through the strict `yield_scalar` operation: current
// yield and the arithmetic simple-yield estimate. These are quick screening
// numbers, not instrument yields: no day count, schedule, stub or calendar is
// applied (see docs/YIELD-SCALARS.md).
//
// Units: inputs are per-100 amounts as typed (coupon "4.875" is both the
// percent rate and the annual cash per 100 of face); every output is a
// PERCENT decimal string (4.8 = 4.8%).

import { request, inputs } from './requests.js';
import { decimal, percentToRate } from './decimal.js';
import { parseIso } from './dates.js';

function scalarRequest(action) {
  return request(inputs(), { kind: 'yield_scalar', action });
}

/**
 * Current yield = annual coupon / clean price, for a constant-coupon bond of
 * any original face (`sec.face`); price per 100. Result `{unit:'percent',
 * representation:'decimal', value:'4.80...'}`.
 */
export const currentYield = (sec, cleanPricePer100) =>
  scalarRequest({
    kind: 'fixed_current_percent',
    annual_rate: percentToRate(sec.coupon),
    original_face: decimal(sec.face || '100'),
    clean_price_per100: decimal(cleanPricePer100),
  });

/** Whole-year fraction between two ISO dates on an ACT/365 basis, 6 decimals (the caller's explicit choice). */
export function yearsAct365(from, to) {
  const days = (parseIso(to).getTime() - parseIso(from).getTime()) / 86400000;
  if (!(days > 0)) throw new Error('maturity must be after settlement');
  return (days / 365).toFixed(6);
}

/**
 * Simple yield = (coupon + (par - price) / years) / price, per 100 and
 * undiscounted. `years` is explicit (default ACT/365 from settlement).
 * Par is the redemption per 100 (`sec.redemption`, default 100).
 */
export function simpleYield(sec, settlement, cleanPricePer100, years) {
  return scalarRequest({
    kind: 'simple_decimal_percent',
    annual_coupon: decimal(sec.coupon),
    clean_price: decimal(cleanPricePer100),
    par_value: decimal(sec.redemption || '100'),
    years_to_maturity: years ?? yearsAct365(settlement, sec.maturity),
  });
}
