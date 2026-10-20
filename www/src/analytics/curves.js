// Curve construction from market datasets, through the engine. Par yield
// sets (Treasury, Canada) are calibrated natively from par-coupon helpers;
// published zero curves (ECB) are supplied directly as continuous zero nodes.
// The calibrated node grid is then re-supplied as a Direct curve with the
// same representation and interpolation, so later requests reuse it exactly.

import * as R from '../lib/requests.js';
import { addTenor, tenorYears } from '../lib/dates.js';
import { addBusinessDays } from '../lib/calendars.js';
import { percentToRate } from '../lib/decimal.js';

export const CURVE_DEFS = {
  'USD.UST': {
    label: 'US Treasury',
    currency: 'USD',
    dataset: 'ust-par',
    method: 'par',
    calendar: 'US.SIFMA',
    settlementDays: 1,
    frequency: 'SemiAnnual',
    dayCount: 'ActActIcma',
    benchmarkId: 'US.TREASURY',
    sovereign: 'US',
  },
  'CAD.GOC': {
    label: 'Government of Canada',
    currency: 'CAD',
    dataset: 'boc-benchmarks',
    method: 'par',
    calendar: 'CA.TOR',
    settlementDays: 1,
    frequency: 'SemiAnnual',
    dayCount: 'Act365Fixed',
    benchmarkId: 'CA.GOC',
    sovereign: 'CA',
  },
  'EUR.AAA': {
    label: 'Euro area AAA',
    currency: 'EUR',
    dataset: 'ecb-aaa',
    method: 'zero',
    calendar: 'EU.TARGET2',
    benchmarkId: 'EU.AAA.ECB',
    sovereign: 'EU.AAA',
  },
  'EUR.ALL': {
    label: 'Euro area all issuers',
    currency: 'EUR',
    dataset: 'ecb-all',
    method: 'zero',
    calendar: 'EU.TARGET2',
    benchmarkId: 'EU.ALL.ECB',
    sovereign: 'EU.ALL',
  },
};

// The checked curve route interpolates zero-rate nodes linearly or by cubic spline.
export const INTERPOLATIONS = ['Linear', 'CubicSpline'];

function role(def) {
  return { kind: 'Government', benchmark_id: def.benchmarkId };
}

/**
 * Build one curve. Returns { id, key, recipe, tradeDate, settlement, method,
 * repricing, nodes, ms, error }; `recipe` is null on failure.
 */
export async function buildCurve(engine, id, dataset, { interpolation } = {}) {
  const def = CURVE_DEFS[id];
  const tradeDate = dataset.asOf;
  const version = `${dataset.asOf}.${dataset.status}${dataset.revision ? `.r${dataset.revision}` : ''}`;
  const key = R.curveKey(id, version);
  const points = dataset.points
    .filter((p) => p.value !== '' && p.value !== null && p.value !== undefined)
    .sort((a, b) => tenorYears(a.tenor) - tenorYears(b.tenor));
  const base = { id, def, key, tradeDate, dataset: { id: dataset.id, asOf: dataset.asOf, status: dataset.status } };

  if (def.method === 'zero') {
    const interp = interpolation || 'CubicSpline';
    const nodes = points
      .map((p) => [tenorYears(p.tenor), Number(percentToRate(p.value))])
      .sort((a, b) => a[0] - b[0]);
    const recipe = R.directZeroCurve({
      key, currency: def.currency, referenceDate: tradeDate, role: role(def), nodes,
      compounding: 'Continuous', interpolation: interp, extrapolation: 'Flat',
    });
    const res = await engine.call(R.curvesBuild([recipe]), { label: `${def.label} curve` });
    if (!res.ok) return { ...base, recipe: null, error: res.error, ms: res.ms };
    const built = res.data.curves[0];
    return { ...base, recipe, settlement: tradeDate, method: 'zero', nodes,
      grid: built.interpolation_grid, domain: built.domain, ms: res.ms, interpolation: interp };
  }

  const interp = interpolation || 'Linear';
  const settlement = addBusinessDays(def.calendar, tradeDate, def.settlementDays);
  const quotes = points.map((p) => ({ tenor: p.tenor, parPct: p.value, maturity: addTenor(settlement, p.tenor) }));
  const req = R.parCurveCalibration({
    key, currency: def.currency, calendar: def.calendar, tradeDate, settlement,
    settlementDays: def.settlementDays, benchmarkId: def.benchmarkId, quotes,
    frequency: def.frequency, dayCount: def.dayCount, interpolation: interp,
  });
  const res = await engine.call(req, { label: `${def.label} par calibration` });
  if (!res.ok) return { ...base, recipe: null, error: res.error, ms: res.ms, settlement };
  const curve = res.data.result.curves[0];
  const grid = curve.interpolation_grid;
  const nodes = grid.tenors.map((t, i) => [t, grid.values[i]]);
  const recipe = R.directZeroCurve({
    key, currency: def.currency, referenceDate: tradeDate, role: role(def), nodes,
    compounding: 'Continuous', interpolation: interp, extrapolation: 'Flat',
  });
  return {
    ...base, recipe, settlement, method: 'par', nodes, grid, domain: curve.domain, interpolation: interp,
    repricing: res.data.repricing, iterations: curve.diagnostics && curve.diagnostics.iterations,
    evaluations: res.data.price_evaluations, ms: res.ms, quotes,
  };
}

/** Zero, 1Y-forward-style instantaneous proxy and DF on a tenor grid, natively queried. */
export async function sampleCurve(engine, curve, { maxYears = 30, step = 0.25 } = {}) {
  const grid = [];
  for (let t = step; t <= maxYears + 1e-9; t += step) grid.push(+t.toFixed(6));
  const fwdStep = 0.25;
  const requests = [];
  for (const t of grid) {
    requests.push(R.curveQuery([curve.recipe], curve.key, R.zeroQuery(t, 'Continuous')));
    requests.push(R.curveQuery([curve.recipe], curve.key, R.forwardQuery(Math.max(0, t - fwdStep), t, 'Continuous')));
    requests.push(R.curveQuery([curve.recipe], curve.key, R.dfQuery({ Tenor: t })));
  }
  const res = await engine.callMany(requests, { label: `${curve.def.label} curve sample ×${requests.length}` });
  const rows = grid.map((t, i) => ({
    t,
    zero: res[3 * i].ok ? res[3 * i].data.value : NaN,
    fwd: res[3 * i + 1].ok ? res[3 * i + 1].data.value : NaN,
    df: res[3 * i + 2].ok ? res[3 * i + 2].data.value : NaN,
  }));
  const failed = res.find((r) => !r.ok);
  return { rows, error: failed && failed.error };
}

/** Recalibrate with every par quote shifted by `shiftBp(tenorYears)` basis points. */
export function shiftedDataset(dataset, shiftBp, label) {
  return {
    ...dataset,
    status: 'derived',
    revision: label,
    points: dataset.points.map((p) => {
      const bp = shiftBp(tenorYears(p.tenor));
      const v = Number(p.value) + bp / 100;
      return { ...p, value: v.toFixed(6).replace(/0+$/, '').replace(/\.$/, '') };
    }),
  };
}
