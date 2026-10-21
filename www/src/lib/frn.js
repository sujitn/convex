// SOFR floating-rate notes: compounded-in-arrears coupons as full `BondTerms`,
// and the market resources one request carries for them: the overnight
// projection view of a discount curve you supplied, the published SOFR
// fixings, the expected publication of every observation the remaining
// coupons need, and the accrued-interest cutoff the observation shift asks for.

import { bondTerms, calendarPins, securityId } from './terms.js';
import { addBusinessDays, adjustFollowing, isBusinessDay, previousBusinessDay } from './calendars.js';
import { addMonths, compareIso } from './dates.js';
import { decimal, bpToRate, percentToRate, isDecimal } from './decimal.js';
import { OIS_INDICES, oisPublications, zonedTimestamp, MAX_PUBLICATIONS } from './ois.js';
import { request, inputs, bondContext, curveKey, collateralRole, SPREAD_INVERSE_SETTINGS } from './requests.js';
import { workoutContext } from './callable.js';

export const SOFR = 'USD.SOFR';

// Floating indices this screen can price: the overnight index, its
// administrator's publication calendar and schedule, and the history dataset.
export const FRN_INDICES = {
  [SOFR]: { ...OIS_INDICES[SOFR], index: SOFR, dataset: 'sofr-history', sourceId: 'NYFED.SOFR', administrator: 'Federal Reserve Bank of New York' },
};

// Conventions a new SOFR FRN starts from: the ARRC-style compounded SOFR with a
// two-day observation shift, quarterly Act/360, Modified Following periods.
export const FRN_DEFAULTS = {
  index: SOFR,
  frequency: 'Quarterly',
  dayCount: 'Act360',
  lookbackDays: 2,
  observationShift: true,
  lockoutDays: 0,
  paymentDelayDays: 0,
  accrualAdjustment: 'ModifiedFollowing',
  paymentAdjustment: 'ModifiedFollowing',
  calendar: 'US.SIFMA',
  settlementDays: 1,
  currency: 'USD',
  face: '100',
  redemption: '100',
  eom: false,
};

const whole = (v, d = 0) => (Number.isInteger(Number(v)) && Number(v) >= 0 ? Number(v) : d);

/** Margin over the index as an exact decimal rate: "85" bp → "0.0085". */
export function marginRate(sec) {
  if (!isDecimal(sec.spreadBp ?? '')) throw new Error(`Spread "${sec.spreadBp ?? ''}" is not a number of basis points.`);
  return bpToRate(sec.spreadBp);
}

/**
 * Full `BondTerms` for an overnight-compounded FRN: the fixed-bond contract
 * (schedule, calls, principal, redemption) with the coupon replaced by daily
 * compounded SOFR, the margin as a simple spread outside the compounding, and
 * the period adjustment and payment delay stated explicitly.
 */
/** A date moved to a business day under a period adjustment convention. */
export function adjustDate(code, iso, convention) {
  if (convention === 'Unadjusted' || isBusinessDay(code, iso)) return iso;
  const next = adjustFollowing(code, iso);
  if (convention === 'Following' || next.slice(0, 7) === iso.slice(0, 7)) return next;
  return previousBusinessDay(code, iso);
}

/** Profile segments re-anchored so they cover exactly [start, end). */
function cover(segments, start, end) {
  return segments.map((p, i) => ({ ...p, start: i === 0 ? start : p.start, end: i === segments.length - 1 ? end : p.end }));
}

export function floatingTerms(sec) {
  const base = bondTerms({ ...sec, coupon: '0', steps: [] });
  const spread = marginRate(sec);
  // With adjusted periods the engine accrues from the adjusted start to the
  // adjusted end, and every profile must cover exactly that span.
  const adjustment = sec.accrualAdjustment || 'ModifiedFollowing';
  const start = adjustDate(sec.calendar, base.schedule.start_date, adjustment);
  const end = adjustDate(sec.calendar, base.schedule.end_date, adjustment);
  const span = (segments) => cover(segments, start, end);
  return {
    ...base,
    coupon: {
      kind: 'overnight',
      index_id: sec.index || SOFR,
      convention: {
        lookback_business_days: whole(sec.lookbackDays, 2),
        observation_shift: sec.observationShift !== false,
        lockout_business_days: whole(sec.lockoutDays, 0),
      },
    },
    dates: {
      ...base.dates,
      accrual_adjustment: adjustment,
      payment_adjustment: sec.paymentAdjustment || 'ModifiedFollowing',
      payment_delay: { BusinessDays: whole(sec.paymentDelayDays, 0) },
    },
    profiles: {
      coupon: span(base.profiles.coupon),
      notional: span(base.profiles.notional),
      gearing: span(base.profiles.gearing.map((p) => ({ ...p, value: '1' }))),
      spread: span(base.profiles.spread.map((p) => ({ ...p, value: spread }))),
    },
  };
}

