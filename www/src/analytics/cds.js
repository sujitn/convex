// Credit default swaps through the engine: the discount curve re-supplied as
// discount factors, native calibration of a survival curve from the par
// spreads you supply, survival/hazard/default queries, standard contract
// pricing, spread ↔ upfront conversion and signed credit risk.

import * as R from '../lib/requests.js';
import * as C from '../lib/cds.js';
import { addMonths } from '../lib/dates.js';
import { bpToRate, shift } from '../lib/decimal.js';
import { curveForCurrency } from './securities.js';

/**
 * Curves a CDS in this currency can discount on: your swap (Discount-role)
 * curves, then government curves, which can only stand in for a swap curve.
 */
export function discountChoices(curveDefs, currency) {
  const defs = Object.values(curveDefs).filter((d) => d.currency === currency);
  const swaps = defs.filter((d) => d.custom && d.role === 'discount');
  const gov = curveForCurrency(currency);
  const govs = defs.filter((d) => d.role !== 'discount').sort((a, b) => (a.id === gov ? -1 : b.id === gov ? 1 : 0));
  return [
    ...swaps.map((d) => ({ id: d.id, def: d, kind: 'swap', label: `${d.label} (your swap curve)` })),
    ...govs.map((d) => ({ id: d.id, def: d, kind: 'government', label: `${d.label}${d.custom ? ' (yours)' : ''}, government stand-in` })),
  ];
}

/** 'auto' takes the swap curve you use for spreads, any swap curve, then the government curve. */
export function resolveDiscount(choice, choices, spreadCurveId) {
  if (choice && choice !== 'auto' && choices.some((c) => c.id === choice)) return choice;
  const swap = choices.find((c) => c.kind === 'swap' && c.id === spreadCurveId) || choices.find((c) => c.kind === 'swap');
  return (swap || choices[0] || {}).id || null;
}

/** Discount factors far enough to cover the last premium date after the longest pillar. */
export function discountHorizon(def, tradeDate) {
  const { quotes } = C.creditQuotes(def, tradeDate);
  const last = quotes.length ? quotes[quotes.length - 1].maturity : C.standardMaturity(tradeDate, '10Y');
  return addMonths(last, 3);
}

/**
 * The CDS pricer takes a collateral curve of dated discount factors with
 * log-linear interpolation and no extrapolation. Discount factors are queried
 * natively from the chosen curve monthly from its curve date to `horizon` and
 * re-supplied in that form.
 */
export async function discountForCds(engine, curve, horizon, kind) {
  const source = curve && curve.def ? { id: curve.def.id, label: curve.def.label, kind, asOf: curve.tradeDate,
    status: curve.dataset && curve.dataset.status, example: !!curve.def.example } : null;
  if (!curve || !curve.recipe) {
    return { source, error: (curve && curve.error) || { code: 'missing_input', message: 'The discount curve has not been built.' } };
  }
  const ref = curve.tradeDate;
  const dates = [ref];
  for (let m = 1; dates[dates.length - 1] < horizon; m++) dates.push(addMonths(ref, m));
  const res = await engine.callMany(dates.map((d) => R.curveQuery([curve.recipe], curve.key, R.dfQuery({ Date: d }))),
    { label: `${curve.def.label} discount factors for CDS ×${dates.length}` });
  const failed = res.find((r) => !r.ok);
  if (failed) return { source, error: failed.error };
  const nodes = dates.map((d, i) => [d, res[i].data.value]);
  const key = R.curveKey(`${curve.key.id}.DF`, `${curve.key.version}.to.${horizon}`);
  const recipe = C.discountFactorCurve({ key, currency: curve.def.currency, referenceDate: ref, nodes });
  return { source, key, recipe, nodes, horizon, ms: res.reduce((a, r) => a + (r.ms || 0), 0) };
}

/**
 * Calibrate one credit curve. Returns { def, key, cal, dates, recipe, survival,
 * discount, quotes, repricing, grid, diagnostics, evaluations, ms, error };
 * `survival` (the fitted grid as direct nodes) is null on failure.
 */
export async function buildCredit(engine, def, { tradeDate, discount }) {
  const base = { def, tradeDate, discount, survival: null };
  if (!discount || !discount.recipe) {
    return { ...base, error: (discount && discount.error) || { code: 'missing_input', message: 'No discount curve.' } };
  }
  if (tradeDate < discount.recipe.reference_date) {
    return { ...base, error: { code: 'invalid_input', message: `The trade date is before the discount curve's date `
      + `(${discount.recipe.reference_date}). Move the trade date to or after it.` } };
  }
  const { quotes, problems } = C.creditQuotes(def, tradeDate);
  if (problems.length) return { ...base, error: { code: 'invalid_input', message: problems.join('. ') } };
  if (!quotes.length) return { ...base, error: { code: 'missing_input', message: 'Enter at least one par spread.' } };
  const cal = C.cdsCalendar(def.calendar);
  const dates = C.contractDates(def.calendar, tradeDate);
  const key = C.survivalKey(def, tradeDate, discount.key);
  const recipe = C.creditRecipe({ def, tradeDate, key, discountKey: discount.key, quotes });
  const res = await engine.call(C.creditCalibration(cal, discount.recipe, recipe), { label: `${def.entity} credit calibration` });
  const out = { ...base, key, cal, dates, recipe, quotes, ms: res.ms };
  if (!res.ok) return { ...out, error: res.error };
  const fitted = res.data.result.curves.find((c) => c.recipe.key.id === key.id);
  const grid = fitted.interpolation_grid;
  // The grid is the origin plus one node per pillar; re-supply it on those dates.
  const points = grid.tenors.length === recipe.pillars.length + 1
    ? [tradeDate, ...recipe.pillars].map((d) => ({ Date: d }))
    : grid.tenors.map((t) => ({ Tenor: t }));
  const survival = C.survivalCurve(recipe, points.map((p, i) => [p, grid.values[i]]));
  return { ...out, survival, grid, diagnostics: fitted.diagnostics, repricing: res.data.repricing,
    evaluations: res.data.price_evaluations };
}

