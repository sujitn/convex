// Single-name credit default swaps on the ISDA Standard Model route
// (`Standard183`). This file holds the standard contract dates, the credit
// curves you supply and the complete `library()` requests; nothing here prices
// anything. The engine infers no CDS convention, so every date, calendar,
// recovery and document revision below is stated explicitly.

import { calendarInput, CALENDAR_YEARS } from './calendars.js';
import { addDays, addMonths, parseIso, weekday, daysBetween, tenorMonths } from './dates.js';
import { decimal, isDecimal, shift } from './decimal.js';
import { SCHEMA_VERSION } from './requests.js';

export const CDS_TENORS = ['6M', '1Y', '2Y', '3Y', '4Y', '5Y', '7Y', '10Y'];
export const CDS_CURRENCIES = ['USD', 'EUR'];
export const SETTLEMENT_LAG = 3;
export const RISK_MODEL = 'convex-web.cds.central.v1';

// Recovery defaults are the standard-contract assumptions per seniority. The
// engine's survival-curve role names the seniority in snake case.
export const SENIORITIES = {
  SeniorUnsecured: { label: 'Senior unsecured', tier: 'SNRFOR', role: 'senior_unsecured', recovery: '40' },
  Subordinated: { label: 'Subordinated', tier: 'SUBLT2', role: 'subordinated', recovery: '25' },
};

// Restructuring clauses under the 2014 Definitions. They are revision pins
// that identify the contract; the Standard183 model does not value them.
export const DOC_CLAUSES = {
  XR14: 'XR14 · No restructuring',
  MR14: 'MR14 · Modified restructuring',
  MM14: 'MM14 · Modified-modified restructuring',
  CR14: 'CR14 · Full restructuring',
};
export const DEFAULT_DOC = { USD: 'XR14', EUR: 'MM14' };
export const DOCUMENT_PIN = { id: 'ISDA.2014.CreditDerivativesDefinitions', version: 1 };

export const COUPONS = { USD: ['100', '500'], EUR: ['25', '100', '300', '500', '750', '1000'] };

// Business days for premium and settlement dates: the union of the holiday
// tables, so a date is good only when every centre is open.
export const CDS_CALENDARS = {
  'NY+LON': { label: 'New York + London', parts: ['US.FED', 'GB.LON'] },
  NY: { label: 'New York', parts: ['US.FED'] },
  'TARGET+LON': { label: 'TARGET2 + London', parts: ['EU.TARGET2', 'GB.LON'] },
  TARGET: { label: 'TARGET2', parts: ['EU.TARGET2'] },
};
export const CALENDAR_CHOICES = { USD: ['NY+LON', 'NY'], EUR: ['TARGET+LON', 'TARGET'] };
export const DEFAULT_CALENDAR = { USD: 'NY+LON', EUR: 'TARGET+LON' };

// Illustrative par spreads so the screen can be shown end to end. They are not
// market data; a curve built from them is marked as an example.
export const EXAMPLE_SPREADS = {
  USD: '6M 32, 1Y 40, 2Y 55, 3Y 70, 4Y 84, 5Y 96, 7Y 112, 10Y 124',
  EUR: '6M 28, 1Y 35, 2Y 48, 3Y 61, 4Y 73, 5Y 84, 7Y 98, 10Y 110',
};

export function exampleSpreads(currency) {
  return EXAMPLE_SPREADS[currency].split(', ').map((cell) => {
    const [tenor, value] = cell.split(' ');
    return { tenor, value };
  });
}

const calendars = new Map();

/** The combined calendar: its `CalendarInput` and business-day arithmetic over it. */
export function cdsCalendar(code) {
  if (calendars.has(code)) return calendars.get(code);
  const def = CDS_CALENDARS[code];
  if (!def) throw new Error(`Unknown CDS calendar ${code}`);
  const parts = def.parts.map(calendarInput);
  const input = parts.length === 1 ? parts[0] : {
    // The pin names the exact tables merged and their span.
    pin: { id: `CDS.${def.parts.join('_')}.rules.${CALENDAR_YEARS[0]}-${CALENDAR_YEARS[1]}`, version: 1 },
    holidays: [...new Set(parts.flatMap((p) => p.holidays))].sort(),
    weekend_days: parts[0].weekend_days,
  };
  const closed = new Set(input.holidays);
  const isBusinessDay = (iso) => !input.weekend_days.includes(weekday(iso)) && !closed.has(iso);
  const cal = {
    code,
    label: def.label,
    input,
    isBusinessDay,
    following(iso) {
      let d = iso;
      while (!isBusinessDay(d)) d = addDays(d, 1);
      return d;
    },
    addBusinessDays(iso, n) {
      let d = iso;
      for (let left = n; left > 0;) {
        d = addDays(d, 1);
        if (isBusinessDay(d)) left -= 1;
      }
      return d;
    },
  };
  calendars.set(code, cal);
  return cal;
}

