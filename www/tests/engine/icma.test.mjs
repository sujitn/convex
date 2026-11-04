// The ICMA nominal mesh must be the engine's own anchored grid: on the first
// regular coupon date when one is given, otherwise on maturity.
import test from 'node:test';
import assert from 'node:assert/strict';
import { run } from './harness.mjs';
import * as R from '../../src/lib/requests.js';
import { icmaPeriods, isMonthEnd } from '../../src/lib/terms.js';
import { addMonths, compareIso } from '../../src/lib/dates.js';

const SETTLE = '2026-10-21';
const bond = (over) => ({
  id: 'ICMA.TEST', cusip: 'ICMATEST1', currency: 'USD', coupon: '5', frequency: 'SemiAnnual', dayCount: 'ActActIcma',
  face: '100', calendar: 'US.SIFMA', settlementDays: 1, paymentAdjustment: 'Following', eom: 'auto', ...over,
});

function priced(sec) {
  const y = run(R.icmaYield(sec, SETTLE, { kind: 'clean', value: 99 }, 'SemiAnnual', '1000000'));
  const back = run(R.icmaPrice(sec, SETTLE, y.quotation.value.decimal_parameter, 'SemiAnnual', '1000000'));
  assert.ok(Math.abs(back.quotation.value.clean_per100 - 99) < 1e-8);
  return y;
}

test('a long first coupon: the mesh covers both reference periods of the first coupon', () => {
  const sec = bond({ issueDate: '2026-07-01', datedDate: '2026-07-01', firstCouponDate: '2027-04-15', maturity: '2031-10-15' });
  const periods = icmaPeriods(sec);
  assert.ok(compareIso(periods[0].start, '2026-07-01') < 0);
  assert.ok(periods.some((p) => p.start === '2026-10-15' && p.end === '2027-04-15'));
  priced(sec);
});

test('a short first coupon whose grid differs from the maturity grid', () => {
  // Rolling from the first coupon (30 Aug) gives 28/29 Feb and 30 Aug; rolling
  // back from a month-end maturity (28 Feb) would give 31 Aug instead.
  const sec = bond({ issueDate: '2026-03-16', datedDate: '2026-03-16', firstCouponDate: '2026-08-30', maturity: '2031-02-28' });
  assert.equal(isMonthEnd(sec.maturity), true);
  const periods = icmaPeriods(sec);
  for (const p of periods) {
    const k = Math.round((Date.parse(p.end) - Date.parse('2026-08-30')) / (182.6 * 86400000));
    assert.equal(p.end, addMonths('2026-08-30', 6 * k), `${p.end} is on the first-coupon grid`);
  }
  assert.ok(periods.some((p) => p.end === '2028-02-29'));
  priced(sec);
});

test('without a first coupon date the mesh is anchored on maturity', () => {
  const sec = bond({ issueDate: '2025-11-15', datedDate: '2025-11-15', maturity: '2035-11-15' });
  const periods = icmaPeriods(sec);
  assert.ok(periods.every((p) => p.end.slice(5) === '05-15' || p.end.slice(5) === '11-15'));
  priced(sec);
});
