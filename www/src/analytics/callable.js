// Callable-bond valuation: Hull-White one-factor OAS, option value, effective
// risk, a volatility sensitivity and the workout paths, all native.

import * as R from '../lib/requests.js';
import { optionalPrice, optionalOas, optionalRisk, hw1fAssumedModel, workoutPaths, workoutRows } from '../lib/callable.js';
import { cashConvention } from '../lib/cash.js';

/**
 * The optional-bond route discounts on a curve in the Discount role. The
 * government zero nodes are re-supplied under that role and a distinct key,
 * so the request states plainly which curve is the discount curve.
 */
export function discountCurveFrom(curve) {
  const g = curve.grid;
  return R.directZeroCurve({
    key: R.curveKey(`${curve.id}.DISCOUNT`, curve.key.version),
    currency: curve.def.currency,
    referenceDate: curve.tradeDate,
    role: { kind: 'Discount', collateral_currency: curve.def.currency },
    nodes: g.tenors.map((t, i) => [t, g.values[i]]),
    interpolation: curve.interpolation,
    extrapolation: 'Flat',
  });
}

export const VOL_LADDER = [0.005, 0.0075, 0.01, 0.0125, 0.015];

export async function analyseCallable(engine, { sec, tradeDate, settlement, curve, cleanPrice, meanReversion, volatility }) {
  const label = sec.ticker || sec.name;
  const discountCurve = discountCurveFrom(curve);
  const model = hw1fAssumedModel({ meanReversion, volatility });
  const out = { errors: {} };

  // Accrued from a model price (same contract), then the OAS for the dirty price.
  const p0 = await engine.call(optionalPrice(sec, settlement, { discountCurve, model, oas: 0 }), { label: `${label} model price` });
  if (!p0.ok) return { fatal: p0.error };
  const accrued = p0.data.accrued;
  const dirty = cleanPrice + accrued;
  out.accrued = accrued;
  out.dirty = dirty;
  out.exercises = p0.data.exercises;

  const oas = await engine.call(optionalOas(sec, settlement, { discountCurve, model, dirtyPrice: dirty }), { label: `${label} OAS` });
  if (!oas.ok) return { ...out, fatal: oas.error };
  out.oas = oas.data.oas_decimal;
  out.oasPrice = oas.data.price;
  out.iterations = oas.data.iterations;
  out.grids = oas.data.price.grids;

  // Straight bond at the same constant spread: the option is the difference.
  const straight = await engine.call(
    R.bondPrice({ ...sec, calls: [] }, settlement, R.spreadMethod(curve.key, curve.spreadRole, out.oas), [curve.recipe]),
    { label: `${label} straight bond at OAS` },
  );
  if (straight.ok) {
    out.straightDirty = straight.data.dirty_per100;
    out.optionValue = straight.data.dirty_per100 - out.oasPrice.dirty_price;
  } else out.errors.straight = straight.error;

  // Effective risk needs the bumped trees to converge; a 50bp parallel shift
  // is tried first and, if the native refinement check fails, 100bp. The
  // shift actually used is reported with the result.
  for (const bump of [0.005, 0.01]) {
    const risk = await engine.call(optionalRisk(sec, settlement, { discountCurve, model, oas: out.oas, axis: 'discount', bump }),
      { label: `${label} effective risk ±${bump * 10000}bp` });
    if (risk.ok) {
      out.risk = { ...risk.data, bump };
      delete out.errors.risk;
      break;
    }
    out.errors.risk = risk.error;
    if (risk.error.code !== 'no_convergence') break;
  }

  const vols = await engine.callMany(
    VOL_LADDER.map((v) => optionalOas(sec, settlement, { discountCurve, model: hw1fAssumedModel({ meanReversion, volatility: v }), dirtyPrice: dirty })),
    { label: `${label} OAS by volatility` },
  );
  out.volLadder = VOL_LADDER.map((v, i) => ({ vol: v, oas: vols[i].ok ? vols[i].data.oas_decimal : NaN, error: vols[i].ok ? null : vols[i].error }));

  const wk = await engine.call(workoutPaths(sec, tradeDate, settlement, { price: cleanPrice, convention: cashConvention(sec) }),
    { label: `${label} workouts` });
  if (wk.ok) out.workout = workoutRows(wk.data);
  else out.errors.workout = wk.error;
  out.model = model;
  out.discountKey = discountCurve.key;
  return out;
}
