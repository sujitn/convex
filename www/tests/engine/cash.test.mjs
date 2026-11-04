import test from 'node:test';
import assert from 'node:assert/strict';
import { run, envelope } from './harness.mjs';
import * as K from '../../src/lib/cash.js';
import * as R from '../../src/lib/requests.js';
import { addBusinessDays } from '../../src/lib/calendars.js';

const TRADE = '2026-10-20';
const SETTLE = addBusinessDays('US.SIFMA', TRADE, 1);
const O = { tradeDate: TRADE, settlement: SETTLE };

const bond = (id, coupon, maturity, dayCount = 'Thirty360US') => ({
  id, cusip: id, currency: 'USD', coupon, issueDate: '2025-11-15', datedDate: '2025-11-15', maturity,
  frequency: 'SemiAnnual', dayCount, face: '100', calendar: 'US.SIFMA', settlementDays: 1,
  paymentAdjustment: 'Following', eom: false,
});
const corp = bond('CORP10', '5.00', '2035-11-15');

test('cash quote: clean -> dirty with 30/360 accrued; yield quote prices consistently', () => {
  const q = run(K.cashQuote(corp, { ...O, quote: { kind: 'CleanPrice', value: '101.5' } })).conversion;
  // Last coupon 2026-05-15, settlement 2026-10-21: 156 days of 30/360 on a 5% coupon.
  assert.ok(Math.abs(q.accrued_per100 - (5 * 156) / 360) < 1e-9);
  assert.ok(Math.abs(q.dirty_per100 - (101.5 + q.accrued_per100)) < 1e-9);
  const dirty = run(K.cashQuote(corp, { ...O, quote: { kind: 'DirtyPrice', value: String(q.dirty_per100) } })).conversion;
  assert.ok(Math.abs(dirty.clean_per100 - 101.5) < 1e-6);
  // A 4.75% yield (percent text) prices above par for a 5% coupon; the inverse recovers it.
  const y = run(K.cashQuote(corp, { ...O, quote: { kind: 'CashflowYield', value: '4.75' } })).conversion;
  assert.ok(y.clean_per100 > 100 && y.clean_per100 < 104);
  const conv = K.cashConvention(corp);
  const inv = run(R.bondYieldInverse(corp, SETTLE, { kind: 'clean', value: y.clean_per100 }, conv));
  assert.ok(Math.abs(inv.decimal_parameter - 0.0475) < 1e-8, `${inv.decimal_parameter}`);
});

test('cash invoice scales dirty price by face amount', () => {
  const inv = run(K.cashInvoice(corp, { ...O, faceAmount: '2500000', quote: { kind: 'CleanPrice', value: '101.5' } }));
  assert.equal(inv.consideration.currency, 'USD');
  assert.ok(Math.abs(inv.consideration.value - (2500000 * inv.quotation.conversion.dirty_per100) / 100) < 1e-4);
});

test('Treasury (Act/Act ICMA) bonds quote under the explicit Act/Act ISDA dated-cash basis', () => {
  const ust = bond('UST5', '4.25', '2031-11-15', 'ActActIcma');
  assert.equal(K.cashConvention(ust).day_count, 'ActActIsda');
  const q = run(K.cashQuote(ust, { ...O, quote: { kind: 'CleanPrice', value: '99.5' } })).conversion;
  assert.ok(q.dirty_per100 > q.clean_per100);
});

const bm = [
  { sec: bond('B5', '4.00', '2031-11-15', 'ActActIcma'), quote: { kind: 'CleanPrice', value: '99.5' } },
  { sec: bond('B10', '4.50', '2036-11-15', 'ActActIcma'), quote: { kind: 'CleanPrice', value: '100.25' } },
];

test('linear G-spread: priced at the interpolated benchmark yield -> ~0; inverse round-trips', () => {
  const target = bond('T7', '4.25', '2033-11-15', 'ActActIcma');
  // Price the target at exactly +0bp, then +85.5bp, with the inverse operation.
  const at0 = run(K.governmentSpreadPrice(target, '0', bm, O));
  const p0 = at0.valuation.clean_per100;
  assert.ok(Number.isFinite(p0));
  const gs0 = run(K.governmentSpread(target, { kind: 'CleanPrice', value: String(p0) }, bm, O));
  assert.ok(Math.abs(gs0.basis_points) < 1e-3, `${gs0.basis_points}`);
  const at85 = run(K.governmentSpreadPrice(target, '85.5', bm, O));
  const p85 = at85.valuation.clean_per100;
  assert.ok(p85 < p0, 'wider spread -> lower price');
  const gs85 = run(K.governmentSpread(target, { kind: 'CleanPrice', value: String(p85) }, bm, O));
  assert.ok(Math.abs(gs85.basis_points - 85.5) < 1e-3, `${gs85.basis_points}`);
  assert.ok(Math.abs(gs85.decimal_spread * 1e4 - gs85.basis_points) < 1e-9);
  // Selected-benchmark weights lie between the two supplied securities by maturity.
  assert.equal(gs85.selected.target_maturity, '2033-11-15');
  assert.ok(gs85.selected.lower_weight > 0 && gs85.selected.upper_weight > 0);
  assert.ok(gs85.benchmark_decimal_yield > 0.03 && gs85.benchmark_decimal_yield < 0.06);
});

test('exact G-spread needs one same-maturity benchmark; identical bond and price gives 0', () => {
  const same = bond('X10', '4.50', '2036-11-15', 'ActActIcma');
  const gs = run(K.governmentSpread(same, { kind: 'CleanPrice', value: '100.25' }, [bm[1]], { ...O, selection: 'Exact' }));
  assert.ok(Math.abs(gs.basis_points) < 1e-6);
  const cheaper = run(K.governmentSpread(same, { kind: 'CleanPrice', value: '99.25' }, [bm[1]], { ...O, selection: 'Exact' }));
  assert.ok(cheaper.basis_points > 10);
  const bad = envelope(K.governmentSpread(same, { kind: 'CleanPrice', value: '99.25' }, bm, { ...O, selection: 'Exact' }));
  assert.equal(bad.ok, 'false');
});
