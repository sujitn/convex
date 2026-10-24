// Credit default swaps against the real engine: a survival curve calibrated
// from par spreads over a swap or government discount curve, standard
// contracts priced on it, spread ↔ upfront conversion and signed risk.
import test from 'node:test';
import assert from 'node:assert/strict';
import { nodeEngine } from './engine-adapter.mjs';
import { SNAPSHOT } from '../../src/market/snapshot.js';
import { buildCurve, customCurveDef } from '../../src/analytics/curves.js';
import { parseTenorTable } from '../../src/lib/paste.js';
import { fromNumber } from '../../src/lib/decimal.js';
import * as C from '../../src/lib/cds.js';
import {
  discountForCds, discountHorizon, buildCredit, creditTable, priceContract, convertQuote, contractRisk, contractTerms,
} from '../../src/analytics/cds.js';

const ds = SNAPSHOT.datasets;
const TRADE = ds['ust-par'].asOf;
const SOFR = parseTenorTable('1W 3.92\n1M 3.90\n3M 3.85\n6M 3.75\n1Y 3.60\n2Y 3.45\n3Y 3.40\n5Y 3.42\n7Y 3.50\n10Y 3.62\n12Y 3.70\n15Y 3.78').points;

const usdCredit = C.creditCurveDef({ entity: 'Example Corp', currency: 'USD', points: C.exampleSpreads('USD'), example: true });

const memo = new Map();
function once(key, fn) {
  if (!memo.has(key)) memo.set(key, fn());
  return memo.get(key);
}

const ust = () => once('ust', () => buildCurve(nodeEngine, 'USD.UST', ds['ust-par']));
const sofr = () => once('sofr', () => {
  const def = customCurveDef({ id: 'USD.SOFR.OIS', label: 'SOFR OIS', currency: 'USD', method: 'ois', asOf: TRADE, points: SOFR, index: 'USD.SOFR' });
  return buildCurve(nodeEngine, def, def);
});

async function usdBuilt() {
  return once('usd', async () => {
    const curve = await sofr();
    assert.equal(curve.error, undefined, JSON.stringify(curve.error));
    const discount = await discountForCds(nodeEngine, curve, discountHorizon(usdCredit, TRADE), 'swap');
    assert.equal(discount.error, undefined, JSON.stringify(discount.error));
    return buildCredit(nodeEngine, usdCredit, { tradeDate: TRADE, discount });
  });
}

const fiveYear = (side = 'ProtectionBuyer', coupon = '100') => ({
  side, notional: '10000000', coupon, maturity: C.standardMaturity(TRADE, '5Y'),
});

test('standard dates: CDS2015 semi-annual roll, quarterly accrual start, T+1 step-in, T+3 settlement', () => {
  assert.equal(C.rollAnchor('2026-03-19'), '2025-12-20');
  assert.equal(C.rollAnchor('2026-03-20'), '2026-06-20');
  assert.equal(C.rollAnchor('2026-09-19'), '2026-06-20');
  assert.equal(C.rollAnchor('2026-10-20'), '2026-12-20');
  assert.equal(C.standardMaturity('2026-10-20', '5Y'), '2031-12-20');
  assert.equal(C.standardMaturity('2026-10-20', '6M'), '2027-06-20');
  assert.equal(C.standardMaturity('2022-06-21', '1Y'), '2023-06-20');
  assert.equal(C.rollOnOrBefore('2026-10-20'), '2026-09-20');
  assert.equal(C.rollOnOrBefore('2026-09-20'), '2026-09-20');
  assert.equal(C.rollOnOrBefore('2026-01-05'), '2025-12-20');
  assert.equal(C.tenorQuarters('1M'), null);
  assert.equal(C.tenorQuarters('18M'), 18);
  // 20 Sep 2026 is a Sunday: accrual starts on the Monday.
  const d = C.contractDates('NY+LON', '2026-10-20');
  assert.deepEqual([d.stepIn, d.accrualStart, d.protectionStart, d.cashSettlement],
    ['2026-10-21', '2026-09-21', '2026-10-21', '2026-10-23']);
  // Both centres must be open: 25 Aug 2025 is a London bank holiday.
  assert.equal(C.cdsCalendar('NY+LON').isBusinessDay('2025-08-25'), false);
  assert.equal(C.cdsCalendar('NY').isBusinessDay('2025-08-25'), true);
  assert.match(C.cdsCalendar('NY+LON').input.pin.id, /^CDS\.US\.FED_GB\.LON\.rules\./);
});

