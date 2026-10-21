// Overnight-indexed swap curves from par swap rates you supply. Each tenor is
// one spot-starting par OIS (fixed vs compounded overnight, same schedule),
// fitted by the native rate calibration as a Discount curve whose zero-basis
// overnight projection is declared explicitly.

import { calendarPin, addBusinessDays, adjustFollowing } from './calendars.js';
import { addTenor } from './dates.js';
import { percentToRate } from './decimal.js';
import { request, inputs } from './requests.js';

// Market conventions used for the standard spot-starting swaps. They are the
// defaults the editor shows; every one can be changed per curve. Each rate is
// published on the next business day at the administrator's local time:
// SOFR 08:00 New York, €STR 08:00 CET, SONIA 09:00 London, and CORRA between
// 09:00 and 11:00 Toronto (the end of that window is used).
export const OIS_INDICES = {
  'USD.SOFR': { label: 'SOFR', currency: 'USD', calendar: 'US.SIFMA', spotLag: 2, paymentDelay: 2, dayCount: 'Act360',
    publication: { time: '08:00', zone: 'America/New_York' } },
  'EUR.ESTR': { label: '€STR', currency: 'EUR', calendar: 'EU.TARGET2', spotLag: 2, paymentDelay: 1, dayCount: 'Act360',
    publication: { time: '08:00', zone: 'Europe/Berlin' } },
  'GBP.SONIA': { label: 'SONIA', currency: 'GBP', calendar: 'GB.LON', spotLag: 0, paymentDelay: 0, dayCount: 'Act365Fixed',
    publication: { time: '09:00', zone: 'Europe/London' } },
  'CAD.CORRA': { label: 'CORRA', currency: 'CAD', calendar: 'CA.TOR', spotLag: 1, paymentDelay: 1, dayCount: 'Act365Fixed',
    publication: { time: '11:00', zone: 'America/Toronto' } },
};

// Illustrative swap rates so the OIS route can be shown end to end. They are
// not market data; a curve built from them is marked as an example.
export const EXAMPLE_OIS = {
  'USD.SOFR': '1W 3.92, 1M 3.90, 3M 3.86, 6M 3.80, 1Y 3.70, 2Y 3.58, 3Y 3.52, 5Y 3.50, 7Y 3.54, 10Y 3.62, 12Y 3.68, 15Y 3.75',
  'EUR.ESTR': '1W 1.93, 1M 1.92, 3M 1.90, 6M 1.88, 1Y 1.86, 2Y 1.90, 3Y 1.98, 5Y 2.12, 7Y 2.25, 10Y 2.42, 12Y 2.50, 15Y 2.58',
  'GBP.SONIA': '1W 3.70, 1M 3.68, 3M 3.62, 6M 3.55, 1Y 3.45, 2Y 3.40, 3Y 3.42, 5Y 3.55, 7Y 3.68, 10Y 3.85, 12Y 3.95, 15Y 4.05',
  'CAD.CORRA': '1W 2.25, 1M 2.25, 3M 2.24, 6M 2.22, 1Y 2.20, 2Y 2.30, 3Y 2.42, 5Y 2.62, 7Y 2.80, 10Y 3.00, 12Y 3.08, 15Y 3.18',
};

/** Example rates as points. */
export function examplePoints(index) {
  return EXAMPLE_OIS[index].split(', ').map((cell) => {
    const [tenor, value] = cell.split(' ');
    return { tenor, value };
  });
}

// One request carries at most 4,096 items per array, and the calibration needs
// one publication record per overnight observation date, so a single fit
// reaches about 15 years. Longer points can be supplied as zero rates. The
// native swap fit also takes at most 128 helpers (its fit dimension).
export const MAX_PUBLICATIONS = 4000;
export const MAX_OIS_YEARS = 15;
export const MAX_OIS_QUOTES = 128;

const zoneFormats = new Map();