// ---- standard dates --------------------------------------------------------

const pad = (n) => String(n).padStart(2, '0');

/** Last unadjusted quarterly twentieth (20 Mar/Jun/Sep/Dec) on or before a date. */
export function rollOnOrBefore(iso) {
  const d = parseIso(iso);
  const y = d.getUTCFullYear();
  const quarterEnd = Math.ceil((d.getUTCMonth() + 1) / 3) * 3;
  const candidate = `${y}-${pad(quarterEnd)}-20`;
  return candidate <= iso ? candidate : addMonths(candidate, -3);
}

export function isQuarterlyTwentieth(iso) {
  const d = parseIso(iso);
  return d.getUTCDate() === 20 && (d.getUTCMonth() + 1) % 3 === 0;
}

/**
 * Semi-annual roll (CDS2015): trades from 20 March take June maturities and
 * trades from 20 September take December ones, so the anchor a tenor is added
 * to is the 20 June or 20 December of the current roll period.
 */
export function rollAnchor(tradeDate) {
  const d = parseIso(tradeDate);
  const y = d.getUTCFullYear();
  const md = (d.getUTCMonth() + 1) * 100 + d.getUTCDate();
  if (md >= 320 && md < 920) return `${y}-06-20`;
  return md >= 920 ? `${y}-12-20` : `${y - 1}-12-20`;
}

/** Whole quarters only: a standard maturity must land on a quarterly twentieth. */
export function tenorQuarters(tenor) {
  let months;
  try {
    months = tenorMonths(tenor);
  } catch {
    return null;
  }
  return months > 0 && Number.isInteger(months) && months % 3 === 0 ? months : null;
}

export function standardMaturity(tradeDate, tenor) {
  const months = tenorQuarters(tenor);
  if (months === null) throw new Error(`${tenor} is not a whole number of quarters`);
  return addMonths(rollAnchor(tradeDate), months);
}

/**
 * Standard dates for a trade: step-in T+1 calendar day, cash settlement T+3
 * business days, and accrual from the last quarterly twentieth on or before the
 * trade date, adjusted to a business day (a full first coupon).
 */
export function contractDates(calendarCode, tradeDate) {
  const cal = cdsCalendar(calendarCode);
  const stepIn = addDays(tradeDate, 1);
  const accrualStart = cal.following(rollOnOrBefore(tradeDate));
  return {
    tradeDate,
    stepIn,
    accrualStart,
    protectionStart: accrualStart > stepIn ? accrualStart : stepIn,
    cashSettlement: cal.addBusinessDays(tradeDate, SETTLEMENT_LAG),
    anchor: rollAnchor(tradeDate),
  };
}

export function pricingContext(dates, initialPayment = 'NotContracted') {
  return {
    cash_settlement_date: dates.cashSettlement,
    initial_payment_state: initialPayment,
    step_in_date: dates.stepIn,
    valuation_date: dates.tradeDate,
  };
}

// ---- the credit curves you supply -------------------------------------------

export function slug(text) {
  return String(text).trim().toUpperCase().replace(/[^A-Z0-9]+/g, '.').replace(/^\.+|\.+$/g, '') || 'ENTITY';
}

/** A new credit curve definition, as kept in the store. */
export function creditCurveDef({ entity, currency, seniority = 'SeniorUnsecured', recovery, docClause, calendar,
  points = [], example = false, existing = {} }) {
  const tier = SENIORITIES[seniority].tier;
  const doc = docClause || DEFAULT_DOC[currency];
  const base = `${slug(entity)}.${tier}.${currency}.${doc}`;
  let id = base;
  for (let n = 2; existing[id]; n++) id = `${base}.${n}`;
  return {
    id,
    entity: String(entity).trim(),
    entityId: slug(entity),
    currency,
    seniority,
    recovery: recovery ?? SENIORITIES[seniority].recovery,
    docClause: doc,
    calendar: calendar || DEFAULT_CALENDAR[currency],
    discount: 'auto',
    points,
    example,
    revision: 1,
  };
}

