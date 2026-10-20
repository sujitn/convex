import test from 'node:test';
import assert from 'node:assert/strict';
import { run, envelope } from './harness.mjs';
import * as S from '../../src/lib/scalars.js';
import { addBusinessDays } from '../../src/lib/calendars.js';

const TRADE = '2026-10-20';
const SETTLE = addBusinessDays('US.SIFMA', TRADE, 1);
const sec = { coupon: '4.375', face: '100', redemption: '100', maturity: '2031-10-31' };

test('current yield: coupon / clean price, percent', () => {
  const r = run(S.currentYield(sec, '98.5'));
  assert.equal(r.unit, 'percent');
  assert.equal(r.representation, 'decimal');
  assert.ok(Math.abs(Number(r.value) - (4.375 / 98.5) * 100) < 1e-12, r.value);
  // Face does not change the per-100 result.
  const big = run(S.currentYield({ ...sec, face: '5000' }, '98.5'));
  assert.ok(Math.abs(Number(big.value) - Number(r.value)) < 1e-12);
});

test('simple yield: (coupon + (par - price)/years)/price, percent', () => {
  const years = S.yearsAct365(SETTLE, sec.maturity);
  const r = run(S.simpleYield(sec, SETTLE, '98.5'));
  assert.equal(r.unit, 'percent');
  const expect = ((4.375 + (100 - 98.5) / Number(years)) / 98.5) * 100;
  assert.ok(Math.abs(Number(r.value) - expect) < 1e-9, `${r.value} vs ${expect}`);
  // At par and par-priced redemption the simple yield equals the coupon.
  const par = run(S.simpleYield(sec, SETTLE, '100'));
  assert.ok(Math.abs(Number(par.value) - 4.375) < 1e-9);
});

test('invalid inputs are rejected by the engine', () => {
  const env = envelope(S.currentYield(sec, '0'));
  assert.equal(env.ok, 'false');
});
