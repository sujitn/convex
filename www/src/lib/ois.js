// Overnight-indexed swap curves from par swap rates you supply. Each tenor is
// one spot-starting par OIS (fixed vs compounded overnight, same schedule),
// fitted by the native rate calibration as a Discount curve whose zero-basis
// overnight projection is declared explicitly.

import { calendarPin, addBusinessDays, adjustFollowing } from './calendars.js';
import { addTenor } from './dates.js';
import { percentToRate } from './decimal.js';
import { request, inputs } from './requests.js';

// Market conventions used for the standard spot-starting swaps. They are the
// defaults the editor shows; every one can be changed per curve.
export const OIS_INDICES = {
  'USD.SOFR': { label: 'SOFR', currency: 'USD', calendar: 'US.SIFMA', spotLag: 2, paymentDelay: 2, dayCount: 'Act360', publishedAt: '12:00:00Z' },
  'EUR.ESTR': { label: '€STR', currency: 'EUR', calendar: 'EU.TARGET2', spotLag: 2, paymentDelay: 1, dayCount: 'Act360', publishedAt: '07:00:00Z' },
  'GBP.SONIA': { label: 'SONIA', currency: 'GBP', calendar: 'GB.LON', spotLag: 0, paymentDelay: 0, dayCount: 'Act365Fixed', publishedAt: '08:00:00Z' },
  'CAD.CORRA': { label: 'CORRA', currency: 'CAD', calendar: 'CA.TOR', spotLag: 1, paymentDelay: 1, dayCount: 'Act365Fixed', publishedAt: '15:00:00Z' },
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
// reaches about 15 years. Longer points can be supplied as zero rates.
export const MAX_PUBLICATIONS = 4000;
export const MAX_OIS_YEARS = 15;

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

/** Swap dates for each quote: effective (spot), maturity and final payment (the pillar). */
export function oisSchedule(def, tradeDate, tenors) {
  const effective = addBusinessDays(def.calendar, tradeDate, def.spotLag);
  return tenors.map((tenor) => {
    const maturity = addTenor(effective, tenor);
    const pillar = addBusinessDays(def.calendar, adjustFollowing(def.calendar, maturity), def.paymentDelay);
    return { tenor, effective, maturity, pillar };
  });
}

/** Expected publication for every overnight observation from the trade date to the last payment. */
export function oisPublications(def, tradeDate, lastDate) {
  const out = [];
  for (let d = tradeDate; d <= lastDate;) {
    const next = addBusinessDays(def.calendar, d, 1);
    out.push({ expected_publication: `${next}T${def.publishedAt}`, index_id: def.index, observation_date: d });
    d = next;
  }
  return out;
}

/**
 * Calibration request for one OIS discount curve. `def` = { index, calendar,
 * spotLag, paymentDelay, dayCount, publishedAt, currency }, `quotes` =
 * [{ tenor, value }] with value in percent. Throws with a plain message when
 * the horizon exceeds what one request can carry.
 */
export function oisCurveCalibration({ key, projectionKey, def, tradeDate, quotes, interpolation = 'Linear' }) {
  const pin = calendarPin(def.calendar);
  const dates = oisSchedule(def, tradeDate, quotes.map((q) => q.tenor));
  const last = dates.reduce((a, d) => (d.pillar > a ? d.pillar : a), tradeDate);
  const publications = oisPublications(def, tradeDate, last);
  if (publications.length > MAX_PUBLICATIONS) {
    const err = new Error(`Reaching ${quotes[quotes.length - 1].tenor} needs ${publications.length.toLocaleString()} daily `
      + `${def.label || def.index} publications; one request carries at most ${MAX_PUBLICATIONS.toLocaleString()}. `
      + `Keep swap tenors to ${MAX_OIS_YEARS} years, or supply longer points as zero rates.`);
    err.code = 'input_limit';
    throw err;
  }
  const helpers = quotes.map((q, i) => {
    const { effective, maturity, pillar } = dates[i];
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