/** Quotes that can be calibrated, in maturity order, or the reason they can't. */
export function creditQuotes(def, tradeDate) {
  const rows = [];
  const problems = [];
  for (const p of def.points) {
    if (p.value === '' || p.value === null || p.value === undefined) continue;
    if (!isDecimal(p.value) || !(Number(p.value) > 0)) {
      problems.push(`${p.tenor}: a par spread must be a positive number of basis points`);
      continue;
    }
    if (tenorQuarters(p.tenor) === null) {
      problems.push(`${p.tenor}: standard CDS tenors are whole quarters (3M, 6M, 1Y, …)`);
      continue;
    }
    rows.push({ tenor: p.tenor, bp: decimal(p.value), maturity: standardMaturity(tradeDate, p.tenor) });
  }
  rows.sort((a, b) => (a.maturity < b.maturity ? -1 : a.maturity > b.maturity ? 1 : 0));
  for (let i = 1; i < rows.length; i++) {
    if (rows[i].maturity === rows[i - 1].maturity) problems.push(`${rows[i - 1].tenor} and ${rows[i].tenor} mature on the same date`);
  }
  const live = rows.filter((r) => r.maturity > addDays(tradeDate, 1));
  if (live.length < rows.length) problems.push('A tenor matures before protection starts');
  return { quotes: live, problems };
}

export function creditReference(def) {
  return {
    document: DOCUMENT_PIN,
    entity_id: def.entityId || slug(def.entity),
    // Percent text to a decimal fraction by moving the point, never by float division.
    recovery_fraction: Number(shift(def.recovery, 2)),
    restructuring_clause: { id: `ISDA.2014.${def.docClause}`, version: 1 },
    seniority: def.seniority,
  };
}

export function survivalKey(def, tradeDate, discountKey) {
  return { id: `CDS.${def.id}`, version: `${tradeDate}.r${def.revision || 1}.${discountKey.id}.${discountKey.version}` };
}

// ---- requests ---------------------------------------------------------------

export function cdsInputs(cal, curves = [], calibration = null) {
  return { calendars: [cal.input], calibration, curves, fixings: [], helper_calendars: [], publications: [] };
}

export const FIT_CONTROLS = { jacobian_step: 1e-6, minimum_singular_ratio: 1e-10, svd_iterations: 1000 };

/** Complete CDS terms. `coupon` and `notional` are exact decimal text. */
export function cdsTerms({ tradeId, side = 'ProtectionBuyer', currency, notional, couponDecimal, reference, maturity,
  dates, cal, discountKey, survival, upfront = null }) {
  return {
    cash_settlement_date: dates.cashSettlement,
    coupon_decimal: couponDecimal,
    currency,
    discount_curve: discountKey,
    initial_clean_upfront_fraction: upfront,
    maturity_rule: 'QuarterlyTwentieth',
    model: 'Standard183',
    notional: decimal(notional),
    pay_accrual_on_default: true,
    premium_calendar: cal.input.pin,
    premium_day_count: 'Act360',
    premium_schedule: {
      end_date: maturity,
      end_of_month: false,
      first_regular_date: null,
      frequency: 'Quarterly',
      penultimate_date: null,
      start_date: dates.accrualStart,
      stub_type: 'ShortFirst',
    },
    protection_start_date: dates.protectionStart,
    reference,
    settlement_calendar: cal.input.pin,
    settlement_lag_business_days: SETTLEMENT_LAG,
    side,
    step_in_date: dates.stepIn,
    survival_curve: survival,
    trade_date: dates.tradeDate,
    trade_id: tradeId,
    version: 1,
  };
}

// Native residual bound for each par-spread quote, in its own unit (bp).
export const QUOTE_TOLERANCE_BP = 1e-7;

/**
 * The native credit recipe: one par CDS per tenor quoted as a running spread
 * in basis points, bootstrapped piecewise (Brent per pillar) to a log-linear
 * survival curve over the supplied discount curve, with no extrapolation.
 */
