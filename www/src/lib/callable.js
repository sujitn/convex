// Callable fixed-rate bonds: optional-bond pricing / OAS / effective risk
// under an explicit Hull-White one-factor tree, plus the strict `workout`
// operation (yield/price to every legal call date and maturity).
//
// The exercise provisions come from `bondTerms(sec).exercise` (terms.js):
// every call `{date, price}` is a European or Bermudan *call* at `price` per
// 100 clean, accrued paid. American (continuous) exercise is not modelled
// here and the native engine rejects it as `unsupported`.

import { request, inputs, bondContext } from './requests.js';
import { bondInput, securityId } from './terms.js';

// ---- Hull-White model (assumed parameters) -----------------------------------

export const HW1F_KEY = { id: 'convex-web.hw1f.assumed', version: '1' };

/**
 * Hull-White one-factor model with user-ASSUMED parameters (not calibrated to
 * swaptions). `meanReversion` and `volatility` are annual decimals
 * (0.03 = 3% mean-reversion speed; 0.01 = 100bp normal short-rate vol).
 */
export function hw1fAssumedModel({ meanReversion, volatility, key = HW1F_KEY, reason } = {}) {
  const a = Number(meanReversion);
  const s = Number(volatility);
  if (!Number.isFinite(a) || !Number.isFinite(s)) throw new Error('Hull-White parameters must be numbers');
  return {
    key,
    kind: 'assumed',
    parameters: { mean_reversion: a, volatility: s },
    reason: reason ||
      `User-assumed Hull-White one-factor parameters (mean reversion ${a}, volatility ${s}); not calibrated to market swaption or cap quotes`,
  };
}

export const DEFAULT_GRID = {
  absolute_price_tolerance: 0.004,
  base_steps: 128,
  refinements: 3,
  relative_price_tolerance: 1e-8,
};

export const RISK_GRID = { ...DEFAULT_GRID, absolute_price_tolerance: 0.01 };

// ---- optional_bond ------------------------------------------------------------

function optionalRequest(sec, settlement, { discountCurve, model, grid = DEFAULT_GRID, exerciseState = 'alive_unexercised' }, action) {
  if (!discountCurve) throw new Error('a discount curve is required');
  if (!model) throw new Error('a Hull-White model is required');
  return request(inputs({ calendars: [sec.calendar], curves: [discountCurve] }), {
    kind: 'optional_bond',
    input: {
      bond: bondInput(sec),
      context: { exercise_state: exerciseState, on_payment: 'Exclude', settlement },
      discount: discountCurve.key,
      model,
      projection: null,
      projection_context: bondContext(settlement),
      settings: { exercise_priority: 'issuer_last', grid },
    },
    action,
  });
}

/** Price at a constant OAS (annual decimal; 0.0025 = 25bp) over the discount curve. */
export function optionalPrice(sec, settlement, opts) {
  return optionalRequest(sec, settlement, opts, { kind: 'price', oas_decimal: Number(opts.oas ?? 0) });
}

export const OAS_SOLVE = { lower: -0.05, upper: 0.1, max_iterations: 64, price_tolerance: 0.004 };

/** OAS that reproduces `dirtyPrice` (per 100, accrued included). */
export function optionalOas(sec, settlement, opts) {
  return optionalRequest(sec, settlement, opts, {
    kind: 'oas',
    dirty_price: Number(opts.dirtyPrice),
    settings: opts.solve || OAS_SOLVE,
  });
}

/**
 * Effective duration / convexity by bumping the chosen axis ('discount' =
 * parallel shift of the discount curve at fixed OAS; 'oas' = bump of the OAS).
 * `bump` is an annual decimal (0.01 = 100bp).
 */
export function optionalRisk(sec, settlement, opts) {
  // Bumped trees converge more slowly than the base price: a 10y callable at
  // +-100bp shows ~0.01 price and ~+-1.5 convexity lattice noise across the
  // 128..1024-step refinements, so the default acceptance bands are wider
  // than for a price. They are returned as-is to the engine, never relaxed
  // silently: failures surface as native `no_convergence`.
  opts = { ...opts, grid: opts.grid || RISK_GRID };
  return optionalRequest(sec, settlement, opts, {
    kind: 'risk',
    axis: opts.axis || 'discount',
    bump: opts.bump ?? 0.01,
    oas_decimal: Number(opts.oas ?? 0),
    settings: opts.tolerances || {
      absolute_convexity_tolerance: 3,
      absolute_duration_tolerance: 0.01,
      relative_convexity_tolerance: 0,
      relative_duration_tolerance: 0,
    },
  });
}