export function frnBondInput(sec) {
  return { calendars: calendarPins(sec.calendar), terms: floatingTerms(sec) };
}

/** The contract and its engine-generated periods; needs calendars only. */
export function frnConstruct(sec, settlement) {
  return request(inputs({ calendars: [sec.calendar] }), {
    kind: 'bond', bond: frnBondInput(sec), context: bondContext(settlement), projection: null, action: { kind: 'construct' },
  });
}

// ---- curves ------------------------------------------------------------------

/**
 * The zero-basis overnight projection view of a discount curve, in the form the
 * OIS calibration itself declares it: role Projection for the index, sourced
 * from the exact discount revision.
 */
export function projectionView(discount, index = SOFR) {
  const key = curveKey(`${discount.key.id}.ON`, discount.key.version);
  return {
    ...discount,
    key,
    dependencies: [discount.key],
    role: { kind: 'Projection', index_id: index },
    source: { OvernightProjection: { discount: discount.key, index_id: index } },
  };
}

/** Discount recipe plus its projection view, for a built curve `{ key, recipe }`. */
export function frnCurves(curve, index = SOFR) {
  const projection = projectionView(curve.recipe, index);
  return { curves: [curve.recipe, projection], discountKey: curve.recipe.key, projectionKey: projection.key };
}

/**
 * The same fitted zero curve moved in parallel by `bp` basis points
 * (continuous zero rate, every node), under its own version so neither
 * revision can stand in for the other.
 */
export function shiftedCurve(curve, bp) {
  const tag = `${bp > 0 ? 'up' : 'dn'}${Math.abs(bp)}bp`;
  const key = curveKey(curve.recipe.key.id, `${curve.recipe.key.version}.${tag}`);
  const nodes = curve.recipe.source.Nodes.map((n) => ({ ...n, quote: { ...n.quote, value: n.quote.value + bp / 10000 } }));
  return { key, recipe: { ...curve.recipe, key, source: { Nodes: nodes } } };
}

// ---- fixings, publications, context -----------------------------------------------

/** Next business day at the administrator's local publication time, as UTC. */
export function publicationInstant(idx, observationDate) {
  return zonedTimestamp(addBusinessDays(idx.calendar, observationDate, 1), idx.publication.time, idx.publication.zone);
}

/**
 * Published fixings from a history dataset (rates in percent text) as engine
 * observations. Each is taken as published, and known, at the scheduled
 * publication time; a date edited twice keeps the later value.
 */
export function historyFixings(dataset, index = SOFR) {
  const idx = FRN_INDICES[index];
  const byDate = new Map();
  for (const p of (dataset && dataset.points) || []) {
    if (!p || !p.date || p.value === '' || p.value === null || p.value === undefined || !isDecimal(p.value)) continue;
    byDate.set(p.date, p);
  }
  return [...byDate.values()]
    .sort((a, b) => compareIso(a.date, b.date))
    .map((p) => {
      const at = publicationInstant(idx, p.date);
      return {
        index_id: index,
        observation_date: p.date,
        rate: percentToRate(p.value),
        published_at: at,
        known_at: at,
        revision: 1,
        status: 'Final',
        source_id: idx.sourceId,
      };
    });
}

/**
 * The information cutoff: the end of the trade date. A fixing published by
 * then must be in the history (else it is reported missing, never guessed);
 * the trade date's own fixing, published the next morning, is forecast as
 * awaiting publication.
 */
export function informationCutoff(tradeDate) {
  return `${tradeDate}T23:59:59Z`;
}