export function creditRecipe({ def, tradeDate, key, discountKey, quotes, coupon = '0.01' }) {
  const cal = cdsCalendar(def.calendar);
  const dates = contractDates(def.calendar, tradeDate);
  const reference = creditReference(def);
  const helpers = quotes.map((q, i) => {
    const id = `${key.id}.${q.tenor}`;
    return {
      context: pricingContext(dates),
      id,
      pillar: q.maturity,
      quote: { unit: 'BasisPoints', value: Number(q.bp) },
      quote_kind: 'RunningSpread',
      quote_version: q.version || 1,
      terms: cdsTerms({ tradeId: id, currency: def.currency, notional: '10000000', couponDecimal: coupon, reference,
        maturity: q.maturity, dates, cal, discountKey, survival: key }),
      tolerance: QUOTE_TOLERANCE_BP,
      _index: i,
    };
  });
  return {
    bootstrap: { lower_hazard: 0, max_iterations: 100, upper_hazard: 5 },
    curve: {
      calibration: 'Piecewise',
      currency: def.currency,
      dependencies: [discountKey],
      extrapolation: 'None',
      interpolation: 'LogLinear',
      key,
      reference_date: tradeDate,
      representation: 'SurvivalProbabilities',
      role: { entity_id: reference.entity_id, kind: 'Credit', recovery: reference.recovery_fraction, seniority: SENIORITIES[def.seniority].role },
      source: { Helpers: helpers.map((h) => h.id) },
      time_basis: 'Act365Fixed',
    },
    helpers: helpers.map(({ _index, ...h }) => h),
    pillars: helpers.map((h) => h.pillar),
  };
}

export function creditCalibration(cal, discount, recipe) {
  return {
    schema_version: SCHEMA_VERSION,
    inputs: cdsInputs(cal, [discount]),
    operation: {
      kind: 'calibration',
      action: { kind: 'build' },
      input: { cash: [], controls: FIT_CONTROLS, credit: [recipe], rate_context: null, rates: [] },
    },
  };
}

/** Collateral discount curve from dated discount factors, in the form the CDS pricer admits. */
export function discountFactorCurve({ key, currency, referenceDate, nodes }) {
  return {
    calibration: 'Direct',
    currency,
    dependencies: [],
    extrapolation: 'None',
    interpolation: 'LogLinear',
    key,
    reference_date: referenceDate,
    representation: 'DiscountFactors',
    role: { collateral_currency: currency, kind: 'Discount' },
    source: { Nodes: nodes.map(([date, df]) => ({ pillar: { Date: date }, quote: { unit: 'DiscountFactor', value: df } })) },
    time_basis: 'Act365Fixed',
  };
}

/** The fitted survival grid re-supplied as direct nodes, so later requests reuse it exactly. */
export function survivalCurve(recipe, nodes) {
  const c = recipe.curve;
  return {
    calibration: 'Direct',
    currency: c.currency,
    dependencies: [],
    extrapolation: 'None',
    interpolation: 'LogLinear',
    key: c.key,
    reference_date: c.reference_date,
    representation: 'SurvivalProbabilities',
    role: c.role,
    source: { Nodes: nodes.map(([point, q]) => ({ pillar: point, quote: { unit: 'SurvivalProbability', value: q } })) },
    time_basis: 'Act365Fixed',
  };
}

export function survivalQuery(cal, curves, key, query) {
  return { schema_version: SCHEMA_VERSION, inputs: cdsInputs(cal, curves), operation: { kind: 'curve_query', curve: key, query } };
}

export const survivalAt = (date) => ({ kind: 'SurvivalProbability', point: { Date: date } });
export const defaultBetween = (start, end, conditional = false) => ({
  kind: 'DefaultProbability', start: { Date: start }, end: { Date: end }, conditional,
});
/** Hazard on the interval ending at the date (`Left`) or starting there (`Right`). */
export const hazardAt = (date, side = 'Left') => ({ kind: 'HazardRate', point: { Date: date }, side });

export function cdsRequest(cal, curves, context, terms, action) {
  return { schema_version: SCHEMA_VERSION, inputs: cdsInputs(cal, curves), operation: { kind: 'cds', action, context, terms } };
}

export const cdsPrice = (cal, curves, context, terms) => cdsRequest(cal, curves, context, terms, { kind: 'price' });
export const cdsConstruct = (cal, curves, context, terms) => cdsRequest(cal, curves, context, terms, { kind: 'construct' });

export const CONVERSION_SETTINGS = { lower_hazard: 0, upper_hazard: 5, native_quote_tolerance: 1e-12, max_iterations: 100 };

/** Running spread ↔ clean/dirty upfront on one flat hazard (the conventional quote). */
export function cdsConvert(cal, curves, context, terms, quote) {
  return cdsRequest(cal, curves, context, terms, { kind: 'convert', quote, settings: CONVERSION_SETTINGS });
}

