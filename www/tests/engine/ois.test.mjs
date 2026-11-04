// OIS helper dates against the engine's own swap schedule.
import test from 'node:test';
import assert from 'node:assert/strict';
import { run } from './harness.mjs';
import { nodeEngine } from './engine-adapter.mjs';
import { buildCurve, customCurveDef } from '../../src/analytics/curves.js';
import { oisCurveCalibration, examplePoints, OIS_INDICES } from '../../src/lib/ois.js';
import { isBusinessDay } from '../../src/lib/calendars.js';
import * as R from '../../src/lib/requests.js';

// Each helper's pillar must be exactly its swap's final payment date as the
// engine generates it (delay counted from the unadjusted end, then adjusted).
test('every OIS pillar is the engine\'s final payment date, weekend maturities included', () => {
  let weekendEnds = 0;
  for (const index of Object.keys(OIS_INDICES)) {
    const def = { ...OIS_INDICES[index], index };
    for (const tradeDate of ['2026-10-20', '2026-10-22', '2026-10-23']) {
      const req = oisCurveCalibration({ key: R.curveKey('T', '1'), projectionKey: R.curveKey('T.ON', '1'), def, tradeDate,
        quotes: examplePoints(index) });
      for (const helper of req.operation.input.rates[0].recipe.helpers) {
        const { kind, ...terms } = helper.terms;
        const built = run({ schema_version: 1, inputs: req.inputs,
          operation: { kind: 'swap', terms, context: req.operation.input.rate_context.cashflows, action: { kind: 'construct' } } });
        const payments = [...built.fixed_periods, ...built.floating_periods].map((p) => p.payment_date).sort();
        assert.equal(helper.pillar, payments.at(-1), `${index} ${tradeDate} ${helper.id}`);
        if (!isBusinessDay(def.calendar, terms.maturity_date)) weekendEnds += 1;
      }
    }
  }
  assert.ok(weekendEnds > 10, `${weekendEnds} maturities on non-business days were checked`);
});

test('SONIA (spot lag 0) on a Saturday trade date starts Monday and calibrates', async () => {
  const def = customCurveDef({ id: 'GBP.SAT', label: 'SONIA Saturday', currency: 'GBP', method: 'ois', asOf: '2026-10-17',
    points: examplePoints('GBP.SONIA'), index: 'GBP.SONIA' });
  const c = await buildCurve(nodeEngine, def, def);
  assert.equal(c.error, undefined, JSON.stringify(c.error));
  assert.ok(c.repricing.every((r) => Math.abs(r.residual) < 1e-9));
});

test('the publications supplied start at the first observation the swaps need', async () => {
  const def = { ...OIS_INDICES['USD.SOFR'], index: 'USD.SOFR' };
  const req = oisCurveCalibration({ key: R.curveKey('U', '1'), projectionKey: R.curveKey('U.ON', '1'), def,
    tradeDate: '2026-10-20', quotes: [{ tenor: '1M', value: '3.9' }, { tenor: '1Y', value: '3.7' }] });
  const data = run(req);
  assert.equal(req.inputs.publications[0].observation_date, '2026-10-22');
  assert.equal(data.publications[0].observation_date, '2026-10-22');
});
