// Dated-cash securities: price <-> yield quotation, invoice cash, and the
// actual-security government spread (G-spread) and its inverse. All go
// through the strict `cash_security`, `government_spread` and
// `government_spread_price` operations over the bond's complete entitled
// cash flows (terms.js), under an EXPLICIT dated-cash yield convention.
//
// Units of the quote values passed to these builders (as typed by a user):
//   CleanPrice / DirtyPrice   price per 100 of face       "99.125"
//   CashflowYield             annual PERCENT              "4.25"
//   spread                    basis points                "85.5"

import { request, inputs } from './requests.js';
import { bondTerms, calendarPins, securityId } from './terms.js';
import { adjustFollowing } from './calendars.js';
import { decimal, percentToRate } from './decimal.js';

export const CASH_INVERSE = { lower: -0.1, upper: 0.5, max_iterations: 128, parameter_tolerance: 1e-13, price_tolerance: 1e-9 };

const FREQ_COMPOUNDING = { Annual: 'Annual', SemiAnnual: 'SemiAnnual', Quarterly: 'Quarterly', Monthly: 'Monthly' };

/**
 * Dated-cash yield convention `{ compounding, day_count }` for a security.
 * Defaults to the security's own coupon frequency and day count, EXCEPT that
 * Act/Act ICMA needs a reference-period mesh and is rejected by the
 * context-free dated-cash route (`unsupported year fraction`). For such bonds
 * the discounting day count defaults to Act/Act ISDA; it is only the
 * quotation basis of this dated-cash yield (the bond's accrual still uses its
 * own ICMA day count) and the UI must label it. The ICMA-consistent yield is
 * `icmaYield`/`icmaPrice` in requests.js.
 */
export function cashConvention(sec, { compounding, dayCount } = {}) {
  return {
    compounding: compounding || FREQ_COMPOUNDING[sec.frequency] || 'SemiAnnual',
    day_count: dayCount || (sec.dayCount === 'ActActIcma' ? 'ActActIsda' : sec.dayCount),
  };
}

function quoteParts(quote, convention) {
  switch (quote.kind) {
    case 'CleanPrice':
    case 'DirtyPrice':
      return { quote: { unit: 'PricePer100', value: Number(decimal(quote.value)) }, quote_kind: { kind: quote.kind } };
    case 'CashflowYield':
      return {
        quote: { unit: 'DecimalRate', value: Number(percentToRate(quote.value)) },
        quote_kind: {
          kind: 'CashflowYield',
          convention: {
            ...convention,
            lower_yield: CASH_INVERSE.lower,
            upper_yield: CASH_INVERSE.upper,
            max_iterations: 128,
            price_tolerance: 1e-9,
          },
        },
      };
    default:
      throw new Error(`unsupported quote kind ${quote.kind}`);
  }
}

/** The library's `CashBondHelper` for a security and its quote. */
export function cashHelper(sec, { tradeDate, settlement, quote, convention, quoteVersion = 1 }) {
  return {
    credit_identity: null,
    id: securityId(sec),
    on_payment: 'Exclude',
    // The final cash payment date (maturity, business-day adjusted).
    pillar: adjustFollowing(sec.calendar, sec.maturity),
    ...quoteParts(quote, convention),
    quote_version: quoteVersion,
    settlement,
    terms: bondTerms(sec),
    tolerance: 1e-8,
    trade_date: tradeDate,
  };
}

function cashRequest(sec, { tradeDate, settlement, quote, convention, quoteVersion }, action) {
  convention = convention || cashConvention(sec);
  return request(inputs({ calendars: [sec.calendar] }), {
    kind: 'cash_security',
    action,
    convention,
    input: {
      calendars: calendarPins(sec.calendar),
      helper: cashHelper(sec, { tradeDate, settlement, quote, convention, quoteVersion }),
    },
    reference_date: tradeDate,
    settings: CASH_INVERSE,
  });
}

/**
 * Convert one quote into the full set: clean/dirty price per 100, accrued and
 * the dated-cash yield. `quote` = `{ kind: 'CleanPrice'|'DirtyPrice'|'CashflowYield', value }`.
 */
export const cashQuote = (sec, opts) => cashRequest(sec, opts, { kind: 'quote' });

