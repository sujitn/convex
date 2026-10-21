// G-spread benchmark choices: the on-the-run set, bonds you pick, the
// same-maturity rule, and benchmarks in another currency at imported prices.
import test from 'node:test';
import assert from 'node:assert/strict';
import { nodeEngine } from './engine-adapter.mjs';
import { SNAPSHOT } from '../../src/market/snapshot.js';
import { buildCurve } from '../../src/analytics/curves.js';
import { treasuryUniverse, onTheRun } from '../../src/analytics/securities.js';
import { computeYas } from '../../src/analytics/yas.js';
import { parseBondTable } from '../../src/lib/bondtable.js';
import { settlementFor } from '../../src/lib/terms.js';
import { yearsBetween } from '../../src/lib/dates.js';

const ds = SNAPSHOT.datasets;
const TRADE = ds['ust-par'].asOf;
const treasuries = treasuryUniverse(ds['ust-auctions']);
const otr = onTheRun(treasuries);
const asBench = (secs) => secs.map((sec) => ({ bucket: Math.round(yearsBetween(TRADE, sec.maturity) * 10) / 10, sec }));

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
  // Interpolated over the on-the-run set gives a different benchmark yield in general.
  const linear = await yas(corp, otr);
  assert.ok(Number.isFinite(linear.gspread.bp));
  // Same-maturity with no same-day bond is refused by the engine, not approximated.
  const five = otr.find((b) => b.bucket === 5).sec;
  const refused = await yas(corp, asBench([five]), { benchmarkSelection: 'ExactMaturity' });
  assert.ok(refused.errors.gspread && refused.errors.gspread.code !== 'not_applicable', JSON.stringify(refused.errors.gspread));
});

test('a chosen Treasury set: off-the-run bonds bracketing the target', async () => {
  const ten = otr.find((b) => b.bucket === 10).sec;
  const fixed = treasuries.filter((s) => s.kind === 'fixed' && s.maturity > '2030-01-01' && s.maturity < '2040-01-01');
  assert.ok(fixed.length >= 2);
  const chosen = asBench([fixed[0], fixed[fixed.length - 1]]);
  const out = await yas(ten, chosen);
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
