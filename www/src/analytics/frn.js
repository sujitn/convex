// Floating-rate note analysis: price and discount margin from each other, the
// projected coupons with each rate's origin, and risk by re-running the engine.
// Each section keeps its own native error; nothing is substituted for a
// missing figure.

import * as F from '../lib/frn.js';
import { parsePrice } from '../lib/decimal.js';
import { compareIso } from '../lib/dates.js';

export const DM_LADDER_BP = [-50, -25, -10, -5, 0, 5, 10, 25, 50];

const ORIGIN = { FutureObservation: 'projected', AwaitingPublication: 'awaiting' };

/** One overnight observation: its dates, rate and where the rate came from. */
function observationRow(o) {
  const published = o.origin && o.origin.Published;
  return {
    date: o.dates.observation_date,
    weightStart: o.dates.weight_start,
    weightEnd: o.dates.weight_end,
    rate: o.rate,
    yearFraction: o.year_fraction,
    origin: published ? 'published' : ORIGIN[o.origin && o.origin.Projected] || 'projected',
    publishedAt: published ? published.published_at : null,
  };
}

/** Coupon rows from the engine's projected cash, each with its rate origin. */
export function couponRows(cash) {
  return (cash.coupons || []).map((c) => {
    const obs = c.overnight ? c.overnight.observations.map(observationRow) : [];
    const published = obs.filter((o) => o.origin === 'published').length;
    return {
      accrualStart: c.accrual_start,
      accrualEnd: c.accrual_end,
      paymentDate: c.payment_date,
      amount: c.amount,
      accrued: c.accrued,
      entitled: c.entitled,
      rate: c.overnight ? c.overnight.annualized_rate : null,
      periodFactor: c.overnight ? c.overnight.period_factor : null,
      observations: obs,
      published,
      projected: obs.length - published,
      origin: !obs.length ? 'unknown' : published === obs.length ? 'fixed' : published ? 'partial' : 'projected',
    };
  });
}

/** "N fixings due but not supplied, first … last" from a native missing manifest. */
export function missingFixings(error) {
  const list = ((error && error.details && error.details.missing) || []).map((m) => m && m.Fixing).filter(Boolean);
  if (!list.length) return null;
  const dates = list.map((f) => f.observation_date).sort(compareIso);
  return { count: dates.length, first: dates[0], last: dates[dates.length - 1], index: list[0].index_id };
}

function fail(out, error) {
  return { ...out, fatal: error, missing: missingFixings(error) };
}

/**
 * Full FRN analysis for one quote. `quote` = { kind: 'price' | 'dm', text },
 * `history` the index's fixing dataset, `curve` a built Discount-role curve.
 */