// The wall-clock reading of instant `ms` in `zone`, as if it were UTC.
function wallClock(ms, zone) {
  let f = zoneFormats.get(zone);
  if (!f) {
    f = new Intl.DateTimeFormat('en-US', { timeZone: zone, hourCycle: 'h23', year: 'numeric', month: '2-digit',
      day: '2-digit', hour: '2-digit', minute: '2-digit', second: '2-digit' });
    zoneFormats.set(zone, f);
  }
  const p = Object.fromEntries(f.formatToParts(new Date(ms)).map((x) => [x.type, x.value]));
  return Date.UTC(Number(p.year), Number(p.month) - 1, Number(p.day), Number(p.hour) % 24, Number(p.minute), Number(p.second));
}

/** Local `time` ("08:00") on ISO `date` in IANA `zone` → UTC timestamp text ("…T12:00:00Z"). */
export function zonedTimestamp(date, time, zone) {
  const [y, m, d] = date.split('-').map(Number);
  const [hh, mm] = time.split(':').map(Number);
  const wall = Date.UTC(y, m - 1, d, hh, mm);
  let at = wall - (wallClock(wall, zone) - wall);
  // Near a daylight-saving change the first offset can be the other side's.
  const check = wallClock(at, zone);
  if (check !== wall) at += wall - check;
  return new Date(at).toISOString().replace(/\.\d{3}Z$/, 'Z');
}

function limitError(limit, message) {
  const err = new Error(message);
  err.code = 'input_limit';
  err.limit = limit;
  return err;
}

function leg(pin, start, end, def) {
  return {
    calendars: { accrual: pin, payment: pin },
    dates: {
      accrual_adjustment: 'Unadjusted',
      ex_coupon: null,
      fixing_offset: null,
      observation_offset: null,
      payment_adjustment: 'Following',
      payment_delay: { BusinessDays: def.paymentDelay },
    },
    day_count: def.dayCount,
    negative_coupons: 'Allow',
    notionals: [{ start, end, value: '1000000' }],
    rounding: 'Unrounded',
    schedule: {
      start_date: start,
      end_date: end,
      end_of_month: false,
      first_regular_date: null,
      frequency: 'Annual',
      penultimate_date: null,
      stub_type: 'ShortFirst',
    },
  };
}

/** Spot: `spotLag` business days after the trade date, and a business day even at lag 0. */
export function oisEffectiveDate(def, tradeDate) {
  return adjustFollowing(def.calendar, addBusinessDays(def.calendar, tradeDate, def.spotLag));
}

/** Swap dates for each quote: effective (spot), maturity and final payment (the pillar). */
export function oisSchedule(def, tradeDate, tenors) {
  const effective = oisEffectiveDate(def, tradeDate);
  return tenors.map((tenor) => {
    const maturity = addTenor(effective, tenor);
    // As the engine dates a payment: the delay counts business days from the
    // unadjusted accrual end, then the result is adjusted (Following).
    const pillar = adjustFollowing(def.calendar, addBusinessDays(def.calendar, maturity, def.paymentDelay));
    return { tenor, effective, maturity, pillar };
  });
}

/**
 * Expected publication for every overnight observation from the first one the
 * swaps need (`from`, moved to a business day) to `lastDate`: the next
 * business day at the index's local publication time.
 */
export function oisPublications(def, from, lastDate) {
  const { time, zone } = def.publication;
  const out = [];
  for (let d = adjustFollowing(def.calendar, from); d <= lastDate;) {
    const next = addBusinessDays(def.calendar, d, 1);
    out.push({ expected_publication: zonedTimestamp(next, time, zone), index_id: def.index, observation_date: d });
    d = next;
  }
  return out;
}

/**
 * Calibration request for one OIS discount curve. `def` = { index, calendar,
 * spotLag, paymentDelay, dayCount, publication, currency }, `quotes` =
 * [{ tenor, value }] with value in percent. Throws with a plain message
 * (`code: 'input_limit'`, `limit: 'quotes' | 'publications'`) when the quotes
 * exceed what one fit or one request can carry, and when two quotes end on the
 * same payment date.
 */