/** Survival, cumulative and interval default probability and hazard at each pillar, queried natively. */
export async function creditTable(engine, built) {
  const curves = [built.discount.recipe, built.survival];
  const q = (query) => C.survivalQuery(built.cal, curves, built.key, query);
  const requests = [];
  built.quotes.forEach((row, i) => {
    const prev = i ? built.quotes[i - 1].maturity : built.tradeDate;
    requests.push(q(C.survivalAt(row.maturity)));
    requests.push(q(C.defaultBetween(built.tradeDate, row.maturity)));
    requests.push(q(C.defaultBetween(prev, row.maturity, true)));
    requests.push(q(C.hazardAt(row.maturity, 'Left')));
  });
  const res = await engine.callMany(requests, { label: `${built.def.entity} survival queries ×${requests.length}` });
  const value = (r) => (r.ok ? r.data.value : NaN);
  const rows = built.quotes.map((row, i) => ({
    ...row,
    years: built.grid ? built.grid.tenors[i + 1] : NaN,
    survival: value(res[4 * i]),
    cumulative: value(res[4 * i + 1]),
    interval: value(res[4 * i + 2]),
    hazard: value(res[4 * i + 3]),
  }));
  const failed = res.find((r) => !r.ok);
  return { rows, error: failed && failed.error };
}

/** Contract terms on a built curve. `contract` = { side, notional, coupon (bp text), maturity }. */
export function contractTerms(built, contract) {
  const { def } = built;
  return C.cdsTerms({
    tradeId: `${def.id}.${contract.maturity}.${contract.coupon}`,
    side: contract.side,
    currency: def.currency,
    notional: contract.notional,
    couponDecimal: bpToRate(contract.coupon),
    reference: C.creditReference(def),
    maturity: contract.maturity,
    dates: built.dates,
    cal: built.cal,
    discountKey: built.discount.key,
    survival: built.key,
  });
}

export async function priceContract(engine, built, contract) {
  const terms = contractTerms(built, contract);
  const context = C.pricingContext(built.dates);
  const res = await engine.call(C.cdsPrice(built.cal, [built.discount.recipe, built.survival], context, terms),
    { label: `CDS ${built.def.entity} ${contract.maturity} price` });
  return res.ok ? { terms, context, price: res.data, ms: res.ms } : { terms, context, error: res.error, ms: res.ms };
}

/**
 * Conventional quote conversion on one flat hazard at the contract coupon:
 * `quote` = { kind: 'spread', bp } or { kind: 'upfront', points } (text).
 */
export async function convertQuote(engine, built, contract, quote) {
  const terms = contractTerms(built, contract);
  const context = C.pricingContext(built.dates);
  const native = quote.kind === 'spread'
    ? C.runningSpread(Number(bpToRate(quote.bp)))
    : C.cleanUpfront(Number(shift(quote.points, 2)));
  const res = await engine.call(C.cdsConvert(built.cal, [built.discount.recipe], context, terms, native),
    { label: `CDS ${quote.kind === 'spread' ? 'spread → upfront' : 'upfront → spread'}` });
  return res.ok ? { quote, data: res.data } : { quote, error: res.error };
}

/**
 * Signed risk for the position, each a central difference executed by the
 * engine with the curve refitted on both sides: CS01 per par-spread quote,
 * every quote ±1 bp together, recovery ±1 point (quotes held, or survival
 * held) and the discount curve ±1 bp.
 */
export async function contractRisk(engine, built, terms, context) {
  const { cal } = built;
  const disc = built.discount.recipe;
  const input = C.creditRiskInput({ recipe: built.recipe, terms, context });
  const helpers = built.recipe.helpers;
  const requests = [
    ...helpers.map((h) => C.quoteCs01(cal, disc, input, h)),
    C.quoteScenario(cal, disc, input, 1),
    C.quoteScenario(cal, disc, input, -1),
    C.recoveryRisk(cal, disc, input, 'RecalibrateQuotes'),
    C.recoveryRisk(cal, disc, input, 'HoldSurvival'),
    C.discountRisk(cal, disc, input),
  ];
  const res = await engine.callMany(requests, { label: `CDS ${built.def.entity} risk ×${requests.length}` });
  const n = helpers.length;
  const sens = (r) => (r.ok ? { value: r.data.value, measure: r.data.measure } : { error: r.error });
  const buckets = built.quotes.map((q, i) => ({ tenor: q.tenor, maturity: q.maturity, ...sens(res[i]) }));
  const up = res[n];
  const down = res[n + 1];
  const parallel = up.ok && down.ok
    ? { value: (up.data.shocked.net_pv - down.data.shocked.net_pv) / 2, up: up.data.pv_change_currency,
      down: down.data.pv_change_currency }
    : { error: (!up.ok ? up : down).error };
  return {
    buckets,
    parallel,
    recovery: sens(res[n + 2]),
    recoveryHeld: sens(res[n + 3]),
    rates: sens(res[n + 4]),
  };
}
