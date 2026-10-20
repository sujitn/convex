// The security universe: actual Treasury securities from auction results,
// plus editable example securities for the structures Treasuries don't have
// (callable, stepped, sinking). Examples are labelled as such everywhere.

import { yearsBetween, formatShort, formatLong } from '../lib/dates.js';

const FRACTIONS = { 0: '', 0.125: '⅛', 0.25: '¼', 0.375: '⅜', 0.5: '½', 0.625: '⅝', 0.75: '¾', 0.875: '⅞' };

/** "4.625" → "4⅝", the conventional coupon shorthand. */
export function couponShort(coupon) {
  const c = Number(coupon);
  if (!Number.isFinite(c)) return '';
  const whole = Math.floor(c);
  const frac = +(c - whole).toFixed(3);
  return frac in FRACTIONS ? `${whole}${FRACTIONS[frac]}` : String(+c.toFixed(3));
}

export function termYears(term) {
  if (!term) return NaN;
  const y = /(\d+)-Year/.exec(term);
  const m = /(\d+)-Month/.exec(term);
  const w = /(\d+)-Week/.exec(term);
  return (y ? +y[1] : 0) + (m ? +m[1] / 12 : 0) + (w ? (+w[1] * 7) / 365 : 0);
}

const BUCKETS = [2, 3, 5, 7, 10, 20, 30];
export function benchmarkBucket(sec) {
  const t = termYears(sec.term);
  return BUCKETS.reduce((best, b) => (Math.abs(b - t) < Math.abs(best - t) ? b : best), BUCKETS[0]);
}

export function treasuryFromAuction(a) {
  const bill = a.type === 'Bill';
  const base = {
    id: `UST-${a.cusip}`,
    source: 'auction',
    cusip: a.cusip,
    isin: `US${a.cusip}${isinCheck(`US${a.cusip}`)}`,
    issuer: 'United States Treasury',
    type: a.type,
    term: a.term,
    currency: 'USD',
    calendar: 'US.SIFMA',
    settlementDays: 1,
    issueDate: a.issueDate,
    maturity: a.maturity,
    face: '100',
    auction: { date: a.auctionDate, price: a.pricePer100, highYield: a.highYield,
      discountRate: a.highDiscountRate, investmentRate: a.highInvestmentRate, reopening: a.reopening },
  };
  if (bill) {
    return { ...base, kind: 'bill', name: `US Treasury Bill ${formatShort(a.maturity)}`,
      ticker: `B 0 ${formatShort(a.maturity)}` };
  }
  return {
    ...base,
    kind: 'fixed',
    name: `US Treasury ${a.type} ${Number(a.coupon).toString()}% ${formatLong(a.maturity)}`,
    ticker: `T ${couponShort(a.coupon)} ${formatShort(a.maturity)}`,
    coupon: a.coupon.replace(/0+$/, '').replace(/\.$/, ''),
    datedDate: a.datedDate || a.issueDate,
    firstCouponDate: a.firstCouponDate || null,
    frequency: 'SemiAnnual',
    dayCount: 'ActActIcma',
    paymentAdjustment: 'Following',
    eom: 'auto',
    redemption: '100',
  };
}

/** ISIN check digit (Luhn over the letter-expanded string). */
export function isinCheck(body) {
  const digits = body.replace(/[A-Z]/g, (c) => String(c.charCodeAt(0) - 55));
  let sum = 0;
  for (let i = 0; i < digits.length; i++) {
    let d = Number(digits[digits.length - 1 - i]);
    if (i % 2 === 0) {
      d *= 2;
      if (d > 9) d -= 9;
    }
    sum += d;
  }
  return String((10 - (sum % 10)) % 10);
}

/** Latest-issued nominal coupon security per benchmark bucket (2/3/5/7/10/20/30Y). */
export function onTheRun(treasuries) {
  const out = {};
  for (const s of treasuries) {
    if (s.kind !== 'fixed') continue;
    const b = benchmarkBucket(s);
    const prev = out[b];
    if (!prev || s.auction.date > prev.auction.date) out[b] = s;
  }
  return Object.keys(out)
    .map(Number)
    .sort((a, b) => a - b)
    .map((b) => ({ bucket: b, sec: out[b] }));
}

