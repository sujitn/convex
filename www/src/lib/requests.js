// Complete `library()` requests. Each builder returns a plain object that is
// exactly what goes over the wire (and what the request inspector shows).

import { calendarInput, adjustFollowing } from './calendars.js';
import { bondInput, icmaPeriods, securityId, bondTerms, calendarPins } from './terms.js';
import { decimal, percentToRate } from './decimal.js';

export const SCHEMA_VERSION = 1;

export function request(inputs, operation) {
  return { schema_version: SCHEMA_VERSION, inputs, operation };
}

export function inputs({ calendars = [], curves = [], fixings = [], calibration = null } = {}) {
  return {
    calendars: [...new Set(calendars)].sort().map(calendarInput),
    calibration,
    curves,
    fixings,
    helper_calendars: [],
    publications: [],
  };
}

export function bondContext(settlement) {
  return {
    fixing: {
      final_only: true,
      known_at: `${settlement}T00:00:00Z`,
      unpublished: 'Forecast',
      valuation_date: settlement,
    },
    on_payment: 'Exclude',
    overnight_accrual_cutoffs: [],
    settlement,
  };
}

export const INVERSE_SETTINGS = {
  lower: -0.5,
  upper: 1.0,
  max_iterations: 256,
  parameter_tolerance: 1e-12,
  price_tolerance: 1e-9,
};

export const SPREAD_INVERSE_SETTINGS = {
  lower: -0.5,
  upper: 1.0,
  max_iterations: 256,
  parameter_tolerance: 1e-12,
  price_tolerance: 1e-9,
};

// ---- bond ----------------------------------------------------------------

function bondRequest(sec, settlement, action, curves = []) {
  return request(inputs({ calendars: [sec.calendar], curves }), {
    kind: 'bond',
    bond: bondInput(sec),
    context: bondContext(settlement),
    projection: null,
    action,
  });
}

export const bondConstruct = (sec, settlement) => bondRequest(sec, settlement, { kind: 'construct' });
export const bondCashflows = (sec, settlement) => bondRequest(sec, settlement, { kind: 'cashflows' });

export function yieldMethod(decimalRate, convention) {
  return { kind: 'yield', decimal_rate: decimalRate, convention };
}

export function spreadMethod(curveKey, role, decimalSpread) {
  return { kind: 'discount_spread', curve: curveKey, role, decimal_spread: decimalSpread };
}

export const collateralRole = () => ({ kind: 'collateral' });
export const governmentRole = (sovereign) => ({ kind: 'government', sovereign });

export function bondPrice(sec, settlement, method, curves = []) {
  return bondRequest(sec, settlement, { kind: 'price', method }, curves);
}

export function bondYieldInverse(sec, settlement, target, convention) {
  return bondRequest(sec, settlement, {
    kind: 'yield_inverse',
    target,
    convention,
    settings: INVERSE_SETTINGS,
  });
}

export function bondSpreadInverse(sec, settlement, target, curve, role) {
  return bondRequest(
    sec,
    settlement,
    { kind: 'spread_inverse', target, curve: curve.key, role, settings: SPREAD_INVERSE_SETTINGS },
    [curve],
  );
}

/** Yield quote risk: signed currency per +1bp for the supplied face position. */
export function bondYieldRisk(sec, settlement, decimalRate, convention, faceAmount, snapshotId) {
  const id = securityId(sec);
  return bondRequest(sec, settlement, {
    kind: 'quote_risk',
    method: yieldMethod(decimalRate, convention),
    key: {
      coordinate: { input_id: id, kind: 'raw', version: 1 },
      factor: {
        basis: 'raw_quote',
        convention_version: 1,
        currency: sec.currency,
        family: 'interest_rate',
        model_version: 'convex-web.dated-yield.v1',
        quote_type: 'yield',
        target: { instrument_id: id, kind: 'instrument' },
        tenor: 'spot',
      },
      native_unit: 'DecimalRate',
      response: 'frozen_curves',
      unit: 'currency_per_bp',
    },
    position: {
      currency: sec.currency,
      quantity: decimal(faceAmount),
      quantity_unit: 'notional',
      reference_quantity: decimal(sec.face || '100'),
    },
    snapshot_id: snapshotId,
    settings: { bump: 0.0001, policy_version: 1, stability: { absolute: 0.05, relative: 0.0001 } },
  });
}

// ---- ICMA contextual quotation (Act/Act ICMA period time) -----------------

export function icmaConvention(sec, compounding) {
  return {
    schema_version: 1,
    reference_context: { id: `${securityId(sec)}.ICMA.MESH`, version: 1 },
    quotation_policy: { id: 'convex-web.icma.policy', version: 1 },
    frequency: sec.frequency,
    compounding,
    max_reference_evaluations: 16777216,
  };
}

export function icmaInput(sec, faceAmount) {
  const id = securityId(sec);
  return {
    security: {
      reference: { id, version: 1 },
      quote_revision: { id: `${id}.QUOTE`, version: 1 },
      bond: bondInput(sec),
    },
    reference_context: {
      revision: { id: `${id}.ICMA.MESH`, version: 1 },
      security: { id, version: 1 },
      frequency: sec.frequency,
      periods: icmaPeriods(sec),
    },
    quotation_policy: { id: 'convex-web.icma.policy', version: 1 },
    position: {
      reference_quantity: decimal(sec.face || '100'),
      quantity_unit: 'notional',
      currency: sec.currency,
      quantity: decimal(faceAmount),
    },
  };
}

