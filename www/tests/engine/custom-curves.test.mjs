// Curves you supply: OIS swap rates calibrated natively, par yields and zero
// rates, and spreads quoted against them in the collateral role.
import test from 'node:test';
import assert from 'node:assert/strict';
import { nodeEngine } from './engine-adapter.mjs';
import { SNAPSHOT } from '../../src/market/snapshot.js';
import { buildCurve, customCurveDef } from '../../src/analytics/curves.js';
import { treasuryUniverse, onTheRun, EXAMPLES } from '../../src/analytics/securities.js';
import { computeYas, benchmarkQuotes } from '../../src/analytics/yas.js';
import { analyseCallable } from '../../src/analytics/callable.js';
import { parseTenorTable } from '../../src/lib/paste.js';
import { EXAMPLE_OIS, examplePoints } from '../../src/lib/ois.js';
import { settlementFor } from '../../src/lib/terms.js';
import * as R from '../../src/lib/requests.js';

const ds = SNAPSHOT.datasets;
const TRADE = ds['ust-par'].asOf;
const SOFR = parseTenorTable('1W 3.92\n1M 3.90\n3M 3.85\n6M 3.75\n1Y 3.60\n2Y 3.45\n3Y 3.40\n5Y 3.42\n7Y 3.50\n10Y 3.62\n12Y 3.70\n15Y 3.78').points;

const sofrDef = customCurveDef({ id: 'USD.SOFR.OIS', label: 'SOFR OIS', currency: 'USD', method: 'ois', asOf: TRADE,
  points: SOFR, index: 'USD.SOFR' });

let sofrCurve;
async function sofr() {
  if (!sofrCurve) sofrCurve = await buildCurve(nodeEngine, sofrDef, sofrDef);
  return sofrCurve;
}

test('SOFR OIS curve calibrates from par swap rates and reprices every swap', async () => {
  const c = await sofr();
  assert.equal(c.error, undefined, JSON.stringify(c.error));
  assert.equal(c.method, 'ois');
  assert.equal(c.repricing.length, SOFR.length);
  assert.ok(c.repricing.every((r) => Math.abs(r.residual) < 1e-9), JSON.stringify(c.repricing.map((r) => r.residual)));
  assert.equal(c.recipe.role.kind, 'Discount');
  assert.ok(c.publications > 3500 && c.publications < 4000, `${c.publications}`);
  // The re-supplied direct curve gives back the swap rate's discount factors. The
  // 1Y swap has one annual period from spot (2026-10-22) to 2027-10-22; with the
  // overnight rate projected off the same curve, its par rate r satisfies
  // DF(end) / DF(start) = 1 / (1 + r × days / 360). Both legs pay on the same
  // delayed date, so that discount factor cancels.
  const df = async (date) => {
    const res = await nodeEngine.call(R.curveQuery([c.recipe], c.key, R.dfQuery({ Date: date })));
    assert.ok(res.ok, JSON.stringify(res.error));
    return res.data.value;
  };
  const ratio = (await df('2027-10-22')) / (await df('2026-10-22'));
  assert.ok(Math.abs(ratio - 1 / (1 + 0.036 * (365 / 360))) < 1e-10, `${ratio}`);
});

test('an OIS tenor past the publication limit is refused before the engine is called', async () => {
  const def = customCurveDef({ id: 'USD.SOFR.LONG', label: 'SOFR long', currency: 'USD', method: 'ois', asOf: TRADE,
    points: [...SOFR, { tenor: '20Y', value: '3.85' }], index: 'USD.SOFR' });
  const c = await buildCurve(nodeEngine, def, def);
  assert.equal(c.recipe, null);
  assert.equal(c.error.code, 'input_limit');
  assert.match(c.error.message, /zero rates/);
});

test('€STR, SONIA and CORRA OIS curves calibrate on their own calendars', async () => {
  const sets = {
    'EUR.ESTR': '1M 1.92\n6M 1.88\n1Y 1.86\n2Y 1.90\n5Y 2.12\n10Y 2.42',
    'GBP.SONIA': '1M 3.68\n6M 3.55\n1Y 3.45\n2Y 3.40\n5Y 3.55\n10Y 3.85',
    'CAD.CORRA': '1M 2.25\n6M 2.22\n1Y 2.20\n2Y 2.30\n5Y 2.62\n10Y 3.00',
  };
  for (const [index, text] of Object.entries(sets)) {
    const currency = index.slice(0, 3);
    const def = customCurveDef({ id: `${currency}.OIS`, label: index, currency, method: 'ois', asOf: TRADE,
      points: parseTenorTable(text).points, index });
    const c = await buildCurve(nodeEngine, def, def);
    assert.equal(c.error, undefined, `${index}: ${JSON.stringify(c.error)}`);
    assert.ok(c.repricing.every((r) => Math.abs(r.residual) < 1e-9), index);
  }
});