/** Invoice (settlement) cash for `faceAmount` of original face, e.g. "1000000". */
export const cashInvoice = (sec, opts) =>
  cashRequest(sec, opts, {
    kind: 'invoice',
    position: {
      currency: sec.currency,
      quantity: decimal(opts.faceAmount),
      quantity_unit: 'notional',
      reference_quantity: decimal(sec.face || '100'),
    },
  });

// ---- government spread -------------------------------------------------------

const SELECTION = {
  LinearMaturity: 'LinearMaturity',
  Linear: 'LinearMaturity',
  Exact: 'ExactMaturity',
  ExactMaturity: 'ExactMaturity',
};

function governmentInputs(targetSec, benchmarks, { tradeDate, settlement, convention, benchmarkId, selection }) {
  const conv = convention || cashConvention(targetSec);
  const bench = benchmarks.map((b) => ({
    calendars: calendarPins(b.sec.calendar),
    quote: {
      benchmark_id: benchmarkId,
      helper: cashHelper(b.sec, { tradeDate, settlement, quote: b.quote, convention: conv, quoteVersion: b.quoteVersion || 1 }),
      security: { id: securityId(b.sec), version: b.version || 1 },
    },
  }));
  const sel = SELECTION[selection];
  if (!sel) throw new Error(`unknown selection ${selection}`);
  return {
    bench,
    government: { benchmark_id: benchmarkId, convention: conv, inverse: CASH_INVERSE, selection: sel },
    calendars: [targetSec.calendar, ...benchmarks.map((b) => b.sec.calendar)],
  };
}

/**
 * G-spread: the target's dated-cash yield minus the benchmark yield selected
 * by contractual maturity (`'LinearMaturity'`: interpolated between the two
 * actual bracketing securities by maturity; `'Exact'`: a benchmark maturing
 * the same day). `benchmarks` = `[{ sec, quote: {kind:'CleanPrice', value} }]`.
 * Every benchmark is validated and inverted, including unselected ones.
 */
export function governmentSpread(targetSec, targetQuote, benchmarks, {
  tradeDate, settlement, selection = 'LinearMaturity', convention, benchmarkId = 'US.TREASURY', quoteVersion,
}) {
  const g = governmentInputs(targetSec, benchmarks, { tradeDate, settlement, convention, benchmarkId, selection });
  return request(inputs({ calendars: g.calendars }), {
    kind: 'government_spread',
    benchmarks: g.bench,
    reference_date: tradeDate,
    settings: g.government,
    target: {
      calendars: calendarPins(targetSec.calendar),
      helper: cashHelper(targetSec, {
        tradeDate, settlement, quote: targetQuote, convention: g.government.convention, quoteVersion,
      }),
    },
  });
}

/**
 * Inverse: price the target from a G-spread in basis points ("85.5") over the
 * selected benchmark yield. `faceAmount` sets the position for the
 * consideration (cash) output.
 */
export function governmentSpreadPrice(targetSec, spreadBp, benchmarks, {
  tradeDate, settlement, selection = 'LinearMaturity', convention, benchmarkId = 'US.TREASURY',
  faceAmount = '1000000', quoteVersion = 1, spreadTolerance = 1e-10,
}) {
  const g = governmentInputs(targetSec, benchmarks, { tradeDate, settlement, convention, benchmarkId, selection });
  return request(inputs({ calendars: g.calendars }), {
    kind: 'government_spread_price',
    benchmarks: g.bench,
    position: {
      currency: targetSec.currency,
      quantity: decimal(faceAmount),
      quantity_unit: 'notional',
      reference_quantity: decimal(targetSec.face || '100'),
    },
    quote: { quote_version: quoteVersion, spread: { unit: 'BasisPoints', value: Number(decimal(spreadBp)) } },
    reference_date: tradeDate,
    settings: { decimal_spread_tolerance: spreadTolerance, government: g.government },
    target: {
      calendars: calendarPins(targetSec.calendar),
      target: {
        on_payment: 'Exclude',
        security: { id: securityId(targetSec), version: 1 },
        settlement,
        terms: bondTerms(targetSec),
        trade_date: tradeDate,
      },
    },
  });
}