function icmaRequest(sec, settlement, compounding, faceAmount, action) {
  return request(inputs({ calendars: [sec.calendar] }), {
    kind: 'icma_quotation',
    input: icmaInput(sec, faceAmount),
    convention: icmaConvention(sec, compounding),
    cash: { kind: 'plain', settlement, on_payment: 'Exclude' },
    action,
  });
}

export function icmaPrice(sec, settlement, decimalYield, compounding, faceAmount) {
  return icmaRequest(sec, settlement, compounding, faceAmount, { kind: 'price', decimal_yield: decimalYield });
}

export function icmaYield(sec, settlement, target, compounding, faceAmount) {
  return icmaRequest(sec, settlement, compounding, faceAmount, {
    kind: 'yield',
    target,
    settings: { lower: -0.5, upper: 1.0, price_tolerance: 1e-8, parameter_tolerance: 1e-12, max_iterations: 128 },
  });
}

// ---- curves ----------------------------------------------------------------

export function curveKey(id, version) {
  return { id, version: String(version) };
}

/** Direct zero-rate curve from tenor nodes (years, decimal rates). */
export function directZeroCurve({ key, currency, referenceDate, role, nodes, compounding = 'Continuous',
  interpolation = 'MonotoneConvex', extrapolation = 'Flat', timeBasis = 'Act365Fixed' }) {
  return {
    calibration: 'Direct',
    currency,
    dependencies: [],
    extrapolation,
    interpolation,
    key,
    reference_date: referenceDate,
    representation: { ZeroRates: { compounding } },
    role,
    source: {
      Nodes: nodes.map(([t, r]) => ({ pillar: { Tenor: t }, quote: { unit: 'DecimalRate', value: r } })),
    },
    time_basis: timeBasis,
  };
}

export function curvesBuild(curves, calendars = []) {
  return request(inputs({ calendars, curves }), { kind: 'curves' });
}

export function curveQuery(curves, key, query) {
  return request(inputs({ curves }), { kind: 'curve_query', curve: key, query });
}

export const zeroQuery = (years, compounding = 'Continuous', dayCount = 'Act365Fixed') => ({
  kind: 'ZeroRate', point: { Tenor: years }, compounding, day_count: dayCount,
});
export const dfQuery = (point) => ({ kind: 'DiscountFactor', point });
export const forwardQuery = (start, end, compounding = 'Simple', dayCount = 'Act365Fixed') => ({
  kind: 'ForwardRate', start: { Tenor: start }, end: { Tenor: end }, compounding, day_count: dayCount,
});

/**
 * Government curve calibrated from par yields: one nominal par bond per tenor
 * (coupon = par yield, clean 100) quoted as `ParCouponRate`, solved by the
 * native piecewise bootstrap. `quotes` = [{ tenor, maturity, parPct }].
 */
export function parCurveCalibration({ key, currency, calendar, tradeDate, settlement, settlementDays = 1,
  benchmarkId, quotes, frequency = 'SemiAnnual', dayCount = 'ActActIcma', interpolation = 'Linear' }) {
  const helpers = quotes.map((q, i) => {
    const sec = {
      id: `${key.id}.par.${q.tenor}`,
      currency,
      coupon: q.parPct,
      issueDate: settlement,
      datedDate: settlement,
      maturity: q.maturity,
      frequency,
      dayCount,
      face: '100',
      calendar,
      settlementDays,
      paymentAdjustment: 'Following',
      eom: false,
    };
    return {
      kind: 'Bond',
      input: {
        credit_identity: null,
        id: `${key.id}.${q.tenor}`,
        on_payment: 'Exclude',
        pillar: adjustFollowing(calendar, q.maturity),
        // DecimalRate parses to the same binary64 as the coupon text, which the
        // native par admission compares exactly.
        quote: { unit: 'DecimalRate', value: Number(percentToRate(q.parPct)) },
        quote_kind: { kind: 'ParCouponRate' },
        quote_version: q.version || 1,
        settlement,
        terms: bondTerms(sec),
        tolerance: 1e-9,
        trade_date: tradeDate,
      },
      _index: i,
    };
  });
  return request(inputs({ calendars: [calendar] }), {
    kind: 'calibration',
    action: { kind: 'build' },
    input: {
      cash: [
        {
          calendars: helpers.map(() => ({ kind: 'bond', pins: calendarPins(calendar) })),
          recipe: {
            basis_model: null,
            bootstrap: { lower_parameter: -0.05, max_iterations: 256, upper_parameter: 0.25 },
            curve: {
              calibration: 'Piecewise',
              currency,
              dependencies: [],
              extrapolation: 'Flat',
              interpolation,
              key,
              reference_date: tradeDate,
              representation: { ZeroRates: { compounding: 'Continuous' } },
              role: { benchmark_id: benchmarkId, kind: 'Government' },
              source: { Helpers: helpers.map((h) => h.input.id) },
              time_basis: 'Act365Fixed',
            },
            helpers: helpers.map(({ _index, ...h }) => h),
            overlap: 'Reject',
            pillars: helpers.map((h) => h.input.pillar),
          },
        },
      ],
      controls: { jacobian_step: 1e-6, minimum_singular_ratio: 1e-10, svd_iterations: 1000 },
      credit: [],
      rate_context: null,
      rates: [],
    },
  });
}