test('the survival curve calibrates from par spreads and reprices every quote', async () => {
  const built = await usdBuilt();
  assert.equal(built.error, undefined, JSON.stringify(built.error));
  assert.equal(built.repricing.length, C.CDS_TENORS.length);
  for (const r of built.repricing) {
    assert.equal(r.input.unit, 'BasisPoints');
    assert.ok(Math.abs(r.residual) <= r.tolerance, JSON.stringify(r));
  }
  assert.equal(built.recipe.curve.dependencies[0].id, built.discount.key.id);
  assert.equal(built.survival.source.Nodes.length, built.quotes.length + 1);
  assert.equal(built.grid.values[0], 1);
  // Survival falls with maturity; default probability and survival are complements.
  const { rows, error } = await creditTable(nodeEngine, built);
  assert.equal(error, undefined, JSON.stringify(error));
  rows.forEach((r, i) => {
    assert.ok(r.survival > 0 && r.survival < 1);
    if (i) assert.ok(r.survival < rows[i - 1].survival);
    assert.ok(Math.abs(r.survival + r.cumulative - 1) < 1e-12, `${r.tenor}`);
    assert.ok(r.hazard > 0 && r.interval > 0);
  });
});

test('a contract priced at its own par spread has zero clean value', async () => {
  const built = await usdBuilt();
  const first = await priceContract(nodeEngine, built, fiveYear());
  assert.equal(first.error, undefined, JSON.stringify(first.error));
  const par = first.price.par_spread_decimal;
  // The 5Y contract is the 5Y helper, so its par spread is the 5Y quote.
  assert.ok(Math.abs(par * 1e4 - 96) < 1e-6, `${par}`);
  const atPar = await priceContract(nodeEngine, built, fiveYear('ProtectionBuyer', fromNumber(par * 1e4, 8)));
  assert.equal(atPar.error, undefined, JSON.stringify(atPar.error));
  const p = atPar.price;
  assert.ok(Math.abs(p.clean_upfront_fraction) < 1e-9, `${p.clean_upfront_fraction}`);
  // What remains is the accrued premium refunded to the buyer at settlement,
  // discounted to the trade date in the PV.
  assert.ok(p.accrued_premium > 0);
  assert.ok(Math.abs(p.cash_settlement_amount + p.accrued_premium) < 1e-4, `${p.cash_settlement_amount} ${p.accrued_premium}`);
  assert.ok(Math.abs(p.net_pv + p.accrued_premium) < 1e-3 * p.accrued_premium, `${p.net_pv} ${p.accrued_premium}`);
  assert.ok(p.premiums.length > 15);
  assert.equal(p.premiums[p.premiums.length - 1].accrual_end, '2031-12-21');
  assert.match(p.model_notice, /ISDA CDS Standard Model/);
});

test('spread → upfront → spread conversion round trips', async () => {
  const built = await usdBuilt();
  for (const bp of ['50', '96', '350']) {
    const there = await convertQuote(nodeEngine, built, fiveYear(), { kind: 'spread', bp });
    assert.equal(there.error, undefined, JSON.stringify(there.error));
    const points = fromNumber(there.data.valuation.clean_upfront_fraction * 100, 12);
    const back = await convertQuote(nodeEngine, built, fiveYear(), { kind: 'upfront', points });
    assert.equal(back.error, undefined, JSON.stringify(back.error));
    assert.ok(Math.abs(back.data.conventional_spread_decimal * 1e4 - Number(bp)) < 1e-6, `${bp} → ${points} → ${back.data.conventional_spread_decimal}`);
  }
  // Below the coupon the buyer is paid upfront; above it the buyer pays.
  const low = await convertQuote(nodeEngine, built, fiveYear(), { kind: 'spread', bp: '50' });
  const high = await convertQuote(nodeEngine, built, fiveYear(), { kind: 'spread', bp: '350' });
  assert.ok(low.data.valuation.clean_upfront_fraction < 0 && high.data.valuation.clean_upfront_fraction > 0);
});