export const runningSpread = (decimalRate) => ({ kind: 'running_spread', decimal_rate: decimalRate });
export const cleanUpfront = (fraction) => ({ kind: 'clean_upfront', fraction });

// ---- credit risk --------------------------------------------------------------

export function creditRiskInput({ recipe, terms, context }) {
  return { context, controls: FIT_CONTROLS, recipe, risk_model: RISK_MODEL, terms };
}

function riskRequest(cal, discount, input, action) {
  return { schema_version: SCHEMA_VERSION, inputs: cdsInputs(cal, [discount]), operation: { kind: 'credit_risk', input, action } };
}

function revisions(key, tag, discountKey = null) {
  return {
    plus_id: `${tag}.plus`,
    minus_id: `${tag}.minus`,
    plus_credit: { id: key.id, version: `${key.version}.${tag}.plus` },
    minus_credit: { id: key.id, version: `${key.version}.${tag}.minus` },
    plus_discount: discountKey && { id: discountKey.id, version: `${discountKey.version}.${tag}.plus` },
    minus_discount: discountKey && { id: discountKey.id, version: `${discountKey.version}.${tag}.minus` },
  };
}

function factor(input, fields) {
  return {
    convention_version: input.terms.reference.document.version,
    currency: input.terms.currency,
    model_version: input.risk_model,
    ...fields,
  };
}

/** Signed CS01 for one par-spread quote: central ±`stepBp`, full recalibration each side. */
export function quoteCs01(cal, discount, input, helper, stepBp = 1) {
  const key = input.recipe.curve.key;
  return riskRequest(cal, discount, input, {
    kind: 'sensitivity',
    bump: { RunningQuote: { helper_id: helper.id, step_bps: stepBp } },
    factor: factor(input, {
      basis: 'market_quote_recalibrated',
      family: 'credit_spread',
      quote_type: 'cds_spread',
      target: { kind: 'curve', curve_id: key.id },
      // The factor's bucket is the helper's actual dated tenor from the curve date.
      tenor: `${daysBetween(input.recipe.curve.reference_date, helper.pillar)}D`,
    }),
    revisions: revisions(key, `cs01.${helper.id}`),
  });
}

/** Recovery derivative per unit fraction; `response` holds survival or refits it at unchanged quotes. */
export function recoveryRisk(cal, discount, input, response = 'RecalibrateQuotes', step = 0.01) {
  return riskRequest(cal, discount, input, {
    kind: 'sensitivity',
    bump: { Recovery: { step_fraction: step, credit_response: response } },
    factor: factor(input, {
      basis: 'model_parameter',
      family: 'recovery',
      quote_type: 'recovery_rate',
      target: { kind: 'entity', entity_id: input.terms.reference.entity_id },
      tenor: 'parallel',
    }),
    revisions: revisions(input.recipe.curve.key, `recovery.${response}`),
  });
}

/** Parallel discount-curve PV01 (continuous zero shift), credit refitted at unchanged quotes. */
export function discountRisk(cal, discount, input, step = 0.0001) {
  return riskRequest(cal, discount, input, {
    kind: 'sensitivity',
    bump: { DiscountZero: { step_decimal: step, credit_response: 'RecalibrateQuotes' } },
    factor: factor(input, {
      basis: 'zero_node',
      family: 'interest_rate',
      quote_type: 'zero_rate',
      target: { kind: 'curve', curve_id: input.terms.discount_curve.id },
      tenor: 'parallel',
    }),
    revisions: revisions(input.recipe.curve.key, 'ir01', discount.key),
  });
}

/** Every par-spread quote moved by `deltaBp` together, then the curve rebuilt and the trade repriced. */
export function quoteScenario(cal, discount, input, deltaBp) {
  const key = input.recipe.curve.key;
  const tag = `parallel.${deltaBp > 0 ? 'up' : 'down'}${Math.abs(deltaBp)}`;
  return riskRequest(cal, discount, input, {
    kind: 'scenario',
    request: {
      id: tag,
      output_credit: { id: key.id, version: `${key.version}.${tag}` },
      shock: {
        Quotes: {
          moves: input.recipe.helpers.map((h) => ({
            helper_id: h.id,
            expected_quote_version: h.quote_version,
            delta: { unit: h.quote.unit, value: deltaBp },
          })),
        },
      },
    },
  });
}