/** Coupons still owed at settlement (payment strictly after it), oldest first. */
export function activePeriods(periods, settlement) {
  return periods.filter((p) => compareIso(p.payment_date, settlement) > 0);
}

/** The coupon period settlement falls strictly inside, if any. */
export function currentPeriod(periods, settlement) {
  return periods.find((p) => compareIso(p.accrual_start, settlement) < 0 && compareIso(settlement, p.accrual_end) < 0) || null;
}

/**
 * Under an observation shift, accrued for a part period compounds the shifted
 * observation weights up to an explicit cutoff. The convention used here: the
 * settlement date shifted back by the lookback, i.e. the observation day that
 * pairs with settlement. Only the period settlement falls inside gets one.
 */
export function accrualCutoffs(sec, periods, settlement) {
  if (sec.observationShift === false) return [];
  const p = currentPeriod(periods, settlement);
  if (!p) return [];
  const idx = FRN_INDICES[sec.index || SOFR];
  return [{ period_start: p.accrual_start, weight_end: addBusinessDays(idx.calendar, settlement, -whole(sec.lookbackDays, 2)) }];
}

/**
 * Expected publications for exactly the overnight observations the unpaid
 * coupons compound: each business day in an accrual period, moved back by the
 * lookback. The first is the earliest period's first business day shifted;
 * the last is the day before the final accrual end, shifted.
 */
export function frnPublications(sec, periods, settlement) {
  const idx = FRN_INDICES[sec.index || SOFR];
  const owed = activePeriods(periods, settlement);
  if (!owed.length) return [];
  const lookback = whole(sec.lookbackDays, 2);
  const from = addBusinessDays(idx.calendar, adjustFollowing(idx.calendar, owed[0].accrual_start), -lookback);
  const to = addBusinessDays(idx.calendar, adjustFollowing(idx.calendar, owed[owed.length - 1].accrual_end), -1 - lookback);
  const out = oisPublications(idx, from, to);
  if (out.length > MAX_PUBLICATIONS) {
    const err = new Error(`The remaining coupons need ${out.length.toLocaleString()} daily ${idx.label} publication records; `
      + `one request carries at most ${MAX_PUBLICATIONS.toLocaleString()}, about ${Math.floor(MAX_PUBLICATIONS / 252)} years of observations.`);
    err.code = 'input_limit';
    throw err;
  }
  return out;
}

/**
 * Everything the pricing requests share: curves, fixings, publications and the
 * projection context. The fixing valuation date is the trade date (the
 * information date, which the curve date must match); settlement owns the cash.
 */
export function frnMarket({ sec, tradeDate, settlement, curve, history, periods }) {
  const index = sec.index || SOFR;
  const { curves, discountKey, projectionKey } = frnCurves(curve, index);
  const knownAt = informationCutoff(tradeDate);
  const cutoffs = accrualCutoffs(sec, periods, settlement);
  return {
    curves,
    discountKey,
    projectionKey,
    fixings: historyFixings(history, index),
    publications: frnPublications(sec, periods, settlement),
    knownAt,
    cutoffs,
    context: {
      fixing: { final_only: true, known_at: knownAt, unpublished: 'Forecast', valuation_date: tradeDate },
      on_payment: 'Exclude',
      overnight_accrual_cutoffs: cutoffs,
      settlement,
    },
  };
}

/** The same market on another curve revision (a shifted curve). */
export function withCurve(market, curve, index = SOFR) {
  return { ...market, ...frnCurves(curve, index) };
}

function marketInputs(sec, market) {
  return { ...inputs({ calendars: [sec.calendar], curves: market.curves, fixings: market.fixings }), publications: market.publications };
}

function frnRequest(sec, market, action) {
  return request(marketInputs(sec, market), {
    kind: 'bond', bond: frnBondInput(sec), context: market.context, projection: market.projectionKey, action,
  });
}

export const dmMethod = (market, decimalDm) => ({
  kind: 'discount_spread', curve: market.discountKey, role: collateralRole(), decimal_spread: decimalDm,
});

/** Price at a discount margin (annual decimal, Act/365F continuous over the collateral curve). */
export const frnPrice = (sec, market, decimalDm) => frnRequest(sec, market, { kind: 'price', method: dmMethod(market, decimalDm) });