test('YAS spreads a Treasury against the SOFR curve; G-spread stays on the Treasury curve', async () => {
  const gov = await buildCurve(nodeEngine, 'USD.UST', ds['ust-par']);
  const c = await sofr();
  const benchmarks = onTheRun(treasuryUniverse(ds['ust-auctions']));
  const ten = benchmarks.find((b) => b.bucket === 10).sec;
  const settlement = settlementFor(ten, TRADE);
  const base = { sec: ten, tradeDate: TRADE, settlement, quote: { kind: 'price', text: '96-16' }, face: '1000000', benchmarks };
  const onGov = await computeYas(nodeEngine, { ...base, curve: gov });
  const onSwaps = await computeYas(nodeEngine, { ...base, curve: c, benchmarkCurve: gov });
  assert.equal(onSwaps.fatal, undefined, JSON.stringify(onSwaps.fatal));
  assert.equal(onSwaps.errors.zspread, undefined, JSON.stringify(onSwaps.errors.zspread));
  assert.ok(Number.isFinite(onSwaps.zspread));
  assert.notEqual(onSwaps.zspread.toFixed(6), onGov.zspread.toFixed(6));
  // Same price and benchmarks, so the same G-spread whichever curve Z is quoted on.
  assert.ok(Math.abs(onSwaps.gspread.bp - onGov.gspread.bp) < 1e-9);
  // A Z quote over swaps prices back to the same clean price.
  const back = await computeYas(nodeEngine, { ...base, curve: c, benchmarkCurve: gov,
    quote: { kind: 'zspread', text: String(onSwaps.zspread * 10000) } });
  assert.ok(Math.abs(back.clean - onSwaps.clean) < 1e-8, `${back.clean} vs ${onSwaps.clean}`);
});

test('benchmark prices you enter replace the curve-implied ones', async () => {
  const gov = await buildCurve(nodeEngine, 'USD.UST', ds['ust-par']);
  const benchmarks = onTheRun(treasuryUniverse(ds['ust-auctions']));
  const two = benchmarks.find((b) => b.bucket === 2).sec;
  const settlement = settlementFor(two, TRADE);
  const out = await benchmarkQuotes(nodeEngine, benchmarks, settlement, gov, { [two.cusip]: '99-16+' });
  const row = out.list.find((b) => b.sec.cusip === two.cusip);
  assert.equal(row.provenance, 'manual');
  assert.equal(Number(row.quote.value), 99 + 16.5 / 32);
  assert.ok(out.list.filter((b) => b.provenance === 'curve-implied').length === benchmarks.length - 1);
});

test('custom par and zero curves build and price in their roles', async () => {
  const par = customCurveDef({ id: 'GBP.GILT', label: 'Gilt par', currency: 'GBP', method: 'par', asOf: TRADE,
    points: parseTenorTable('1Y 3.80\n2Y 3.75\n5Y 3.90\n10Y 4.40\n30Y 5.10').points });
  const pc = await buildCurve(nodeEngine, par, par);
  assert.equal(pc.error, undefined, JSON.stringify(pc.error));
  assert.equal(pc.recipe.role.kind, 'Government');
  assert.equal(pc.spreadRole.kind, 'government');

  const zero = customCurveDef({ id: 'USD.SWAP.ZERO', label: 'Swap zeros', currency: 'USD', method: 'zero', asOf: TRADE,
    points: parseTenorTable('1Y 3.6\n5Y 3.4\n10Y 3.6\n20Y 3.8\n30Y 3.7').points, role: 'discount' });
  const zc = await buildCurve(nodeEngine, zero, zero);
  assert.equal(zc.error, undefined, JSON.stringify(zc.error));
  const ex = EXAMPLES.find((e) => e.currency === 'USD' && !(e.calls || []).length);
  const settlement = settlementFor(ex, TRADE);
  const res = await nodeEngine.call(R.bondPrice(ex, settlement, R.spreadMethod(zc.key, zc.spreadRole, 0.01), [zc.recipe]));
  assert.ok(res.ok, JSON.stringify(res.error));
});

test('OAS of the example callable over the example SOFR curve', async () => {
  const def = customCurveDef({ id: 'USD.SOFR.EX', label: 'SOFR example', currency: 'USD', method: 'ois', asOf: TRADE,
    points: examplePoints('USD.SOFR'), index: 'USD.SOFR' });
  const c = await buildCurve(nodeEngine, def, def);
  const callable = EXAMPLES.find((e) => (e.calls || []).length > 0 && e.currency === 'USD');
  const settlement = settlementFor(callable, TRADE);
  // The OAS screen's opening inputs: clean 97.00, σ 1.00%, a 3.0%.
  const out = await analyseCallable(nodeEngine, { sec: callable, tradeDate: TRADE, settlement, curve: c, cleanPrice: 97,
    meanReversion: 0.03, volatility: 0.01 });
  assert.equal(out.fatal, undefined, JSON.stringify(out.fatal));
  assert.ok(Number.isFinite(out.oas));
  assert.ok(out.risk, JSON.stringify(out.errors.risk));
});

test('every example OIS set calibrates', async () => {
  for (const index of Object.keys(EXAMPLE_OIS)) {
    const currency = index.slice(0, 3);
    const def = customCurveDef({ id: `${currency}.EX`, label: index, currency, method: 'ois', asOf: TRADE,
      points: examplePoints(index), index });
    const c = await buildCurve(nodeEngine, def, def);
    assert.equal(c.error, undefined, `${index}: ${JSON.stringify(c.error)}`);
  }
});
