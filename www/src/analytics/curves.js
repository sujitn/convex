// Curve construction from market datasets, through the engine. Par yield
// sets (Treasury, Canada) are calibrated natively from par-coupon helpers;
// published zero curves (ECB) are supplied directly as continuous zero nodes;
// OIS swap rates you supply are calibrated natively from par swaps. The
// calibrated node grid is then re-supplied as a Direct curve with the same
// representation and interpolation, so later requests reuse it exactly.

import * as R from '../lib/requests.js';
import { addTenor, tenorYears } from '../lib/dates.js';
import { addBusinessDays } from '../lib/calendars.js';
import { percentToRate } from '../lib/decimal.js';
import { OIS_INDICES, oisCurveCalibration } from '../lib/ois.js';

const BUILT_IN = {
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
    note: 'Bank of Canada benchmark bond yields are calibrated here as par yields. That is an approximation: '
      + 'benchmark bonds trade away from par, so their yields are not exactly par yields.',
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

export const CURVE_DEFS = Object.fromEntries(Object.entries(BUILT_IN).map(([id, d]) => [id, { id, role: 'government', ...d }]));

export const CALENDAR_FOR = { USD: 'US.SIFMA', EUR: 'EU.TARGET2', GBP: 'GB.LON', CAD: 'CA.TOR' };

/** A dataset point with a quote; '' / null / undefined mark a tenor with none. */
export function hasValue(p) {
  return p.value !== '' && p.value !== null && p.value !== undefined;
}

/** Curve role in the recipe: a government benchmark curve or a collateral discount curve. */
export function curveRole(def) {
  return def.role === 'discount'
    ? { kind: 'Discount', collateral_currency: def.currency }
    : { kind: 'Government', benchmark_id: def.benchmarkId };
}

/** The matching bond reference role for spread pricing. */
export function spreadRole(def) {
  return def.role === 'discount' ? R.collateralRole() : R.governmentRole(def.benchmarkId);
}

export const CURVE_METHODS = {
  ois: 'OIS swap rates',
  par: 'Par yields',
  zero: 'Zero rates (continuous)',
};

/** A new curve you supply. Its points live on the definition, which is also its dataset. */
export function customCurveDef({ id, label, currency, method, asOf, points = [], index, role }) {
  const base = {
    id, label, currency, method, asOf, points, custom: true, status: 'manual', revision: 1,
    interpolation: 'Linear', benchmarkId: id,
  };
  if (method === 'ois') {
    const conv = OIS_INDICES[index];
    return { ...base, role: 'discount', index, calendar: conv.calendar, spotLag: conv.spotLag,
      paymentDelay: conv.paymentDelay, dayCount: conv.dayCount };
  }
  if (method === 'par') {
    return { ...base, role: 'government', calendar: CALENDAR_FOR[currency] || 'US.SIFMA', settlementDays: 1,
      frequency: 'SemiAnnual', dayCount: currency === 'USD' ? 'ActActIcma' : 'Act365Fixed' };
  }
  return { ...base, role: role || 'discount', calendar: CALENDAR_FOR[currency] || 'US.SIFMA', interpolation: 'CubicSpline' };
}

// The checked curve route interpolates zero-rate nodes linearly or by cubic spline.
export const INTERPOLATIONS = ['Linear', 'CubicSpline'];

/**
 * Build one curve from a definition (or a built-in id) and its dataset.
 * Returns { id, def, key, recipe, spreadRole, tradeDate, settlement, method,
 * repricing, nodes, ms, error }; `recipe` is null on failure.
 */
export async function buildCurve(engine, idOrDef, dataset, { interpolation } = {}) {
  const def = typeof idOrDef === 'string' ? CURVE_DEFS[idOrDef] : idOrDef;
  const { id } = def;
  const tradeDate = dataset.asOf;
  const version = `${dataset.asOf}.${dataset.status}${dataset.revision ? `.r${dataset.revision}` : ''}`;
  const key = R.curveKey(id, version);
  const points = dataset.points.filter(hasValue).sort((a, b) => tenorYears(a.tenor) - tenorYears(b.tenor));
  const base = { id, def, key, tradeDate, spreadRole: spreadRole(def),
    dataset: { id: dataset.id || id, asOf: dataset.asOf, status: dataset.status } };
  if (points.length < 2) {
    return { ...base, recipe: null, error: { code: 'missing_input', message: 'A curve needs at least two points.' } };
  }

  if (def.method === 'zero') {
    const interp = interpolation || 'CubicSpline';
    const nodes = points
      .map((p) => [tenorYears(p.tenor), Number(percentToRate(p.value))])
      .sort((a, b) => a[0] - b[0]);
    const recipe = R.directZeroCurve({
      key, currency: def.currency, referenceDate: tradeDate, role: curveRole(def), nodes,
      compounding: 'Continuous', interpolation: interp, extrapolation: 'Flat',
    });
    const res = await engine.call(R.curvesBuild([recipe]), { label: `${def.label} curve` });
    if (!res.ok) return { ...base, recipe: null, error: res.error, ms: res.ms };
    const built = res.data.curves[0];
    return { ...base, recipe, settlement: tradeDate, method: 'zero', nodes,
      grid: built.interpolation_grid, domain: built.domain, ms: res.ms, interpolation: interp };
  }

  if (def.method === 'ois') return buildOisCurve(engine, def, points, base, interpolation);

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
    key, currency: def.currency, referenceDate: tradeDate, role: curveRole(def), nodes,
    compounding: 'Continuous', interpolation: interp, extrapolation: 'Flat',
  });
  return {
    ...base, recipe, settlement, method: 'par', nodes, grid, domain: curve.domain, interpolation: interp,
    repricing: res.data.repricing, iterations: curve.diagnostics && curve.diagnostics.iterations,
    evaluations: res.data.price_evaluations, ms: res.ms, quotes,
  };
}

