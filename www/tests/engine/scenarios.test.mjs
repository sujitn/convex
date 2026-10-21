// Scenario recalibrations repeat the base curve's build: same quoted points,
// same interpolation, only the shift differs.
import test from 'node:test';
import assert from 'node:assert/strict';
import { nodeEngine } from './engine-adapter.mjs';
import { SNAPSHOT } from '../../src/market/snapshot.js';
import { buildCurve } from '../../src/analytics/curves.js';
import { repriceUnderShifts, keyRateVariants } from '../../src/analytics/scenarios.js';
import { EXAMPLES } from '../../src/analytics/securities.js';
import { settlementFor } from '../../src/lib/terms.js';
import * as R from '../../src/lib/requests.js';

const base = SNAPSHOT.datasets['ust-par'];
// The 4M quote is missing, as a source can leave a tenor blank.
const dataset = { ...base, points: base.points.map((p) => (p.tenor === '4M' ? { ...p, value: '' } : p)) };

function recording() {
  const seen = [];
  return {
    seen,
    call: nodeEngine.call,
    async callMany(reqs, opts) {
      seen.push({ label: opts && opts.label, reqs });
      return nodeEngine.callMany(reqs);
    },
  };
}

test('a zero shift reprices to the base price; missing tenors stay out', async () => {
  const curve = await buildCurve(nodeEngine, 'USD.UST', dataset);
  assert.equal(curve.error, undefined, JSON.stringify(curve.error));
  const sec = EXAMPLES.find((e) => e.id === 'EX-STEP-2031');
  const settlement = settlementFor(sec, curve.tradeDate);
  const z = 0.004;
  const at = await nodeEngine.call(R.bondPrice(sec, settlement,
    R.spreadMethod(curve.key, R.governmentRole('US.TREASURY'), z), [curve.recipe]));
  const engine = recording();
  const [row] = await repriceUnderShifts(engine, { sec, settlement, curve, dataset, zDecimal: z, label: 'test',
    variants: [{ id: 'zero', label: 'No shift', shift: () => 0 }] });
  assert.equal(row.error, null, JSON.stringify(row.error));
  const recipe = engine.seen[0].reqs[0].operation.input.cash[0].recipe;
  assert.equal(recipe.curve.interpolation, curve.interpolation);
  assert.ok(!recipe.helpers.some((h) => h.input.id.endsWith('.4M')));
  assert.equal(recipe.helpers.length, dataset.points.length - 1);
  assert.ok(Math.abs(row.dirty - at.data.dirty_per100) < 1e-9, `${row.dirty} vs ${at.data.dirty_per100}`);

  // The recalibration asks for the base curve's own interpolation, whatever it is
  // (the native par route admits Linear only, so this one is refused, not repriced on Linear).
  const spline = recording();
  const [refused] = await repriceUnderShifts(spline, { sec, settlement, curve: { ...curve, interpolation: 'CubicSpline' },
    dataset, zDecimal: z, label: 'test', variants: [{ id: 'zero', label: 'No shift', shift: () => 0 }] });
  assert.equal(spline.seen[0].reqs[0].operation.input.cash[0].recipe.curve.interpolation, 'CubicSpline');
  assert.ok(Number.isNaN(refused.dirty));
  assert.equal(refused.error.code, 'invalid_input');
});

test('key rates bump only quoted tenors', () => {
  const tenors = keyRateVariants(dataset).map((v) => v.tenor);
  assert.equal(tenors.length, dataset.points.length - 1);
  assert.ok(!tenors.includes('4M'));
});
