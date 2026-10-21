// Yield-and-spread analysis: one orchestration over many native library calls.
// Each section keeps its own native error; a failed spread never blanks the
// price, and no number is substituted for a missing one.

import * as R from '../lib/requests.js';
import { cashConvention, cashInvoice, governmentSpread, governmentSpreadPrice } from '../lib/cash.js';
import { currentYield, simpleYield } from '../lib/scalars.js';
import { parsePrice, fromNumber, percentToRate } from '../lib/decimal.js';
import { FREQ_PER_YEAR } from '../lib/terms.js';
import { daysBetween } from '../lib/dates.js';

const DC = { ActActIsda: 'Act/Act ISDA', Thirty360US: '30/360 US', Thirty360E: '30E/360', Act365Fixed: 'Act/365F', Act360: 'Act/360' };

export const LADDER_BP = [-100, -50, -25, -10, -5, 0, 5, 10, 25, 50, 100];

function compounding(sec) {
  return { Annual: 'Annual', SemiAnnual: 'SemiAnnual', Quarterly: 'Quarterly', Monthly: 'Monthly' }[sec.frequency] || 'SemiAnnual';
}

const icmaValue = (data) => data.quotation.value;

/** Price, yields, spreads, risk, invoice and ladder for one security and quote. */
export async function computeYas(engine, { sec: security, tradeDate, settlement, quote, face, curve, benchmarks = [],
  benchmarkCurve = curve, benchPrices = {}, benchmarkSelection = 'LinearMaturity' }) {
  // Straight yield of an optional bond is conditional on no exercise, so the
  // straight analysis runs on the bond with its calls explicitly removed and
  // says so; workouts and OAS treat the calls.
  const callable = (security.calls || []).length > 0;
  const sec = callable ? { ...security, calls: [] } : security;
  const out = { errors: {}, sources: {}, callsIgnored: callable };
  const comp = compounding(sec);
  // The spread role names the curve's own identity exactly: its government
  // benchmark, or collateral for a swap (discount) curve.
  const role = curve ? curve.spreadRole : null;
  const label = sec.ticker || sec.name;

  // 1. Resolve the quote to a clean price.
  let clean;
  if (quote.kind === 'price') {
    clean = parsePrice(quote.text);
  } else if (quote.kind === 'yield') {
    const res = await engine.call(R.icmaPrice(sec, settlement, Number(percentToRate(quote.text)), comp, face),
      { label: `${label} price from yield` });
    if (!res.ok) return { ...out, fatal: res.error };
    clean = icmaValue(res.data).clean_per100;
  } else if (quote.kind === 'zspread') {
    if (!curve || !curve.recipe) return { ...out, fatal: { code: 'missing_input', message: 'No discount curve is available for this currency.' } };
    const z = Number(quote.text) / 10000;
    const res = await engine.call(R.bondPrice(sec, settlement, R.spreadMethod(curve.key, role, z), [curve.recipe]),
      { label: `${label} price from Z-spread` });
    if (!res.ok) return { ...out, fatal: res.error };
    clean = res.data.clean_per100;
  } else if (quote.kind === 'gspread') {
    const bench = await benchmarkQuotes(engine, benchmarks, settlement, benchmarkCurve, benchPrices);
    if (bench.error) return { ...out, fatal: bench.error };
    const res = await engine.call(governmentSpreadPrice(sec, quote.text, bench.list,
      { tradeDate, settlement, faceAmount: face, selection: benchmarkSelection, benchmarkId: benchmarkIdFor(sec.currency) }),
      { label: `${label} price from G-spread` });
    if (!res.ok) return { ...out, fatal: res.error };
    clean = res.data.valuation.clean_per100;
  }
  if (!Number.isFinite(clean)) return { ...out, fatal: { code: 'invalid_input', message: `"${quote.text}" is not a usable quote.` } };
  out.clean = clean;

  // 2. ICMA yield from the clean price, with dirty price, accrued and consideration.
  const base = await engine.call(R.icmaYield(sec, settlement, { kind: 'clean', value: clean }, comp, face),
    { label: `${label} ICMA yield` });
  if (!base.ok) return { ...out, fatal: base.error };
  const val = icmaValue(base.data);
  const y = val.decimal_parameter;
  out.ytm = y;
  out.dirty = val.valuation.dirty_per100;
  out.accrued = val.valuation.accrued_per100;
  out.consideration = base.data.consideration;
  out.cash = val.valuation.cash;
  out.inverse = { iterations: val.iterations, residual: val.native_residual };

  // 3. Risk from native ICMA valuations at y ± 1bp, and the price/yield ladder.
  const h = 1e-4;
  const bumps = [y - h, y + h, ...LADDER_BP.map((b) => y + b / 10000)];
  const priced = await engine.callMany(bumps.map((yy) => R.icmaPrice(sec, settlement, yy, comp, face)),
    { label: `${label} yield ladder` });
  if (priced.every((p) => p.ok)) {
    const dirtyAt = priced.map((p) => icmaValue(p.data).dirty_per100);
    const [pm, pp] = dirtyAt;
    const p0 = out.dirty;
    const f = FREQ_PER_YEAR[sec.frequency] || 2;
    const mod = (pm - pp) / (2 * h * p0);
    out.risk = {
      modified: mod,
      macaulay: mod * (1 + y / f),
      convexity: (pp + pm - 2 * p0) / (h * h * p0),
      dv01Per100: (pm - pp) / 2,
      dv01: ((pm - pp) / 2) * (Number(face) / 100),
    };
    out.ladder = LADDER_BP.map((b, i) => {
      const v = icmaValue(priced[i + 2].data);
      return { bp: b, yield: y + b / 10000, clean: v.clean_per100, dirty: v.dirty_per100, change: v.dirty_per100 - p0 };
    });
  } else {
    out.errors.risk = priced.find((p) => !p.ok).error;
  }

  // 4. Alternative yield conventions (all native; labelled by basis).
  const altSpecs = [
    { label: 'ICMA, annual equivalent', req: R.icmaYield(sec, settlement, { kind: 'clean', value: clean }, 'Annual', face), icma: true },
    { label: 'ICMA, continuous', req: R.icmaYield(sec, settlement, { kind: 'clean', value: clean }, 'Continuous', face), icma: true },
    { label: `Dated cash, ${DC[cashConvention(sec).day_count] || cashConvention(sec).day_count}, ${cashConvention(sec).compounding === 'SemiAnnual' ? 'semi-annual' : cashConvention(sec).compounding.toLowerCase()}`,
      req: R.bondYieldInverse(sec, settlement, { kind: 'clean', value: clean }, cashConvention(sec)) },
    { label: 'Dated cash, Act/365F, annual',
      req: R.bondYieldInverse(sec, settlement, { kind: 'clean', value: clean }, { compounding: 'Annual', day_count: 'Act365Fixed' }) },
  ];
  const alts = await engine.callMany(altSpecs.map((s) => s.req), { label: `${label} yield conventions` });
  out.yields = [
    { label: `ICMA, ${comp === 'SemiAnnual' ? 'semi-annual' : comp.toLowerCase()} (street equivalent)`, value: y },
    ...altSpecs.map((s, i) => ({
      label: s.label,
      value: alts[i].ok ? (s.icma ? icmaValue(alts[i].data).decimal_parameter : alts[i].data.decimal_parameter) : NaN,
      error: alts[i].ok ? null : alts[i].error,
    })),
  ];

  // 5. Scalar yields.
  const cleanText = fromNumber(clean, 10);
  const scalars = await engine.callMany([currentYield(sec, cleanText), simpleYield(sec, settlement, cleanText)],
    { label: `${label} current and simple yield` });
  out.currentYield = scalars[0].ok ? Number(scalars[0].data.value) / 100 : NaN;
  out.simpleYield = scalars[1].ok ? Number(scalars[1].data.value) / 100 : NaN;
  if (!scalars[0].ok) out.errors.scalars = scalars[0].error; // current yield
  if (!scalars[1].ok) out.errors.simpleYield = scalars[1].error;

  // 6. Z-spread over the chosen curve and spread risk.
  if (curve && curve.recipe) {
    const zr = await engine.call(R.bondSpreadInverse(sec, settlement, { kind: 'clean', value: clean }, curve.recipe, role),
      { label: `${label} Z-spread` });
    if (zr.ok) {
      const z = zr.data.decimal_parameter;
      out.zspread = z;
      const zb = await engine.callMany([z - h, z + h].map((zz) =>
        R.bondPrice(sec, settlement, R.spreadMethod(curve.key, role, zz), [curve.recipe])), { label: `${label} spread bumps` });
      if (zb.every((r) => r.ok)) {
        const [dm, dp] = zb.map((r) => r.data.dirty_per100);
        out.spreadDuration = (dm - dp) / (2 * h * out.dirty);
        out.cs01 = ((dm - dp) / 2) * (Number(face) / 100);
      } else out.errors.spreadRisk = zb.find((r) => !r.ok).error;
    } else out.errors.zspread = zr.error;
    out.sources.curve = curve.key;
  } else out.errors.zspread = { code: 'missing_input', message: 'No curve is available for this currency.' };

  // 7. G-spread against actual benchmark securities in the target's currency:
  // interpolated between the two that bracket its maturity, or the one that
  // matures the same day.
  const usable = benchmarks.filter((b) => b.sec.currency === sec.currency);
  const needed = benchmarkSelection === 'ExactMaturity' ? 1 : 2;
  if (usable.length >= needed) {
    const bench = await benchmarkQuotes(engine, usable, settlement, benchmarkCurve, benchPrices);
    if (bench.error) out.errors.gspread = bench.error;
    else {
      const gr = await engine.call(governmentSpread(sec, { kind: 'CleanPrice', value: cleanText }, bench.list,
        { tradeDate, settlement, selection: benchmarkSelection, benchmarkId: benchmarkIdFor(sec.currency) }),
      { label: `${label} G-spread` });
      if (gr.ok) {
        out.gspread = {
          bp: gr.data.basis_points,
          decimal: gr.data.decimal_spread,
          benchYield: gr.data.benchmark_decimal_yield,
          targetYield: gr.data.target.yield_inverse.decimal_parameter,
          selected: gr.data.selected,
          benchmarks: bench.list,
          convention: cashConvention(sec),
          selection: benchmarkSelection,
        };
      } else out.errors.gspread = gr.error;
    }
  } else {
    out.errors.gspread = { code: 'not_applicable', message: `G-spread needs ${needed === 1 ? 'a' : 'at least two'} ${sec.currency} `
      + `benchmark securit${needed === 1 ? 'y' : 'ies'}; choose them under G-spread benchmarks.` };
  }

  // 8. Invoice for the face amount.
  const inv = await engine.call(cashInvoice(sec, { tradeDate, settlement, quote: { kind: 'CleanPrice', value: cleanText },
    faceAmount: face }), { label: `${label} invoice` });
  if (inv.ok) {
    out.invoice = { total: inv.data.consideration.value, face: Number(face) };
  } else out.errors.invoice = inv.error;

  // Accrual period facts from the native cash schedule.
  const coupons = (out.cash && out.cash.coupons) || [];
  const current = coupons.find((c) => c.accrual_start <= settlement && settlement < c.accrual_end);
  if (current) {
    out.accruedDays = daysBetween(current.accrual_start, settlement);
    out.prevCoupon = current.accrual_start;
    out.nextCoupon = current.payment_date;
  }
  return out;
}