async function buildOisCurve(engine, def, points, base, interpolation) {
  const { key, tradeDate } = base;
  const conv = { ...OIS_INDICES[def.index], index: def.index, currency: def.currency };
  for (const f of ['calendar', 'spotLag', 'paymentDelay', 'dayCount']) if (def[f] !== undefined) conv[f] = def[f];
  const interp = interpolation || 'Linear';
  let req;
  try {
    req = oisCurveCalibration({ key, projectionKey: R.curveKey(`${def.id}.ON`, key.version), def: conv, tradeDate,
      quotes: points, interpolation: interp });
  } catch (e) {
    return { ...base, recipe: null, error: { code: e.code || 'invalid_input', message: e.message } };
  }
  const res = await engine.call(req, { label: `${def.label} OIS calibration` });
  if (!res.ok) return { ...base, recipe: null, error: res.error, ms: res.ms };
  const curve = res.data.result.curves.find((c) => c.recipe.key.id === key.id);
  const grid = curve.interpolation_grid;
  const nodes = grid.tenors.map((t, i) => [t, grid.values[i]]);
  const recipe = R.directZeroCurve({
    key, currency: def.currency, referenceDate: tradeDate, role: curveRole(def), nodes,
    compounding: 'Continuous', interpolation: interp, extrapolation: 'Flat',
  });
  return {
    ...base, recipe, settlement: tradeDate, method: 'ois', nodes, grid, domain: curve.domain, interpolation: interp,
    repricing: res.data.repricing, iterations: curve.diagnostics && curve.diagnostics.iterations,
    evaluations: res.data.price_evaluations, ms: res.ms, publications: res.data.publications.length,
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

/**
 * Recalibrate with every par quote shifted by `shiftBp(tenorYears)` basis
 * points. A tenor without a quote stays without one.
 */
export function shiftedDataset(dataset, shiftBp, label) {
  return {
    ...dataset,
    status: 'derived',
    revision: label,
    points: dataset.points.map((p) => {
      if (!hasValue(p)) return p;
      const bp = shiftBp(tenorYears(p.tenor));
      const v = Number(p.value) + bp / 100;
      return { ...p, value: v.toFixed(6).replace(/0+$/, '').replace(/\.$/, '') };
    }),
  };
}
