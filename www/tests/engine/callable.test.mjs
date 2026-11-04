import test from 'node:test';
import assert from 'node:assert/strict';
import { run, envelope } from './harness.mjs';
import * as R from '../../src/lib/requests.js';
import * as C from '../../src/lib/callable.js';
import { addBusinessDays } from '../../src/lib/calendars.js';

const TRADE = '2026-10-20';
const SETTLE = addBusinessDays('US.SIFMA', TRADE, 1);

// US corporate, 5.00% semi-annual 30/360, 10Y, callable from 2029-11-15 at 102 / 2030 at 101 / 2031 at 100.
const corp = (calls = []) => ({
  id: 'CORP', cusip: 'CORP00001', currency: 'USD', coupon: '5.00', issueDate: '2025-11-15', datedDate: '2025-11-15',
  maturity: '2035-11-15', frequency: 'SemiAnnual', dayCount: 'Thirty360US', face: '100', calendar: 'US.SIFMA',
  settlementDays: 1, paymentAdjustment: 'Following', eom: false, calls,
});
const CALLS = [
  { date: '2029-11-15', price: '102' },
  { date: '2030-11-15', price: '101' },
  { date: '2031-11-15', price: '100' },
];

// Upward-sloping continuous zero curve (4.0% -> 5.0%).
const key = R.curveKey('USD.DISCOUNT.TEST', TRADE);
const curve = R.directZeroCurve({
  key, currency: 'USD', referenceDate: TRADE,
  role: { kind: 'Discount', collateral_currency: 'USD' },
  nodes: [[0.0, 0.04], [1, 0.041], [2, 0.0425], [5, 0.0455], [10, 0.048], [30, 0.05]],
  interpolation: 'Linear',
});
const model = C.hw1fAssumedModel({ meanReversion: 0.03, volatility: 0.01 });
const base = { discountCurve: curve, model };

test('exercise terms: one provision per price, European when single date', () => {
  const ex = R.bondConstruct(corp(CALLS), SETTLE).operation.bond.terms.exercise;
  assert.equal(ex.length, 3);
  assert.deepEqual(ex.map((e) => e.style.style), ['european', 'european', 'european']);
});

test('callable price <= bullet price on the same curve; OAS round trip; risk', () => {
  const call = run(C.optionalPrice(corp(CALLS), SETTLE, base));
  // The plain bond route on the same curve (zero spread) is the no-option reference.
  const bullet = run(R.bondPrice(corp([]), SETTLE, R.spreadMethod(key, R.collateralRole(), 0), [curve]));
  console.log('callable clean', call.clean_price, 'bullet clean', bullet.clean_per100, 'accrued', call.accrued);
  assert.ok(call.clean_price <= bullet.clean_per100 + 1e-6);
  assert.ok(call.clean_price < bullet.clean_per100 - 0.1, 'the call must have visible value');
  // The call price caps the callable value near the call price after the call date.
  const oas = 0.0035;
  const p = run(C.optionalPrice(corp(CALLS), SETTLE, { ...base, oas }));
  assert.ok(p.dirty_price < call.dirty_price);
  const solved = run(C.optionalOas(corp(CALLS), SETTLE, { ...base, dirtyPrice: p.dirty_price }));
  console.log('oas bps', solved.oas_bps, 'iterations', solved.iterations);
  assert.ok(Math.abs(solved.oas_decimal - oas) < 2e-4, `OAS ${solved.oas_decimal}`);

  const risk = run(C.optionalRisk(corp(CALLS), SETTLE, { ...base, oas }));
  console.log('eff dur', risk.duration, 'convexity', risk.convexity);
  assert.ok(risk.duration > 0 && risk.duration < 8, 'a 10y callable has effective duration below the bullet 8y+');

  const up = run(C.optionalScenario(corp(CALLS), SETTLE, { ...base, oas, discountShift: 0.01 }));
  assert.ok(up.dirty_price < p.dirty_price);
});

test('american exercise is reported unsupported by the engine', () => {
  const req = C.optionalPrice(corp(CALLS), SETTLE, base);
  req.operation.input.bond.terms.exercise[0].style = { style: 'american', start: '2029-11-15', end: '2030-11-14' };
  const env = envelope(req);
  assert.equal(env.ok, 'false');
  assert.equal(env.error.code, 'unsupported');
});

test('workout: yield to each call and maturity; worst is the minimum yield', () => {
  const sec = corp(CALLS);
  const convention = { compounding: 'SemiAnnual', day_count: 'Thirty360US' };
  // Premium price: every call is cheaper than maturity; the latest call wins (lowest yield).
  const premium = C.workoutRows(run(C.workoutPaths(sec, TRADE, SETTLE, { price: 103, convention, earliestNotice: SETTLE })));
  assert.equal(premium.paths.length, 4);
  assert.deepEqual(premium.paths.map((p) => p.kind), ['maturity', 'call', 'call', 'call']);
  const min = Math.min(...premium.paths.map((p) => p.decimalYield));
  assert.equal(premium.worst.decimalYield, min);
  assert.equal(premium.worst.date, '2031-11-15');
  for (const p of premium.paths) assert.ok(Math.abs(p.cleanPer100 - 103) < 1e-9);
  // Discount price: calling is the issuer's loss; the earliest call yields the most, maturity the least.
  const disc = C.workoutRows(run(C.workoutPaths(sec, TRADE, SETTLE, { price: 98, convention, earliestNotice: SETTLE })));
  assert.equal(disc.worst.kind, 'maturity');
  assert.ok(disc.paths[1].decimalYield > disc.paths[0].decimalYield);
  // Price at the solved yield returns the quote (same cash, same convention).
  const back = run(C.workoutPrice(sec, TRADE, SETTLE, { decimalYield: premium.worst.decimalYield, convention,
    selection: { kind: 'exercise', provision: 2, date: '2031-11-15' }, earliestNotice: SETTLE }));
  assert.ok(Math.abs(back.quotation.quotation.value.clean_per100 - 103) < 1e-6);
});

test('workout: an issued notice forces its path', () => {
  const sec = corp(CALLS);
  const convention = { compounding: 'SemiAnnual', day_count: 'Thirty360US' };
  const notified = { provision: 1, exercise: '2030-11-15', notice_date: '2026-10-19' };
  const rows = C.workoutRows(run(C.workoutPaths(sec, TRADE, SETTLE, { price: 103, convention, notified })));
  assert.equal(rows.forcedByNotice, true);
  assert.equal(rows.worst.date, '2030-11-15');
});

test('same-price call dates form one Bermudan provision', () => {
  const sec = corp([{ date: '2029-11-15', price: '101' }, { date: '2030-05-15', price: '101' }, { date: '2030-11-15', price: '101' }]);
  const ex = R.bondConstruct(sec, SETTLE).operation.bond.terms.exercise;
  assert.equal(ex.length, 1);
  assert.equal(ex[0].style.style, 'bermudan');
  const call = run(C.optionalPrice(sec, SETTLE, base));
  const bullet = run(R.bondPrice(corp([]), SETTLE, R.spreadMethod(key, R.collateralRole(), 0), [curve]));
  assert.ok(call.clean_price < bullet.clean_per100);
  // The engine echoes one entry per Bermudan exercise date it considered.
  assert.deepEqual(call.exercises.map((e) => e.dates.exercise), ['2029-11-15', '2030-05-15', '2030-11-15']);
});
