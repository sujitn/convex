// Bonds pasted from a spreadsheet are complete contracts the engine prices.
import test from 'node:test';
import assert from 'node:assert/strict';
import { nodeEngine } from './engine-adapter.mjs';
import { SNAPSHOT } from '../../src/market/snapshot.js';
import { buildCurve } from '../../src/analytics/curves.js';
import { computeYas } from '../../src/analytics/yas.js';
import { parseBondTable } from '../../src/lib/bondtable.js';
import { settlementFor } from '../../src/lib/terms.js';
import { parsePrice } from '../../src/lib/decimal.js';

const ds = SNAPSHOT.datasets;
const TRADE = ds['ust-par'].asOf;

test('pasted bonds, with and without issue dates, price through YAS at their pasted price', async () => {
  const text = [
    'CUSIP\tName\tCoupon\tMaturity\tIssue Date\tDay Count\tPrice\tCalls',
    '91282CRF0\tUS Treasury Note\t4.625\t08/15/2036\t08/15/2026\tACT/ACT\t96-16+\t',
    '037833DX5\tApple 2.2 2029\t2.2\t09/11/2029\t\t30/360\t93.25\t',
    '\tCallable corp\t5.25\t03/15/2034\t03/15/2024\t30/360\t97\t03/15/2029@101; 03/15/2030@100',
  ].join('\n');
  const { rows, errors } = parseBondTable(text, { tradeDate: TRADE });
  assert.equal(errors.length, 0, JSON.stringify(errors));
  assert.equal(rows.length, 3);
  const curve = await buildCurve(nodeEngine, 'USD.UST', ds['ust-par']);
  for (const { sec } of rows) {
    const settlement = settlementFor(sec, TRADE);
    const out = await computeYas(nodeEngine, { sec, tradeDate: TRADE, settlement,
      quote: { kind: 'price', text: sec.price }, face: '1000000', curve });
    assert.equal(out.fatal, undefined, `${sec.name}: ${JSON.stringify(out.fatal)}`);
    assert.ok(out.ytm > 0 && out.ytm < 0.2, `${sec.name}: ${out.ytm}`);
    assert.ok(Number.isFinite(out.zspread), `${sec.name}: ${JSON.stringify(out.errors.zspread)}`);
    // The solved yield prices back to the pasted price.
    const back = await computeYas(nodeEngine, { sec, tradeDate: TRADE, settlement,
      quote: { kind: 'yield', text: String(out.ytm * 100) }, face: '1000000', curve });
    assert.ok(Math.abs(back.clean - parsePrice(sec.price)) < 1e-6, `${sec.name}: ${back.clean} vs ${sec.price}`);
  }
});