/** Reprice after a parallel shift (annual decimal) of the discount curve at fixed OAS. */
export function optionalScenario(sec, settlement, opts) {
  return optionalRequest(sec, settlement, opts, {
    kind: 'scenario',
    oas_decimal: Number(opts.oas ?? 0),
    shift: { discount: Number(opts.discountShift ?? 0), projection: 0 },
  });
}

// ---- workout ---------------------------------------------------------------------

export const WORKOUT_INVERSE = { lower: -0.05, upper: 0.5, max_iterations: 128, parameter_tolerance: 1e-12, price_tolerance: 1e-8 };

/**
 * Workout context. `earliestNotice` is the first date an issuer notice could
 * be given (the caller's policy date: the library embeds no default). Pass
 * `notified` = `{provision, exercise, notice_date}` for an already-issued
 * notice, which forces that single path.
 */
export function workoutContext(tradeDate, settlement, { earliestNotice, notified = null, referenceDate } = {}) {
  let notice;
  if (notified) {
    notice = { kind: 'notified', provision: notified.provision, exercise: notified.exercise, notice_date: notified.notice_date };
  } else {
    // Information cut-off must lie within [trade, settlement]; the first
    // possible notice must not precede it.
    notice = { kind: 'unnotified', earliest_notice: earliestNotice || settlement };
  }
  return {
    notice,
    on_payment: 'Exclude',
    reference_date: referenceDate || tradeDate,
    settlement,
    trade_date: tradeDate,
  };
}

function workoutRequest(sec, tradeDate, settlement, convention, action, ctxOpts) {
  const id = securityId(sec);
  return request(inputs({ calendars: [sec.calendar] }), {
    kind: 'workout',
    input: {
      bond: bondInput(sec),
      quote_revision: { id: `${id}.QUOTE`, version: 1 },
      reference: { id, version: 1 },
    },
    context: workoutContext(tradeDate, settlement, ctxOpts),
    convention,
    action,
  });
}

/**
 * Yield to every legal path (maturity and each call date) and the worst, for
 * a CLEAN price per 100 (set `kind: 'dirty'` for a dirty target).
 * `convention` = `{ compounding, day_count }` of the dated-cash yield,
 * e.g. `{ compounding: 'SemiAnnual', day_count: 'Thirty360US' }`.
 * Yields are annual decimals (0.045 = 4.5%).
 */
export function workoutPaths(sec, tradeDate, settlement, { price, kind = 'clean', convention, earliestNotice, notified,
  maxDates = 1024, maxCashEvaluations = 10_000_000, inverse = WORKOUT_INVERSE }) {
  return workoutRequest(sec, tradeDate, settlement, convention, {
    kind: 'worst',
    target: { kind, value: Number(price) },
    settings: { inverse, max_dates: maxDates, max_cash_evaluations: maxCashEvaluations },
  }, { earliestNotice, notified });
}

/** Yield for one path: `selection` = `{kind:'maturity'}` or `{kind:'exercise', provision, date}`. */
export function workoutYield(sec, tradeDate, settlement, { price, kind = 'clean', convention, selection, earliestNotice, notified,
  inverse = WORKOUT_INVERSE }) {
  return workoutRequest(sec, tradeDate, settlement, convention, {
    kind: 'yield', selection, target: { kind, value: Number(price) }, settings: inverse,
  }, { earliestNotice, notified });
}

/** Price for one path at an annual decimal yield. */
export function workoutPrice(sec, tradeDate, settlement, { decimalYield, convention, selection, earliestNotice, notified }) {
  return workoutRequest(sec, tradeDate, settlement, convention, {
    kind: 'price', selection, decimal_yield: Number(decimalYield),
  }, { earliestNotice, notified });
}

/**
 * Flatten a `workout` worst response into UI rows. Yields are annual decimals
 * (0.045 = 4.5%); prices are per 100 of original face. `worst` is the path
 * the engine selected (minimum yield among maturity and issuer calls; puts
 * would be excluded); `forcedByNotice` is true when an issued notice fixed it.
 */
export function workoutRows(data) {
  const q = data.quotation;
  const paths = q.quotes.map((x, i) => ({
    index: i,
    kind: x.selection.kind === 'maturity' ? 'maturity' : (x.exercise?.right || 'call'),
    provision: x.selection.provision ?? null,
    date: x.selection.date ?? null,
    decimalYield: x.decimal_yield,
    cleanPer100: x.clean_per100,
    dirtyPer100: x.dirty_per100,
    accruedPer100: x.accrued_per100,
    redemptionCash: x.exercise ? x.exercise.settlement_cash : null,
  }));
  return { paths, worst: paths[q.selected_index], forcedByNotice: q.forced_by_notice };
}
