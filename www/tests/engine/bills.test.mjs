import test from 'node:test';
import assert from 'node:assert/strict';
import { run, envelope } from './harness.mjs';
import * as B from '../../src/lib/bills.js';
import { addBusinessDays } from '../../src/lib/calendars.js';

const TRADE = '2026-10-20';
const SETTLE = addBusinessDays('US.SIFMA', TRADE, 1); // T+1

// Valid 9-character CUSIP from an 8-character base (the library checks the digit).
function cusip(b8) {
  let sum = 0;
  [...b8].forEach((c, i) => {
    let v = /\d/.test(c) ? Number(c) : c.charCodeAt(0) - 55;
    if (i % 2 === 1) v *= 2;
    sum += Math.floor(v / 10) + (v % 10);
  });
  return b8 + ((10 - (sum % 10)) % 10);
}

const bill = (maturity, issueDate = '2026-07-23') => ({
  cusip: cusip('912797XX'), issueDate, maturity, currency: 'USD', calendar: 'US.SIFMA', settlementDays: 1, face: '100',
});
const days = (a, b) => Math.round((Date.parse(b) - Date.parse(a)) / 86400000);

test('discount rate -> price -> BEY matches 365*d/(360 - d*t) for a short bill', () => {
  const b = bill('2027-01-21'); // 92 days from settlement
  const t = days(SETTLE, b.maturity);
  assert.ok(t <= 182);
  const r = B.billResult(run(B.billFromDiscountRate(b, SETTLE, '4.20')));
  const d = 0.042;
  assert.equal(r.remainingDays, t);
  assert.ok(Math.abs(Number(r.pricePer100) - 100 * (1 - (d * t) / 360)) < 1e-9);
  const bey = (365 * d) / (360 - d * t);
  assert.ok(Math.abs(Number(r.investmentPercent) / 100 - bey) < 1e-9, `${r.investmentPercent} vs ${bey * 100}`);
  assert.ok(Math.abs(Number(r.discountPercent) - 4.2) < 1e-12); // exact decimal, may carry 28 digits (4.1999...)
  // Round trip: the investment rate quotes the same price back.
  const back = B.billResult(run(B.billFromInvestmentRate(b, SETTLE, r.investmentPercent.slice(0, 25))));
  assert.ok(Math.abs(Number(back.pricePer100) - Number(r.pricePer100)) < 1e-9);
  // Price quote returns the same rates.
  const fromPrice = B.billResult(run(B.billFromPrice(b, SETTLE, r.pricePer100.slice(0, 20))));
  assert.ok(Math.abs(Number(fromPrice.discountPercent) - 4.2) < 1e-9);
  assert.ok(Number(r.pricePerUp1bpDiscount) < 0);
});

test('bill_conversions agree with the Treasury route (percent outputs)', () => {
  const b = bill('2027-01-21');
  const price = '98.8';
  const conv = run(B.billConversions(b, SETTLE, price));
  const tb = B.billResult(run(B.billFromPrice(b, SETTLE, price)));
  assert.ok(Math.abs(Number(conv.discount_yield_percent) - Number(tb.discountPercent)) < 1e-9);
  assert.ok(Math.abs(Number(conv.bond_equivalent_yield_percent) - Number(tb.investmentPercent)) < 1e-9);
  // Generic semi-annual yield quote prices the same clean value back.
  const g = run(B.billGenericFromPrice(b, SETTLE, price));
  assert.equal(g.pricing.clean_price_per_100, 98.8);
});

test('52-week bill uses the long-bill branch; native rejects a bad CUSIP', () => {
  const b = bill('2027-10-14', '2026-10-15');
  const r = B.billResult(run(B.billFromDiscountRate(b, SETTLE, '4.10')));
  assert.ok(r.remainingDays > 182);
  assert.ok(Number(r.investmentPercent) > 4.1);
  const env = envelope(B.billFromDiscountRate({ ...b, cusip: '912797XXX' }, SETTLE, '4.10'));
  assert.equal(env.ok, 'false');
  assert.equal(env.error.code, 'invalid_input');
});