/** The government benchmark identity sent with G-spread requests, per currency. */
export function benchmarkIdFor(currency) {
  return currency === 'USD' ? 'US.TREASURY' : `${currency}.GOVERNMENT`;
}

/**
 * Benchmark quotes: actual securities, at the price you entered for one (by
 * CUSIP, or id for a bond without one), else the price it was imported with,
 * else priced off the fitted government curve (curve-implied). A price you
 * entered that can't be read is an input error, never replaced.
 */
export async function benchmarkQuotes(engine, benchmarks, settlement, curve, prices = {}) {
  const yours = [];
  for (const b of benchmarks) {
    const key = b.sec.cusip || b.sec.id;
    const text = prices[key] !== undefined && String(prices[key]).trim() !== '' ? prices[key] : b.sec.price;
    if (text === undefined || text === null || String(text).trim() === '') {
      yours.push(null);
      continue;
    }
    let v = NaN;
    try {
      v = parsePrice(text);
    } catch {
      /* reported below */
    }
    if (!(v > 0)) {
      return { error: { code: 'invalid_input', details: { field: 'benchmark_price', cusip: key },
        message: `The benchmark price "${text}" you entered for ${key} is not a positive price per 100 `
          + '(e.g. 99.5 or 99-16+). Correct it or clear it to use the curve-implied price.' } };
    }
    yours.push(v);
  }
  const implied = benchmarks.filter((_, i) => yours[i] === null);
  let res = [];
  if (implied.length) {
    if (!curve || !curve.recipe) {
      return { error: { code: 'missing_input', message: 'No government curve for curve-implied benchmark prices: enter a price for each benchmark.' } };
    }
    res = await engine.callMany(
      implied.map((b) => R.bondPrice(b.sec, settlement, R.spreadMethod(curve.key, curve.spreadRole, 0), [curve.recipe])),
      { label: 'benchmark curve-implied prices' },
    );
    const bad = res.find((r) => !r.ok);
    if (bad) return { error: bad.error };
  }
  let k = 0;
  return {
    list: benchmarks.map((b, i) => {
      const own = yours[i] !== null;
      const price = own ? yours[i] : res[k++].data.clean_per100;
      return {
        sec: b.sec,
        bucket: b.bucket,
        quote: { kind: 'CleanPrice', value: fromNumber(price, 10) },
        provenance: own ? (prices[b.sec.cusip || b.sec.id] ? 'manual' : 'imported') : 'curve-implied',
      };
    }),
  };
}
