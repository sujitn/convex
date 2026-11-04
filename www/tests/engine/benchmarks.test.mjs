// G-spread benchmark choices: the on-the-run set, bonds you pick, the
// same-maturity rule, and benchmarks in another currency at imported prices.
import test from 'node:test';
import assert from 'node:assert/strict';
import { nodeEngine } from './engine-adapter.mjs';
import { SNAPSHOT } from '../../src/market/snapshot.js';
import { buildCurve } from '../../src/analytics/curves.js';
import { treasuryUniverse, onTheRun } from '../../src/analytics/securities.js';
import { computeYas, gspreadBenchmarks, gspreadAvailable } from '../../src/analytics/yas.js';
import { parseBondTable } from '../../src/lib/bondtable.js';
import { settlementFor } from '../../src/lib/terms.js';
import { yearsBetween, addMonths } from '../../src/lib/dates.js';
import { governmentSpread } from '../../src/lib/cash.js';

const ds = SNAPSHOT.datasets;
const TRADE = ds['ust-par'].asOf;
const treasuries = treasuryUniverse(ds['ust-auctions']);
const otr = onTheRun(treasuries);
const asBench = (secs) => secs.map((sec) => ({ bucket: Math.round(yearsBetween(TRADE, sec.maturity) * 10) / 10, sec }));

const months = (a, b) => (+b.slice(0, 4) - +a.slice(0, 4)) * 12 + (+b.slice(5, 7) - +a.slice(5, 7));

let gov;
async function curve() {
  if (!gov) gov = await buildCurve(nodeEngine, 'USD.UST', ds['ust-par']);
  return gov;
}

async function yas(sec, benchmarks, extra = {}) {
  return computeYas(nodeEngine, { sec, tradeDate: TRADE, settlement: settlementFor(sec, TRADE), quote: { kind: 'price', text: '97' },
    face: '1000000', curve: await curve(), benchmarks, ...extra });
}

test('a corporate maturing on a Treasury\'s date: same-maturity G-spread equals the yield difference to that bond', async () => {
  const ten = otr.find((b) => b.bucket === 10).sec;
  const { rows } = parseBondTable(`Name\tCoupon\tMaturity\tIssue Date\nCorp\t5.5\t${ten.maturity}\t2026-08-15`, { tradeDate: TRADE });
  const corp = rows[0].sec;
  const exact = await yas(corp, asBench([ten]), { benchmarkSelection: 'ExactMaturity', benchPrices: { [ten.cusip]: '96-16' } });
  assert.equal(exact.errors.gspread, undefined, JSON.stringify(exact.errors.gspread));
  assert.equal(exact.gspread.selected.lower_weight, 1);
  assert.equal(exact.gspread.selected.upper_index, null);
  assert.ok(Math.abs(exact.gspread.bp - (exact.gspread.targetYield - exact.gspread.benchYield) * 10000) < 1e-9);
  // The library takes exactly one security for ExactMaturity; handed the whole on-the-run set it refuses ...
  const all = await yas(corp, otr, { benchmarkSelection: 'ExactMaturity', benchPrices: { [ten.cusip]: '96-16' } });
  assert.equal(all.errors.gspread, undefined, JSON.stringify(all.errors.gspread));
  assert.equal(all.gspread.benchmarks.length, 1, 'only the same-maturity bond is sent');
  assert.equal(all.gspread.bp, exact.gspread.bp);
  // ... so the analysis sends only the one maturing the same day.
  assert.deepEqual(gspreadBenchmarks(corp, otr, 'ExactMaturity').map((b) => b.sec.id), [ten.id]);
  const quoted = otr.map((b) => ({ ...b, quote: { kind: 'CleanPrice', value: '100' } }));
  const raw = await nodeEngine.call(governmentSpread(corp, { kind: 'CleanPrice', value: '97' }, quoted,
    { tradeDate: TRADE, settlement: settlementFor(corp, TRADE), selection: 'ExactMaturity' }));
  assert.equal(raw.ok, false);
  assert.match(raw.error.message, /exact selection requires one supplied security/);
  // Same-maturity with no same-day bond is not asked of the engine at all.
  const five = otr.find((b) => b.bucket === 5).sec;
  const refused = await yas(corp, asBench([five]), { benchmarkSelection: 'ExactMaturity' });
  assert.equal(refused.errors.gspread.code, 'not_applicable', JSON.stringify(refused.errors.gspread));
  assert.equal(gspreadAvailable(corp, asBench([five]), 'ExactMaturity'), false);
  assert.equal(gspreadAvailable(corp, otr, 'ExactMaturity'), true);
});

