// Treasury bills through the strict `zero` operation. A bill is a zero-coupon
// instrument: `billTerms` builds the library's `ZeroInput`, and the Treasury
// actions (31 CFR 356 Appendix B conventions) quote it from a price, a bank
// discount rate or an investment rate.
//
// Units: this module takes rates as PERCENT text exactly as a user types them
// ("4.215") and converts by moving the decimal point (decimal.js); the native
// request carries annual DECIMAL rates ("0.04215"). Prices are per 100 of
// face; with the default `face: '100'` absolute purchase cash equals the
// per-100 price.

import { request, inputs } from './requests.js';
import { calendarPin } from './calendars.js';
import { decimal, percentToRate, rateToPercent, shift } from './decimal.js';

/**
 * ZeroInput for a Treasury bill. `bill` = `{ cusip, issueDate, maturity,
 * currency='USD', calendar='US.SIFMA', settlementDays=1, face='100' }`.
 * Day count Act/360 and semi-annual compounding are the generic zero
 * settings; the Treasury actions use their own named convention regardless.
 */
export function billTerms(bill) {
  const calendar = bill.calendar || 'US.SIFMA';
  const face = decimal(bill.face || '100');
  return {
    calendar: calendarPin(calendar),
    compounding: 'semi_annual',
    currency: bill.currency || 'USD',
    day_count: 'Act360',
    face_value: face,
    identifiers: {
      cusip: bill.cusip || null,
      figi: null,
      isin: null,
      issue_name: bill.issueName || null,
      issuer_name: bill.issuerName || null,
      sedol: null,
      ticker: null,
    },
    issue_date: bill.issueDate,
    issue_price: null,
    maturity: bill.maturity,
    redemption_value: face,
    settlement_days: Number(bill.settlementDays ?? 1),
  };
}

function zeroRequest(bill, action) {
  return request(inputs({ calendars: [bill.calendar || 'US.SIFMA'] }), {
    kind: 'zero',
    action,
    input: billTerms(bill),
  });
}

export const billConstruct = (bill) => zeroRequest(bill, { kind: 'construct' });

/** Treasury-convention quote action; `quote` is the native `TreasuryBillQuote`. */
export function billQuote(bill, settlement, quote) {
  return zeroRequest(bill, { kind: 'treasury_bill', settlement, quote });
}

/**
 * From a bank discount rate in percent ("4.20" = 4.20%). Response
 * `treasury_bill` carries both rates (decimal) and the price; see `billRead`.
 */
export const billFromDiscountRate = (bill, settlement, discountPercent) =>
  billQuote(bill, settlement, { kind: 'bank_discount', decimal_rate: percentToRate(discountPercent) });

/** From a bond-equivalent (investment) rate in percent. */
export const billFromInvestmentRate = (bill, settlement, investmentPercent) =>
  billQuote(bill, settlement, { kind: 'investment', decimal_rate: percentToRate(investmentPercent) });

/** From a purchase price per 100 of face (scaled to original face when face != 100). */
export function billFromPrice(bill, settlement, pricePer100) {
  return billQuote(bill, settlement, { kind: 'price', price_original_face: scaleToFace(pricePer100, bill.face || '100') });
}

// Canonical decimal text → [signed integer of all its digits, digits after the point].
function scaled(text) {
  const [i, f = ''] = text.split('.');
  return [BigInt(i + f), f.length];
}

// price/100 × face as exact decimal text: the product of both digit strings,
// with the point moved back by both scales and the per-100 divisor.
export function scaleToFace(pricePer100, face) {
  const p = decimal(pricePer100);
  const f = decimal(face);
  if (f.startsWith('-') || !/[1-9]/.test(f)) throw new Error(`face ${face} must be a positive amount`);
  if (f === '100') return p;
  const [pi, ps] = scaled(p);
  const [fi, fs] = scaled(f);
  return shift((pi * fi).toString(), ps + fs + 2);
}

/**
 * Legacy generic conversions: discount yield and bond-equivalent yield from a
 * price per 100 (original-face price for face 100). Native BEY admits 1..182
 * remaining days only. Outputs are PERCENT decimal strings.
 */
export const billConversions = (bill, settlement, pricePer100) =>
  zeroRequest(bill, { kind: 'bill_conversions', settlement, price_original_face: scaleToFace(pricePer100, bill.face || '100') });

/** Generic zero pricing from a clean price per 100 (semi-annual compounding yield, not the Treasury convention). */
export const billGenericFromPrice = (bill, settlement, cleanPer100) =>
  zeroRequest(bill, { kind: 'quote_price', settlement, clean_price_per100: decimal(cleanPer100) });

/** Generic zero pricing from an annual decimal yield with explicit compounding frequency. */
export const billGenericFromYield = (bill, settlement, yieldPercent, frequency = 'SemiAnnual') =>
  zeroRequest(bill, { kind: 'quote_yield', settlement, decimal_yield: percentToRate(yieldPercent), frequency });

/**
 * Flatten a `treasury_bill` response (`result.data.calculation`) for display.
 * Exact decimal STRINGS; percent fields are percent (4.2 = 4.2%), per-100
 * fields are price points, `pricePerUp1bp*` is the signed price change per
 * 100 face for a +1bp move in that quote (negative: price falls as rates rise).
 */
export function billResult(data) {
  const c = data.calculation;
  return {
    pricePer100: c.price_per100,
    auctionPricePer100: c.auction_price_per100, // rounded to 6 dp, midpoint away from zero
    discountPercent: rateToPercent(c.bank_discount_decimal),
    investmentPercent: rateToPercent(c.investment_decimal),
    remainingDays: c.remaining_days,
    basisDays: c.basis_days, // 365 or 366: days in the year from original issue
    pricePerUp1bpDiscount: c.bank_discount_risk.signed_price_per100_per_up_1bp,
    pricePerUp1bpInvestment: c.investment_risk.signed_price_per100_per_up_1bp,
    method: c.method, // 'simple' (<= half year) or the long-bill quadratic branch
  };
}
