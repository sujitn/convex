// Security definitions (the screens' editable model) → the library's full
// `BondTerms`. Every contractual date rule, calendar and principal profile is
// stated explicitly; nothing is defaulted inside the engine.

import { addMonths, daysInMonth, parseIso, compareIso } from './dates.js';
import { calendarPin, addBusinessDays } from './calendars.js';
import { decimal, percentToRate, shift } from './decimal.js';

export const FREQ_MONTHS = { Annual: 12, SemiAnnual: 6, Quarterly: 3, Monthly: 1 };
export const FREQ_PER_YEAR = { Annual: 1, SemiAnnual: 2, Quarterly: 4, Monthly: 12 };

export function isMonthEnd(iso) {
  const d = parseIso(iso);
  return d.getUTCDate() === daysInMonth(d.getUTCFullYear(), d.getUTCMonth() + 1);
}

function monthEnd(iso) {
  const d = parseIso(iso);
  const y = d.getUTCFullYear();
  const m = d.getUTCMonth() + 1;
  return `${y}-${String(m).padStart(2, '0')}-${String(daysInMonth(y, m)).padStart(2, '0')}`;
}

export function endOfMonthRule(sec) {
  return sec.eom === 'auto' || sec.eom === undefined ? isMonthEnd(sec.maturity) : !!sec.eom;
}

/** Regular dates anchored on maturity, stepping back `k` periods (EOM-aware). */
export function anchoredDate(maturity, months, k, eom) {
  const d = addMonths(maturity, -months * k);
  return eom ? monthEnd(d) : d;
}

/** Contractual coupon dates after `from` up to maturity, anchored on maturity. */
export function couponDates(sec, from) {
  const months = FREQ_MONTHS[sec.frequency];
  if (!months) return [sec.maturity];
  const eom = endOfMonthRule(sec);
  const out = [];
  for (let k = 0; k < 2000; k++) {
    const d = anchoredDate(sec.maturity, months, k, eom);
    if (compareIso(d, from) <= 0) break;
    out.unshift(d);
  }
  return out;
}

/** Piecewise-constant profile over [start, end) from dated change points. */
function profile(start, end, base, changes = []) {
  const pts = changes
    .filter((c) => compareIso(c.date, start) > 0 && compareIso(c.date, end) < 0)
    .sort((a, b) => compareIso(a.date, b.date));
  const segs = [];
  let from = start;
  let value = base;
  for (const c of pts) {
    segs.push({ start: from, end: c.date, value });
    from = c.date;
    value = c.value;
  }
  segs.push({ start: from, end, value });
  return segs;
}

/** Outstanding-notional changes implied by a sinking-fund schedule (face units). */
function notionalChanges(sec) {
  let outstanding = decimal(sec.face);
  const out = [];
  for (const s of [...(sec.sinking || [])].sort((a, b) => compareIso(a.date, b.date))) {
    outstanding = subtractDecimal(outstanding, decimal(s.amount));
    out.push({ date: s.date, value: outstanding });
  }
  return out;
}

/** Exact decimal subtraction on canonical strings (scale-aligned BigInt). */
export function subtractDecimal(a, b) {
  const scale = Math.max(fracLen(a), fracLen(b));
  const ai = BigInt(shift(a, -scale).replace(/\..*$/, ''));
  const bi = BigInt(shift(b, -scale).replace(/\..*$/, ''));
  return shift((ai - bi).toString(), scale);
}

function fracLen(s) {
  const i = s.indexOf('.');
  return i < 0 ? 0 : s.length - i - 1;
}

export function accrualStart(sec) {
  return sec.datedDate || sec.issueDate;
}

function calendarIds(code) {
  const id = calendarPin(code).id;
  return { exercise: id, observation: id, payment: id, reset: id };
}

export function calendarPins(code) {
  const pin = calendarPin(code);
  return { exercise: pin, observation: pin, payment: pin, reset: pin };
}

