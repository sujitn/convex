// Curve scenarios and key-rate risk by recalibration: each scenario shifts the
// par quotes, the engine recalibrates the curve, and the bond is repriced at
// an unchanged Z-spread. Nothing is approximated from durations.

import * as R from '../lib/requests.js';
import { CURVE_DEFS, shiftedDataset, hasValue } from './curves.js';
import { addTenor, tenorYears } from '../lib/dates.js';
import { addBusinessDays } from '../lib/calendars.js';

export const SCENARIOS = [
  { id: 'p-200', label: 'Parallel −200', shift: () => -200 },
  { id: 'p-100', label: 'Parallel −100', shift: () => -100 },
  { id: 'p-50', label: 'Parallel −50', shift: () => -50 },
  { id: 'p-25', label: 'Parallel −25', shift: () => -25 },
  { id: 'p+25', label: 'Parallel +25', shift: () => 25 },
  { id: 'p+50', label: 'Parallel +50', shift: () => 50 },
  { id: 'p+100', label: 'Parallel +100', shift: () => 100 },
  { id: 'p+200', label: 'Parallel +200', shift: () => 200 },
  { id: 'bull-steep', label: 'Bull steepener (2s −40, 30s −10)', shift: (t) => pivot(t, -40, -10) },
  { id: 'bear-steep', label: 'Bear steepener (2s +10, 30s +40)', shift: (t) => pivot(t, 10, 40) },
  { id: 'bull-flat', label: 'Bull flattener (2s −10, 30s −40)', shift: (t) => pivot(t, -10, -40) },
  { id: 'bear-flat', label: 'Bear flattener (2s +40, 30s +10)', shift: (t) => pivot(t, 40, 10) },
  { id: 'twist', label: 'Twist (2s +25, 30s −25)', shift: (t) => pivot(t, 25, -25) },
];

/** Linear in tenor between the 2Y and 30Y shifts, flat outside. */
function pivot(t, short, long) {
  if (t <= 2) return short;
  if (t >= 30) return long;
  return short + ((long - short) * (t - 2)) / 28;
}

/** The par recalibration of a shifted dataset, with the same points and interpolation `buildCurve` used. */
export function calibrationRequest(def, ds, interpolation) {
  const tradeDate = ds.asOf;
  const settlement = addBusinessDays(def.calendar, tradeDate, def.settlementDays);
  const key = R.curveKey(def.id, `${ds.asOf}.${ds.revision}`);
  const quotes = ds.points
    .filter(hasValue)
    .sort((a, b) => tenorYears(a.tenor) - tenorYears(b.tenor))
    .map((p) => ({ tenor: p.tenor, parPct: p.value, maturity: addTenor(settlement, p.tenor) }));
  return R.parCurveCalibration({ key, currency: def.currency, calendar: def.calendar, tradeDate, settlement,
    settlementDays: def.settlementDays, benchmarkId: def.benchmarkId, quotes, frequency: def.frequency, dayCount: def.dayCount,
    interpolation });
}

function directFrom(def, ds, data) {
  const c = data.result.curves[0];
  const g = c.interpolation_grid;
  return R.directZeroCurve({ key: c.recipe.key, currency: def.currency, referenceDate: ds.asOf,
    role: { kind: 'Government', benchmark_id: def.benchmarkId }, nodes: g.tenors.map((t, i) => [t, g.values[i]]),
    interpolation: c.recipe.interpolation, extrapolation: 'Flat' });
}

/**
 * Reprice `sec` at constant Z under each shifted dataset.
 * `variants` = [{ id, label, shift: (tenorYears) => bp }]. Returns rows with
 * the scenario dirty price, or the native error for that scenario.
 */
export async function repriceUnderShifts(engine, { sec, settlement, curve, dataset, zDecimal, variants, label }) {
  const def = curve.def || CURVE_DEFS[curve.id];
  const role = R.governmentRole(def.benchmarkId);
  const shifted = variants.map((v) => shiftedDataset(dataset, v.shift, v.id));
  // Recalibrate exactly as the base curve was built, interpolation included,
  // so a scenario differs from the base only by its shift.
  const cals = await engine.callMany(shifted.map((ds) => calibrationRequest(def, ds, curve.interpolation)),
    { label: `${label} recalibrations` });
  const prices = await engine.callMany(
    cals.map((c, i) => {
      if (!c.ok) return R.bondPrice(sec, settlement, R.spreadMethod(curve.key, role, zDecimal), [curve.recipe]);
      const direct = directFrom(def, shifted[i], c.data);
      return R.bondPrice(sec, settlement, R.spreadMethod(direct.key, role, zDecimal), [direct]);
    }),
    { label: `${label} scenario prices` },
  );
  return variants.map((v, i) => ({
    ...v,
    dirty: cals[i].ok && prices[i].ok ? prices[i].data.dirty_per100 : NaN,
    clean: cals[i].ok && prices[i].ok ? prices[i].data.clean_per100 : NaN,
    error: !cals[i].ok ? cals[i].error : !prices[i].ok ? prices[i].error : null,
  }));
}

/** One-basis-point bump per quoted par tenor: market-quote key-rate durations. */
export function keyRateVariants(dataset) {
  return dataset.points
    .filter(hasValue)
    .sort((a, b) => tenorYears(a.tenor) - tenorYears(b.tenor))
    .map((p) => ({ id: `kr-${p.tenor}`, label: p.tenor, tenor: p.tenor, shift: (t) => (Math.abs(t - tenorYears(p.tenor)) < 1e-9 ? 1 : 0) }));
}