export async function analyseFrn(engine, { sec: security, tradeDate, settlement, curve, history, quote, face }) {
  // Straight figures are conditional on no exercise, so they run with the
  // calls removed and say so; the workout table prices every legal path.
  const calls = (security.calls || []).filter((c) => c && c.date && c.price);
  const callable = calls.length > 0;
  const sec = callable ? { ...security, calls: [] } : security;
  const label = sec.ticker || sec.name;
  const out = { errors: {}, callsIgnored: callable };

  if (!curve || !curve.recipe) return fail(out, { code: 'missing_input', message: 'No SOFR discount curve is built.' });

  const built = await engine.call(F.frnConstruct(sec, settlement), { label: `${label} periods` });
  if (!built.ok) return fail(out, built.error);
  const periods = built.data.periods;
  out.periods = periods;

  let market;
  try {
    market = F.frnMarket({ sec, tradeDate, settlement, curve, history, periods });
  } catch (e) {
    return fail(out, { code: e.code || 'invalid_input', message: e.message });
  }
  out.market = {
    knownAt: market.knownAt,
    cutoffs: market.cutoffs,
    fixings: market.fixings.length,
    publications: market.publications.length,
    discountKey: market.discountKey,
    projectionKey: market.projectionKey,
  };

  // 1. The quote as a clean price.
  let clean;
  if (quote.kind === 'price') {
    try {
      clean = parsePrice(quote.text);
    } catch {
      clean = NaN;
    }
  } else {
    const res = await engine.call(F.frnPrice(sec, market, Number(quote.text) / 10000), { label: `${label} price from DM` });
    if (!res.ok) return fail(out, res.error);
    clean = res.data.clean_per100;
    out.fromDm = res.data;
  }
  if (!Number.isFinite(clean)) return fail(out, { code: 'invalid_input', message: `"${quote.text}" is not a usable quote.` });
  out.quotedClean = clean;

  // 2. Discount margin from the clean price, with the cash it was solved on.
  const inv = await engine.call(F.frnDiscountMargin(sec, market, { kind: 'clean', value: clean }), { label: `${label} discount margin` });
  if (!inv.ok) return fail(out, inv.error);
  const v = inv.data.valuation;
  const dm = inv.data.decimal_parameter;
  out.dm = dm;
  out.clean = v.clean_per100;
  out.dirty = v.dirty_per100;
  out.accrued = v.accrued_per100;
  out.presentValue = v.present_value;
  out.inverse = { iterations: inv.data.iterations, residual: inv.data.native_residual, evaluations: inv.data.price_evaluations,
    target: inv.data.target };
  out.cash = v.cash;
  out.coupons = couponRows(v.cash);
  out.principal = (v.cash.principal || []).map((p) => ({ date: p.payment_date || p.date, amount: p.amount }));
  out.current = out.coupons.find((c) => compareIso(c.accrualStart, settlement) < 0 && compareIso(settlement, c.accrualEnd) < 0)
    || out.coupons[0] || null;

  // 3. Risk: DM ± 1bp with coupons frozen, and the curve ± 1bp in parallel
  // (projection and discounting move together, coupons re-projected), all at
  // the solved DM. Plus a price ladder in DM.
  const h = 1e-4;
  const shifted = [-1, 1].map((bp) => F.withCurve(market, F.shiftedCurve(curve, bp), sec.index));
  const reqs = [
    F.frnPrice(sec, market, dm - h),
    F.frnPrice(sec, market, dm + h),
    F.frnPrice(sec, shifted[0], dm),
    F.frnPrice(sec, shifted[1], dm),
    ...DM_LADDER_BP.map((b) => F.frnPrice(sec, market, dm + b / 10000)),
  ];
  const priced = await engine.callMany(reqs, { label: `${label} DM and curve bumps` });
  const dirty = (r) => (r.ok ? r.data.dirty_per100 : NaN);
  const scale = Number(face) / Number(sec.face || 100);
  const [dmDown, dmUp, cvDown, cvUp] = priced.slice(0, 4);
  if (dmDown.ok && dmUp.ok) {
    const pm = dirty(dmDown);
    const pp = dirty(dmUp);
    out.dmRisk = {
      down: pm, up: pp,
      dm01Per100: (pp - pm) / 2,
      dm01: ((pp - pm) / 2) * scale,
      duration: (pm - pp) / (2 * h * out.dirty),
    };
  } else out.errors.dmRisk = (dmDown.ok ? dmUp : dmDown).error;
  if (cvDown.ok && cvUp.ok) {
    const pm = dirty(cvDown);
    const pp = dirty(cvUp);
    out.curveRisk = {
      down: pm, up: pp,
      ir01Per100: (pp - pm) / 2,
      ir01: ((pp - pm) / 2) * scale,
      duration: (pm - pp) / (2 * h * out.dirty),
      nextCoupon: { down: cvDown.data.cash?.coupons?.[0]?.amount, up: cvUp.data.cash?.coupons?.[0]?.amount },
    };
  } else out.errors.curveRisk = (cvDown.ok ? cvUp : cvDown).error;
  const ladder = priced.slice(4);
  if (ladder.every((r) => r.ok)) {
    out.ladder = DM_LADDER_BP.map((b, i) => ({ bp: b, dm: dm + b / 10000, clean: ladder[i].data.clean_per100,
      dirty: ladder[i].data.dirty_per100, change: ladder[i].data.dirty_per100 - out.dirty }));
  } else out.errors.ladder = ladder.find((r) => !r.ok).error;

  // 4. Workouts: the discount margin to each legal redemption path.
  if (callable) {
    const paths = F.workoutPathList(security);
    const res = await engine.callMany(paths.map((path) => F.frnWorkoutDm(security, market, {
      tradeDate, settlement, face, target: { kind: 'clean', value: clean },
      selection: F.workoutSelection(security, periods, path.kind === 'maturity' ? { kind: 'maturity' }
        : { kind: 'exercise', provision: path.provision, date: path.date }),
    })), { label: `${label} workout DMs` });
    out.workouts = paths.map((path, i) => {
      const r = res[i];
      if (!r.ok) return { ...path, error: r.error };
      const q = r.data.quotation.quote;
      const v = q.quotation.value;
      return { ...path, dm: v.decimal_parameter, residual: v.native_residual, iterations: v.iterations,
        exercise: q.exercise || null, consideration: q.consideration, accrued: v.valuation.accrued_per100 };
    });
  }
  return out;
}