test('interpolation: a target strictly between two benchmarks weights both', async () => {
  const seven = otr.find((b) => b.bucket === 7).sec;
  const ten = otr.find((b) => b.bucket === 10).sec;
  const mid = addMonths(seven.maturity, Math.round(months(seven.maturity, ten.maturity) / 2));
  assert.ok(seven.maturity < mid && mid < ten.maturity);
  const { rows } = parseBondTable(`Name	Coupon	Maturity	Issue Date
Corp	5.5	${mid}	2026-08-15`, { tradeDate: TRADE });
  const corp = rows[0].sec;
  assert.ok(!otr.some((b) => b.sec.maturity === corp.maturity));
  const out = await yas(corp, asBench([seven, ten]));
  assert.equal(out.errors.gspread, undefined, JSON.stringify(out.errors.gspread));
  const sel = out.gspread.selected;
  assert.notEqual(sel.upper_index, null);
  assert.ok(sel.lower_weight > 0 && sel.upper_weight > 0);
  assert.ok(Math.abs(sel.lower_weight + sel.upper_weight - 1) < 1e-12);
  // The whole on-the-run set brackets it too, and gives a finite spread.
  assert.ok(Number.isFinite((await yas(corp, otr)).gspread.bp));
  assert.equal(gspreadAvailable(corp, asBench([seven, ten]), 'LinearMaturity'), true);
  assert.equal(gspreadAvailable(corp, asBench([seven]), 'LinearMaturity'), false);
  assert.equal(gspreadAvailable(corp, asBench([otr[0].sec, seven]), 'LinearMaturity'), false, 'target beyond the longest benchmark');
});

test('the security itself and sinking-fund bonds are never its benchmarks', () => {
  const ten = otr.find((b) => b.bucket === 10);
  const seven = otr.find((b) => b.bucket === 7);
  assert.deepEqual(gspreadBenchmarks(ten.sec, [seven, ten], 'LinearMaturity').map((b) => b.sec.id), [seven.sec.id]);
  const sinking = { ...seven, sec: { ...seven.sec, sinking: [{ date: '2030-01-15', amount: '10' }] } };
  assert.deepEqual(gspreadBenchmarks(ten.sec, [sinking, ten], 'LinearMaturity'), []);
  assert.equal(gspreadAvailable(ten.sec, otr, 'ExactMaturity'), false, 'its own maturity does not count');
});

test('a chosen Treasury set: off-the-run bonds bracketing the target', async () => {
  const ten = otr.find((b) => b.bucket === 10).sec;
  const fixed = treasuries.filter((s) => s.kind === 'fixed' && s.maturity > '2030-01-01' && s.maturity < '2040-01-01');
  assert.ok(fixed.length >= 2);
  const chosen = asBench([fixed[0], fixed[fixed.length - 1]]);
  const target = fixed.find((x) => x.maturity > fixed[0].maturity && x.maturity < fixed[fixed.length - 1].maturity);
  assert.ok(target && target.id !== ten.id);
  const out = await yas(target, chosen);
  assert.equal(out.errors.gspread, undefined, JSON.stringify(out.errors.gspread));
  assert.equal(out.gspread.benchmarks.length, 2);
  assert.ok(out.gspread.selected.lower_weight + out.gspread.selected.upper_weight > 0.999999);
});

test('EUR benchmarks you imported, at their imported prices', async () => {
  const text = ['ISIN\tName\tCoupon\tMaturity\tIssue Date\tCurrency\tPrice',
    '\tBund 2030\t2.4\t2030-10-18\t2025-10-18\tEUR\t99.10',
    '\tBund 2036\t2.6\t2036-02-15\t2026-02-15\tEUR\t97.80',
    '\tTarget corp\t3.5\t2033-03-15\t2023-03-15\tEUR\t98.00'].join('\n');
  const { rows, errors } = parseBondTable(text, { tradeDate: TRADE });
  assert.equal(errors.length, 0, JSON.stringify(errors));
  const [b1, b2, target] = rows.map((r) => r.sec);
  const out = await computeYas(nodeEngine, { sec: target, tradeDate: TRADE, settlement: settlementFor(target, TRADE),
    quote: { kind: 'price', text: target.price }, face: '1000000', curve: null, benchmarks: asBench([b1, b2]), benchmarkCurve: null });
  assert.equal(out.errors.gspread, undefined, JSON.stringify(out.errors.gspread));
  assert.ok(out.gspread.benchmarks.every((b) => b.provenance === 'imported'));
  assert.ok(Number.isFinite(out.gspread.bp));
});