test('CS01 is positive for bought protection, opposite for sold, and the quote buckets add up', async () => {
  const built = await usdBuilt();
  const results = {};
  for (const side of ['ProtectionBuyer', 'ProtectionSeller']) {
    const contract = fiveYear(side);
    const terms = contractTerms(built, contract);
    const context = C.pricingContext(built.dates);
    results[side] = await contractRisk(nodeEngine, built, terms, context);
  }
  const buy = results.ProtectionBuyer;
  const sell = results.ProtectionSeller;
  assert.equal(buy.parallel.error, undefined, JSON.stringify(buy.parallel.error));
  assert.ok(buy.parallel.value > 0, `${buy.parallel.value}`);
  assert.ok(Math.abs(buy.parallel.value + sell.parallel.value) < 1e-6 * Math.abs(buy.parallel.value));
  // A 10mm 5Y position moves by roughly notional × risky annuity × 1bp.
  assert.ok(buy.parallel.value > 3000 && buy.parallel.value < 6000, `${buy.parallel.value}`);
  for (const b of buy.buckets) assert.equal(b.error, undefined, JSON.stringify(b.error));
  const five = buy.buckets.find((b) => b.tenor === '5Y');
  assert.ok(five.value > 0 && five.measure === 'Cs01Signed');
  // Quotes past the contract's maturity don't move it (piecewise, causal bootstrap).
  assert.equal(buy.buckets.find((b) => b.tenor === '10Y').value, 0);
  const sum = buy.buckets.reduce((a, b) => a + b.value, 0);
  assert.ok(Math.abs(sum - buy.parallel.value) < 1e-3 * Math.abs(buy.parallel.value), `${sum} vs ${buy.parallel.value}`);
  // With survival held, higher recovery makes protection worth less to its
  // buyer; with the quotes held the hazard refits and most of that goes away.
  assert.equal(buy.recovery.measure, 'RecoveryDerivative');
  assert.ok(buy.recoveryHeld.value < 0, JSON.stringify(buy.recoveryHeld));
  assert.ok(Math.abs(buy.recovery.value) < 0.1 * Math.abs(buy.recoveryHeld.value), JSON.stringify([buy.recovery, buy.recoveryHeld]));
  assert.equal(buy.rates.error, undefined, JSON.stringify(buy.rates.error));
});

test('a government curve can stand in for the swap curve (EUR AAA, subordinated, TARGET2 + London)', async () => {
  const aaa = await buildCurve(nodeEngine, 'EUR.AAA', ds['ecb-aaa']);
  assert.equal(aaa.error, undefined, JSON.stringify(aaa.error));
  const def = C.creditCurveDef({ entity: 'Example AG', currency: 'EUR', seniority: 'Subordinated', points: C.exampleSpreads('EUR') });
  assert.equal(def.recovery, '25');
  assert.equal(def.docClause, 'MM14');
  assert.equal(def.calendar, 'TARGET+LON');
  const tradeDate = ds['ecb-aaa'].asOf;
  const discount = await discountForCds(nodeEngine, aaa, discountHorizon(def, tradeDate), 'government');
  assert.equal(discount.error, undefined, JSON.stringify(discount.error));
  const built = await buildCredit(nodeEngine, def, { tradeDate, discount });
  assert.equal(built.error, undefined, JSON.stringify(built.error));
  assert.ok(built.repricing.every((r) => Math.abs(r.residual) <= r.tolerance));
  assert.equal(built.recipe.curve.role.seniority, 'subordinated');
  assert.equal(built.recipe.curve.role.recovery, 0.25);
  const priced = await priceContract(nodeEngine, built, { side: 'ProtectionSeller', notional: '5000000', coupon: '100',
    maturity: C.standardMaturity(tradeDate, '5Y') });
  assert.equal(priced.error, undefined, JSON.stringify(priced.error));
  assert.ok(priced.price.net_pv > 0, 'a seller of protection below the coupon is in the money');
});

test('the Treasury curve stands in for USD; inputs the engine cannot take are refused before it is called', async () => {
  const curve = await ust();
  const discount = await discountForCds(nodeEngine, curve, '2037-03-20', 'government');
  assert.equal(discount.error, undefined, JSON.stringify(discount.error));
  assert.equal(discount.recipe.role.kind, 'Discount');
  assert.equal(discount.source.kind, 'government');
  const onUst = await buildCredit(nodeEngine, usdCredit, { tradeDate: TRADE, discount });
  assert.equal(onUst.error, undefined, JSON.stringify(onUst.error));
  assert.ok(onUst.repricing.every((r) => Math.abs(r.residual) <= r.tolerance));
  const odd = { ...usdCredit, points: [...usdCredit.points, { tenor: '1M', value: '20' }] };
  const built = await buildCredit(nodeEngine, odd, { tradeDate: TRADE, discount });
  assert.equal(built.survival, null);
  assert.match(built.error.message, /whole quarters/);
  const early = await buildCredit(nodeEngine, usdCredit, { tradeDate: '2020-01-02', discount });
  assert.match(early.error.message, /before the discount curve/);
});

test('an engine failure comes back verbatim, never as a number', async () => {
  const built = await usdBuilt();
  // Beyond the last pillar the survival curve has no extrapolation.
  const res = await priceContract(nodeEngine, built, { ...fiveYear(), maturity: C.standardMaturity(TRADE, '15Y') });
  assert.equal(res.price, undefined);
  assert.ok(res.error && res.error.code && res.error.message, JSON.stringify(res.error));
});
