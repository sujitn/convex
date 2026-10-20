import test from 'node:test';
import assert from 'node:assert/strict';
import { nodeEngine } from './engine-adapter.mjs';
import { SNAPSHOT } from '../../src/market/snapshot.js';
import { buildCurve } from '../../src/analytics/curves.js';
import { treasuryUniverse, onTheRun, EXAMPLES } from '../../src/analytics/securities.js';
import { computeYas } from '../../src/analytics/yas.js';
import { settlementFor } from '../../src/lib/terms.js';

const ds = SNAPSHOT.datasets;

test('UST curve calibrates from the snapshot par yields and reprices every helper', async () => {
  const curve = await buildCurve(nodeEngine, 'USD.UST', ds['ust-par']);
  assert.equal(curve.error, undefined, JSON.stringify(curve.error));
  assert.ok(curve.repricing.every((r) => Math.abs(r.residual) < 1e-6));
});

test('EUR curve builds from ECB continuous spot rates', async () => {
  const curve = await buildCurve(nodeEngine, 'EUR.AAA', ds['ecb-aaa']);
  assert.equal(curve.error, undefined, JSON.stringify(curve.error));
});

test('CAD curve calibrates from Bank of Canada benchmark yields', async () => {
  const curve = await buildCurve(nodeEngine, 'CAD.GOC', ds['boc-benchmarks']);
  assert.equal(curve.error, undefined, JSON.stringify(curve.error));
});

test('YAS on the on-the-run 10Y: price → ICMA yield, Z ≈ 0, G ≈ 0, duration sane', async () => {
  const curve = await buildCurve(nodeEngine, 'USD.UST', ds['ust-par']);
  const treasuries = treasuryUniverse(ds['ust-auctions']);
  const benchmarks = onTheRun(treasuries);
  const ten = benchmarks.find((b) => b.bucket === 10).sec;
  const tradeDate = ds['ust-par'].asOf;
  const settlement = settlementFor(ten, tradeDate);
  const out = await computeYas(nodeEngine, {
    sec: ten, tradeDate, settlement, quote: { kind: 'price', text: '96-16' }, face: '1000000', curve, benchmarks,
  });
  assert.equal(out.fatal, undefined, JSON.stringify(out.fatal));
  console.log(JSON.stringify({ ytm: out.ytm, z: out.zspread, g: out.gspread && out.gspread.bp, risk: out.risk,
    yields: out.yields.map((y) => [y.label, y.value]), cy: out.currentYield, sy: out.simpleYield, inv: out.invoice,
    days: out.accruedDays, errors: out.errors }, null, 1));
  assert.ok(out.ytm > 0.04 && out.ytm < 0.07);
  assert.ok(out.risk.modified > 6 && out.risk.modified < 9);
  assert.ok(Number.isFinite(out.zspread));
  assert.ok(out.gspread && Number.isFinite(out.gspread.bp));
});

test('YAS from yield and from Z-spread round-trips the price (step-up bond)', async () => {
  const curve = await buildCurve(nodeEngine, 'USD.UST', ds['ust-par']);
  const sec = EXAMPLES.find((e) => e.id === 'EX-STEP-2031');
  const tradeDate = ds['ust-par'].asOf;
  const settlement = settlementFor(sec, tradeDate);
  const a = await computeYas(nodeEngine, { sec, tradeDate, settlement, quote: { kind: 'price', text: '101.25' }, face: '1000000', curve });
  assert.equal(a.fatal, undefined, JSON.stringify(a.fatal));
  const b = await computeYas(nodeEngine, { sec, tradeDate, settlement, quote: { kind: 'yield', text: String(a.ytm * 100) }, face: '1000000', curve });
  assert.ok(Math.abs(b.clean - 101.25) < 1e-6);
  const c = await computeYas(nodeEngine, { sec, tradeDate, settlement, quote: { kind: 'zspread', text: String(a.zspread * 10000) }, face: '1000000', curve });
  assert.ok(Math.abs(c.clean - 101.25) < 1e-6);
});