export function oisCurveCalibration({ key, projectionKey, def, tradeDate, quotes, interpolation = 'Linear' }) {
  const name = def.label || def.index;
  if (quotes.length > MAX_OIS_QUOTES) {
    throw limitError('quotes', `${quotes.length} ${name} swap quotes are more than one fit takes: the engine solves `
      + `at most ${MAX_OIS_QUOTES} swaps per curve. Remove some tenors.`);
  }
  const pin = calendarPin(def.calendar);
  // Helpers and pillars go to the engine in payment-date order, which is not
  // always tenor order (1M from 31 January ends before 30D does).
  const dated = oisSchedule(def, tradeDate, quotes.map((q) => q.tenor))
    .map((d, i) => ({ ...d, quote: quotes[i] }))
    .sort((a, b) => (a.pillar < b.pillar ? -1 : a.pillar > b.pillar ? 1 : 0));
  for (let i = 1; i < dated.length; i++) {
    if (dated[i].pillar === dated[i - 1].pillar) {
      throw new Error(`${dated[i - 1].tenor} and ${dated[i].tenor} both pay on ${dated[i].pillar}; keep one of them.`);
    }
  }
  const effective = oisEffectiveDate(def, tradeDate);
  const last = dated.length ? dated[dated.length - 1].pillar : effective;
  const publications = oisPublications(def, effective, last);
  if (publications.length > MAX_PUBLICATIONS) {
    throw limitError('publications', `Reaching ${dated[dated.length - 1].tenor} needs ${publications.length.toLocaleString()} `
      + `daily ${name} publications; one request carries at most ${MAX_PUBLICATIONS.toLocaleString()}. `
      + `Keep swap tenors to ${MAX_OIS_YEARS} years, or supply longer points as zero rates.`);
  }
  const helpers = dated.map(({ quote: q, maturity, pillar }) => {
    const rate = percentToRate(q.value);
    const id = `${key.id}.${q.tenor}`;
    return {
      id,
      pillar,
      // DecimalRate parses to the same binary64 as the fixed-rate text, which
      // the native admission compares exactly.
      quote: { unit: 'DecimalRate', value: Number(rate) },
      terms: {
        collateral: { agreement: { id: `CSA.${def.currency}`, version: 1 }, currency: def.currency, discount_curve: key },
        currency: def.currency,
        effective_date: effective,
        fixed: { leg: leg(pin, effective, maturity, def), rates: [{ start: effective, end: maturity, value: rate }] },
        floating: {
          convention: { lockout_business_days: 0, lookback_business_days: 0, observation_shift: false },
          fixing_policy: { final_only: true, unpublished: 'Forecast' },
          index_id: def.index,
          kind: 'overnight',
          leg: leg(pin, effective, maturity, def),
          observation_calendar: pin,
          profiles: {
            gearings: [{ start: effective, end: maturity, value: '1' }],
            spreads: [{ start: effective, end: maturity, value: '0' }],
          },
          projection_curve: projectionKey,
        },
        kind: 'swap',
        maturity_date: maturity,
        schema_version: 1,
        side: 'PayFixed',
        trade_id: id,
        version: 1,
      },
      tolerance: 1e-10,
    };
  });
  const asOf = `${tradeDate}T12:00:00Z`;
  return request({ ...inputs({ calendars: [def.calendar] }), publications }, {
    kind: 'calibration',
    action: { kind: 'build' },
    input: {
      cash: [],
      controls: { jacobian_step: 1e-6, minimum_singular_ratio: 1e-10, svd_iterations: 1000 },
      credit: [],
      rate_context: {
        as_of: asOf,
        cashflows: {
          fixings: { final_only: true, known_at: asOf, unpublished: 'Forecast', valuation_date: tradeDate },
          on_payment: 'Include',
        },
      },
      rates: [
        {
          futures_calendar_stamps: helpers.map(() => null),
          quote_versions: helpers.map(() => 1),
          recipe: {
            bootstrap: { lower_zero: -0.08, max_iterations: 100, upper_zero: 0.25 },
            curve: {
              calibration: 'Piecewise',
              currency: def.currency,
              dependencies: [],
              extrapolation: 'Flat',
              interpolation,
              key,
              reference_date: tradeDate,
              representation: { ZeroRates: { compounding: 'Continuous' } },
              role: { collateral_currency: def.currency, kind: 'Discount' },
              source: { Helpers: helpers.map((h) => h.id) },
              time_basis: 'Act365Fixed',
            },
            helpers,
            overlap_policy: 'Reject',
            overnight_projection: { index_id: def.index, key: projectionKey },
            pillars: helpers.map((h) => h.pillar),
          },
        },
      ],
    },
  });
}