function exerciseTerms(sec) {
  const calls = (sec.calls || []).filter((c) => c.date && c.price);
  if (!calls.length) return [];
  const lag = { BusinessDays: 0 };
  const base = {
    accrued: 'pay_accrued',
    notice_offset: { CalendarDays: 0 },
    protection_end: null,
    right: 'call',
    settlement_adjustment: 'Following',
    settlement_lag: lag,
  };
  // Each distinct price is its own exercise provision; dates with the same
  // price form one Bermudan (or European, when single) schedule.
  const byPrice = new Map();
  for (const c of [...calls].sort((a, b) => compareIso(a.date, b.date))) {
    const p = decimal(c.price);
    if (!byPrice.has(p)) byPrice.set(p, []);
    byPrice.get(p).push(c.date);
  }
  return [...byPrice.entries()].map(([price, dates]) => ({
    ...base,
    price_per_100: price,
    style: dates.length === 1 ? { style: 'european', date: dates[0] } : { style: 'bermudan', dates },
  }));
}

/** Full `BondTerms` for a fixed-coupon (bullet, stepped, sinking, callable) bond. */
export function bondTerms(sec) {
  const start = accrualStart(sec);
  const end = sec.maturity;
  const coupon = percentToRate(sec.coupon);
  const steps = (sec.steps || [])
    .filter((s) => s.date && s.coupon !== '')
    .map((s) => ({ date: s.date, value: percentToRate(s.coupon) }));
  const face = decimal(sec.face || '100');
  const zeroProfile = [{ start, end, value: '0' }];
  return {
    calendars: calendarIds(sec.calendar),
    coupon: { kind: 'fixed' },
    coupon_principal: 'AccrualProfile',
    currency: sec.currency,
    dates: {
      accrual_adjustment: 'Unadjusted',
      ex_coupon: null,
      fixing_offset: null,
      observation_offset: null,
      payment_adjustment: sec.paymentAdjustment || 'Following',
      payment_delay: { CalendarDays: 0 },
    },
    day_count: sec.dayCount,
    ex_coupon_accrual: 'SubtractCoupon',
    exercise: exerciseTerms(sec),
    face_value: face,
    issue_date: sec.issueDate,
    negative_coupons: 'Allow',
    principal_prices: [],
    profiles: {
      coupon: profile(start, end, coupon, steps),
      gearing: zeroProfile,
      notional: profile(start, end, face, notionalChanges(sec)),
      spread: zeroProfile,
    },
    redemption_multiplier: shift(decimal(sec.redemption || '100'), 2),
    rounding: 'Unrounded',
    schedule: {
      end_date: end,
      end_of_month: endOfMonthRule(sec),
      first_regular_date: sec.firstCouponDate || null,
      frequency: sec.frequency,
      penultimate_date: null,
      start_date: start,
      stub_type: 'ShortFirst',
    },
    schema_version: 1,
    settlement_adjustment: 'Following',
    settlement_lag: { BusinessDays: Number(sec.settlementDays ?? 1) },
  };
}

/** The `{ calendars, terms }` bond object library operations take. */
export function bondInput(sec) {
  return { calendars: calendarPins(sec.calendar), terms: bondTerms(sec) };
}

export function settlementFor(sec, tradeDate) {
  return addBusinessDays(sec.calendar, tradeDate, Number(sec.settlementDays ?? 1));
}

/**
 * The complete ICMA nominal determination mesh: regular periods anchored on
 * maturity, from two periods before the accrual start to two past maturity.
 */
export function icmaPeriods(sec) {
  const months = FREQ_MONTHS[sec.frequency];
  const eom = endOfMonthRule(sec);
  const start = accrualStart(sec);
  const dates = [];
  let before = 0;
  for (let k = -2; k < 4000; k++) {
    const d = anchoredDate(sec.maturity, months, k, eom);
    dates.unshift(d);
    if (compareIso(d, start) < 0 && ++before >= 2) break;
  }
  const periods = [];
  for (let i = 0; i + 1 < dates.length; i++) periods.push({ start: dates[i], end: dates[i + 1] });
  return periods;
}

/** Stable identifier for pins derived from the security. */
export function securityId(sec) {
  return sec.cusip || sec.isin || sec.id || 'SECURITY';
}

export function annualCouponPer100(sec) {
  return Number(sec.coupon);
}