/** Discount margin from a clean or dirty price per 100, projected coupons frozen. */
export const frnDiscountMargin = (sec, market, target) => frnRequest(sec, market, {
  kind: 'spread_inverse', target, curve: market.discountKey, role: collateralRole(), settings: SPREAD_INVERSE_SETTINGS,
});

/** Fixing requirements: published, missing and projected observations. */
export const frnRequirements = (sec, market) => frnRequest(sec, market, { kind: 'requirements' });

// ---- workouts (callable / puttable FRNs) -------------------------------------------

/**
 * One legal redemption path and its partial-coupon cutoff: an exercise inside
 * a coupon period under an observation shift stops the shifted weights at the
 * exercise date less the lookback, the same rule as accrued.
 */
export function workoutSelection(sec, periods, path) {
  if (path.kind !== 'exercise' || sec.observationShift === false) return { path, weight_end: null };
  const inside = periods.some((p) => compareIso(p.accrual_start, path.date) < 0 && compareIso(path.date, p.accrual_end) < 0);
  const idx = FRN_INDICES[sec.index || SOFR];
  return { path, weight_end: inside ? addBusinessDays(idx.calendar, path.date, -whole(sec.lookbackDays, 2)) : null };
}

/** Maturity plus every exercise date in the terms, in provision order. */
export function workoutPathList(sec) {
  const terms = floatingTerms(sec);
  const out = [{ kind: 'maturity' }];
  terms.exercise.forEach((ex, provision) => {
    const dates = ex.style.style === 'european' ? [ex.style.date] : ex.style.dates || [];
    for (const date of dates) out.push({ kind: 'exercise', provision, date, right: ex.right, price: ex.price_per_100 });
  });
  return out;
}

/** Discount margin to one legal path from a clean or dirty price. */
export function frnWorkoutDm(sec, market, { tradeDate, settlement, target, selection, face }) {
  const id = securityId(sec);
  return request(marketInputs(sec, market), {
    kind: 'floating_workout',
    input: {
      security: { bond: frnBondInput(sec), quote_revision: { id: `${id}.QUOTE`, version: 1 }, reference: { id, version: 1 } },
      projection_context: market.context,
      projection: market.projectionKey,
      discount: market.discountKey,
      position: { quantity: decimal(face), reference_quantity: decimal(sec.face || '100'), quantity_unit: 'notional', currency: sec.currency },
      quotation_policy: { id: 'convex-web.frn.policy', version: 1 },
    },
    context: workoutContext(tradeDate, settlement),
    action: { kind: 'discount_margin', selection, target, settings: SPREAD_INVERSE_SETTINGS },
  });
}

// ---- securities ----------------------------------------------------------------------

/** Ticker in the street form: "EXBK FRN 08/15/29". */
export function frnTicker(short, maturity) {
  return `${String(short || 'FRN').toUpperCase().slice(0, 10)} FRN ${maturity.slice(5, 7)}/${maturity.slice(8, 10)}/${maturity.slice(2, 4)}`;
}

/** A new FRN security from the terms form (or an import row), with the stated defaults. */
export function frnSecurity(fields) {
  const { short, ...s } = { ...FRN_DEFAULTS, ...fields };
  const margin = decimal(s.spreadBp);
  return {
    ...s,
    kind: 'floating',
    spreadBp: margin,
    datedDate: s.datedDate || s.issueDate,
    firstCouponDate: s.firstCouponDate || null,
    name: s.name || `${FRN_INDICES[s.index].label} + ${margin} FRN ${s.maturity.slice(0, 4)}`,
    ticker: s.ticker || frnTicker(short, s.maturity),
  };
}

/** Issue date for a regular FRN without one: the last coupon date on or before `date`. */
export function lastCouponOnOrBefore(maturity, frequency, date) {
  const months = { Annual: 12, SemiAnnual: 6, Quarterly: 3, Monthly: 1 }[frequency] || 3;
  let d = maturity;
  for (let k = 1; compareIso(d, date) > 0 && k < 2000; k++) d = addMonths(maturity, -k * months);
  return d;
}

export function frnLabel(sec) {
  const idx = FRN_INDICES[sec.index || SOFR];
  return `${idx ? idx.label : sec.index} + ${sec.spreadBp} bp`;
}