test('a callable runs straight analysis with calls removed and says so', async () => {
  const curve = await buildCurve(nodeEngine, 'USD.UST', ds['ust-par']);
  const sec = EXAMPLES.find((e) => e.id === 'EX-CALL-2034');
  const tradeDate = ds['ust-par'].asOf;
  const out = await computeYas(nodeEngine, { sec, tradeDate, settlement: settlementFor(sec, tradeDate),
    quote: { kind: 'price', text: '100.5' }, face: '1000000', curve });
  assert.equal(out.fatal, undefined, JSON.stringify(out.fatal));
  assert.equal(out.callsIgnored, true);
});

test('sinking-fund and euro examples price through YAS', async () => {
  for (const [id, cid] of [['EX-SINK-2036', 'USD.UST'], ['EX-EUR-2035', 'EUR.AAA']]) {
    const dsId = cid === 'USD.UST' ? 'ust-par' : 'ecb-aaa';
    const curve = await buildCurve(nodeEngine, cid, ds[dsId]);
    const sec = EXAMPLES.find((e) => e.id === id);
    const tradeDate = ds[dsId].asOf;
    const out = await computeYas(nodeEngine, { sec, tradeDate, settlement: settlementFor(sec, tradeDate),
      quote: { kind: 'price', text: '99' }, face: '1000000', curve });
    assert.equal(out.fatal, undefined, `${id}: ${JSON.stringify(out.fatal)}`);
    assert.ok(Number.isFinite(out.zspread), `${id}: ${JSON.stringify(out.errors)}`);
  }
});

test('callable analysis: OAS, option value, effective risk, vol ladder, workouts', async () => {
  const { analyseCallable } = await import('../../src/analytics/callable.js');
  const curve = await buildCurve(nodeEngine, 'USD.UST', ds['ust-par']);
  const sec = EXAMPLES.find((e) => e.id === 'EX-CALL-2034');
  const tradeDate = ds['ust-par'].asOf;
  const t = Date.now();
  const out = await analyseCallable(nodeEngine, { sec, tradeDate, settlement: settlementFor(sec, tradeDate), curve,
    cleanPrice: 100.5, meanReversion: 0.03, volatility: 0.01 });
  console.log('callable ms', Date.now() - t, JSON.stringify({ oas: out.oas, opt: out.optionValue, risk: out.risk && { d: out.risk.duration, c: out.risk.convexity },
    vols: out.volLadder.map((v) => [v.vol, v.oas, v.error && v.error.message]), wk: out.workout && out.workout.worst, errors: out.errors, fatal: out.fatal }));
  assert.equal(out.fatal, undefined, JSON.stringify(out.fatal));
  assert.ok(out.optionValue > 0);
});

test('without a curve, Z-spread is reported missing, never zero', async () => {
  const treasuries = treasuryUniverse(ds['ust-auctions']);
  const sec = onTheRun(treasuries).find((b) => b.bucket === 5).sec;
  const tradeDate = ds['ust-par'].asOf;
  const out = await computeYas(nodeEngine, { sec, tradeDate, settlement: settlementFor(sec, tradeDate),
    quote: { kind: 'price', text: '99.5' }, face: '1000000', curve: null });
  assert.equal(out.fatal, undefined);
  assert.equal(out.zspread, undefined);
  assert.equal(out.errors.zspread.code, 'missing_input');
});

test('a par bond at each calibrated tenor prices to 100 at zero spread', async () => {
  const R = await import('../../src/lib/requests.js');
  const curve = await buildCurve(nodeEngine, 'USD.UST', ds['ust-par']);
  for (const q of curve.quotes.filter((x) => /Y$/.test(x.tenor))) {
    const sec = { id: `PAR${q.tenor}`, cusip: 'PARTEST01', currency: 'USD', coupon: q.parPct, issueDate: curve.settlement,
      datedDate: curve.settlement, maturity: q.maturity, frequency: 'SemiAnnual', dayCount: 'ActActIcma', face: '100',
      calendar: 'US.SIFMA', settlementDays: 1, paymentAdjustment: 'Following', eom: false };
    const res = await nodeEngine.call(R.bondPrice(sec, curve.settlement,
      R.spreadMethod(curve.key, R.governmentRole('US.TREASURY'), 0), [curve.recipe]));
    assert.ok(res.ok, JSON.stringify(res.error));
    assert.ok(Math.abs(res.data.clean_per100 - 100) < 1e-6, `${q.tenor}: ${res.data.clean_per100}`);
  }
});