/** Unique securities (reopenings collapse onto the latest auction). */
export function treasuryUniverse(dataset) {
  if (!dataset) return [];
  const byCusip = new Map();
  for (const a of dataset.securities || []) {
    const s = treasuryFromAuction(a);
    const prev = byCusip.get(s.cusip);
    if (!prev || s.auction.date > prev.auction.date) byCusip.set(s.cusip, s);
  }
  return [...byCusip.values()].sort((a, b) => (a.maturity < b.maturity ? -1 : 1));
}

export function yearsToMaturity(sec, from) {
  return yearsBetween(from, sec.maturity);
}

/** Editable example structures. Issuer names say "Example" on purpose. */
export const EXAMPLES = [
  {
    id: 'EX-CALL-2034',
    source: 'example',
    kind: 'fixed',
    issuer: 'Example Industrials Inc. (hypothetical)',
    name: 'Example Industrials 5.25% 2034, callable',
    ticker: 'EXIND 5¼ 03/15/34',
    currency: 'USD',
    coupon: '5.25',
    issueDate: '2024-03-15',
    datedDate: '2024-03-15',
    firstCouponDate: null,
    maturity: '2034-03-15',
    frequency: 'SemiAnnual',
    dayCount: 'Thirty360US',
    face: '100',
    calendar: 'US.SIFMA',
    settlementDays: 1,
    paymentAdjustment: 'Following',
    eom: 'auto',
    redemption: '100',
    calls: [
      { date: '2027-03-15', price: '102' },
      { date: '2028-03-15', price: '101' },
      { date: '2029-03-15', price: '100' },
      { date: '2030-03-15', price: '100' },
      { date: '2031-03-15', price: '100' },
      { date: '2032-03-15', price: '100' },
      { date: '2033-03-15', price: '100' },
    ],
  },
  {
    id: 'EX-STEP-2031',
    source: 'example',
    kind: 'fixed',
    issuer: 'Example Utilities Co. (hypothetical)',
    name: 'Example Utilities step-up 2031',
    ticker: 'EXUTL STEP 06/01/31',
    currency: 'USD',
    coupon: '4.00',
    issueDate: '2025-06-01',
    datedDate: '2025-06-01',
    firstCouponDate: null,
    maturity: '2031-06-01',
    frequency: 'SemiAnnual',
    dayCount: 'Thirty360US',
    face: '100',
    calendar: 'US.SIFMA',
    settlementDays: 1,
    paymentAdjustment: 'Following',
    eom: 'auto',
    redemption: '100',
    steps: [
      { date: '2027-06-01', coupon: '4.50' },
      { date: '2029-06-01', coupon: '5.25' },
    ],
  },
  {
    id: 'EX-SINK-2036',
    source: 'example',
    kind: 'fixed',
    issuer: 'Example Port Authority (hypothetical)',
    name: 'Example Port Authority 4.75% 2036, sinking fund',
    ticker: 'EXPORT 4¾ 12/01/36',
    currency: 'USD',
    coupon: '4.75',
    issueDate: '2024-12-01',
    datedDate: '2024-12-01',
    firstCouponDate: null,
    maturity: '2036-12-01',
    frequency: 'SemiAnnual',
    dayCount: 'Thirty360US',
    face: '100',
    calendar: 'US.SIFMA',
    settlementDays: 1,
    paymentAdjustment: 'Following',
    eom: 'auto',
    redemption: '100',
    sinking: [
      { date: '2032-12-01', amount: '20' },
      { date: '2033-12-01', amount: '20' },
      { date: '2034-12-01', amount: '20' },
      { date: '2035-12-01', amount: '20' },
    ],
  },
  {
    id: 'EX-EUR-2035',
    source: 'example',
    kind: 'fixed',
    issuer: 'Example Euro Sovereign (hypothetical)',
    name: 'Example Euro Sovereign 3.10% 2035',
    ticker: 'EXEUR 3.1 02/15/35',
    currency: 'EUR',
    coupon: '3.10',
    issueDate: '2025-02-15',
    datedDate: '2025-02-15',
    firstCouponDate: null,
    maturity: '2035-02-15',
    frequency: 'Annual',
    dayCount: 'ActActIcma',
    face: '100',
    calendar: 'EU.TARGET2',
    settlementDays: 2,
    paymentAdjustment: 'Following',
    eom: 'auto',
    redemption: '100',
  },
];

export function curveForCurrency(currency) {
  return { USD: 'USD.UST', EUR: 'EUR.AAA', CAD: 'CAD.GOC' }[currency] || null;
}

export function describe(sec) {
  if (!sec) return '';
  return sec.ticker || sec.name;
}
