// Everything you supplied (edited datasets, your curves and securities,
// benchmark prices and the G-spread benchmark choice, the curve chosen for
// spreads per currency) as one JSON
// document, and the checked way back in. Every record is checked whole: a
// malformed one is left out and listed in `rejected`, never partly imported.

import { OIS_INDICES } from './ois.js';
import { CALENDAR_RULES } from './calendars.js';
import { isIso, compareIso } from './dates.js';
import { parsePrice } from './decimal.js';
import { normaliseTenor } from './paste.js';
import { FREQ_MONTHS } from './terms.js';
import { CURVE_DEFS, INTERPOLATIONS } from '../analytics/curves.js';
import { EXAMPLES } from '../analytics/securities.js';
import { SOURCES } from '../market/feeds.js';
import { SENIORITIES, DOC_CLAUSES, CDS_CALENDARS, CALENDAR_CHOICES } from './cds.js';
import { FRN_INDICES } from './frn.js';

export const DATA_KIND = 'convex-terminal.data';
export const DATA_VERSION = 1;
export const DAY_COUNTS = ['ActActIcma', 'ActActIsda', 'Thirty360US', 'Thirty360E', 'Act365Fixed', 'Act360'];

/** Everything you supplied, as one JSON document (the inverse of the importData action). */
export function exportData(state) {
  return {
    kind: DATA_KIND,
    version: DATA_VERSION,
    exportedAt: new Date().toISOString(),
    datasets: Object.fromEntries(Object.entries(state.datasets).filter(([, d]) => d && d.status === 'manual')),
    custom: state.custom,
    customCurves: state.customCurves,
    credit: state.credit || {},
    spreadCurve: state.spreadCurve || {},
    benchPrices: state.benchPrices,
    benchmarkChoice: state.benchmarkChoice,
  };
}

const isObject = (v) => !!v && typeof v === 'object' && !Array.isArray(v);
const isText = (v) => typeof v === 'string' && v.trim() !== '';
const isCurrency = (v) => typeof v === 'string' && /^[A-Z]{3}$/.test(v);
// Plain decimal text exactly as the library takes it: no commas, exponents or signs other than a leading one.
const isPlainDecimal = (v) => typeof v === 'string' && /^[+-]?(?:\d+\.?\d*|\.\d+)$/.test(v);
const isTenor = (v) => typeof v === 'string' && normaliseTenor(v) === v;

/** The first reason a curve point is unusable, or null. `missingOk`: '' or null marks an unquoted tenor. */
function pointProblem(p, i, missingOk) {
  if (!isObject(p)) return `point ${i + 1} is not a { tenor, value } record`;
  if (!isTenor(p.tenor)) return `point ${i + 1} has no valid tenor such as 3M or 10Y`;
  if (missingOk && (p.value === '' || p.value === null)) return null;
  if (!isPlainDecimal(p.value)) return `point ${i + 1} (${p.tenor}) value is not a decimal number in text`;
  return null;
}

function pointsProblem(points, missingOk) {
  if (!Array.isArray(points)) return 'points are missing';
  for (let i = 0; i < points.length; i++) {
    const problem = pointProblem(points[i], i, missingOk);
    if (problem) return problem;
  }
  return null;
}

const CURVE_ROLES = ['discount', 'government'];
const CURVE_SOURCES = new Map(SOURCES.filter((s) => s.kind === 'par' || s.kind === 'zero').map((s) => [s.id, s]));

function datasetProblem(id, d) {
  if (!isObject(d) || d.id !== id) return 'id does not match its entry';
  if (!CURVE_SOURCES.has(id)) return 'not a curve dataset this app edits';
  if (!isIso(d.asOf)) return `as-of date "${d.asOf}" is not a real date`;
  if (d.revision !== undefined && !Number.isSafeInteger(d.revision)) return 'revision is not a whole number';
  return pointsProblem(d.points, true);
}

function curveProblem(id, c) {
  if (!isObject(c) || c.id !== id || c.custom !== true) return 'id does not match its entry';
  if (CURVE_DEFS[id]) return `"${id}" is a built-in curve id`;
  if (!isCurrency(c.currency)) return 'currency is not a three-letter code';
  if (!isIso(c.asOf)) return `as-of date "${c.asOf}" is not a real date`;
  if (c.label !== undefined && typeof c.label !== 'string') return 'label is not text';
  if (c.interpolation !== undefined && !INTERPOLATIONS.includes(c.interpolation)) return `interpolation ${c.interpolation} is not supported`;
  if (c.method === 'ois') {
    const index = OIS_INDICES[c.index];
    if (!index) return `overnight index ${c.index} is not supported`;
    if (index.currency !== c.currency) return `${c.index} is not a ${c.currency} index`;
    // The OIS fit is always built in the collateral discount role.
    if (c.role !== 'discount') return `role ${c.role}: an OIS curve is a discount curve`;
  } else if (c.method === 'par' || c.method === 'zero') {
    if (!CALENDAR_RULES[c.calendar]) return `calendar ${c.calendar} is not known`;
    if (c.method === 'par') {
      // buildCurve sends these straight to the engine as par-swap conventions.
      if (!FREQ_MONTHS[c.frequency]) return `coupon frequency ${c.frequency} is not supported`;
      if (!DAY_COUNTS.includes(c.dayCount)) return `day count ${c.dayCount} is not supported`;
      if (!(Number.isInteger(c.settlementDays) && c.settlementDays >= 0 && c.settlementDays <= 10)) return 'settlement days is not a whole number from 0 to 10';
    }
    if (!CURVE_ROLES.includes(c.role)) return `role ${c.role} is not discount or government`;
  } else return `method ${c.method} is not supported`;
  return pointsProblem(c.points, false);
}

function creditProblem(id, c) {
  if (!isObject(c) || c.id !== id) return 'id does not match its entry';
  if (!isText(c.entity)) return 'no reference entity';
  if (!CALENDAR_CHOICES[c.currency]) return `currency ${c.currency} is not USD or EUR`;
  if (!SENIORITIES[c.seniority]) return `seniority ${c.seniority} is not supported`;
  if (!isPlainDecimal(c.recovery) || !(Number(c.recovery) >= 0 && Number(c.recovery) < 100)) return `recovery "${c.recovery}" is not a percentage`;
  if (!DOC_CLAUSES[c.docClause]) return `clause ${c.docClause} is not supported`;
  if (!CDS_CALENDARS[c.calendar] || !CALENDAR_CHOICES[c.currency].includes(c.calendar)) return `calendar ${c.calendar} is not a ${c.currency} CDS calendar`;
  if (c.discount !== undefined && typeof c.discount !== 'string') return 'discount curve choice is not text';
  return pointsProblem(c.points, false);
}

function rowsProblem(rows, field, what) {
  if (rows === undefined || rows === null) return null;
  if (!Array.isArray(rows)) return `${what} rows are not a list`;
  for (let i = 0; i < rows.length; i++) {
    const r = rows[i];
    const blank = (v) => v === undefined || v === null || v === '';
    if (isObject(r) && blank(r.date) && blank(r[field])) continue; // an empty editor line
    if (!isObject(r) || !isIso(r.date) || !isPlainDecimal(r[field])) return `${what} row ${i + 1} needs a real date and a decimal ${field}`;
  }
  return null;
}

function securityProblem(id, s) {
  if (!isObject(s) || s.id !== id) return 'id does not match its entry';
  // An edited example is saved under its own id and stays source "example"; anything else may not take it.
  if (EXAMPLES.some((e) => e.id === id) && s.source !== 'example') return `"${id}" is a built-in example security id`;
  if (s.kind !== undefined && s.kind !== 'fixed' && s.kind !== 'floating') return `kind ${s.kind} cannot be imported`;
  if (!isCurrency(s.currency)) return 'currency is not a three-letter code';
  if (s.kind === 'floating') {
    const index = FRN_INDICES[s.index];
    if (!index) return `floating index ${s.index} is not supported`;
    if (index.currency !== s.currency) return `${s.index} is not a ${s.currency} index`;
    if (!isPlainDecimal(s.spreadBp)) return 'spread is not a decimal number of basis points in text';
    for (const f of ['lookbackDays', 'lockoutDays', 'paymentDelayDays']) {
      if (s[f] !== undefined && !(Number.isInteger(s[f]) && s[f] >= 0 && s[f] <= 10)) return `${f} is not a whole number from 0 to 10`;
    }
  } else if (!isPlainDecimal(s.coupon)) return 'coupon is not a decimal number in text';
  if (!isIso(s.issueDate)) return `issue date "${s.issueDate}" is not a real date`;
  if (!isIso(s.maturity)) return `maturity "${s.maturity}" is not a real date`;
  if (compareIso(s.issueDate, s.maturity) >= 0) return 'maturity is not after the issue date';
  for (const f of ['datedDate', 'firstCouponDate']) {
    if (s[f] !== undefined && s[f] !== null && s[f] !== '' && !isIso(s[f])) return `${f} "${s[f]}" is not a real date`;
  }
  if (!FREQ_MONTHS[s.frequency]) return `frequency ${s.frequency} is not supported`;
  if (!DAY_COUNTS.includes(s.dayCount)) return `day count ${s.dayCount} is not supported`;
  if (!CALENDAR_RULES[s.calendar]) return `calendar ${s.calendar} is not known`;
  for (const f of ['face', 'redemption']) {
    if (s[f] !== undefined && !isPlainDecimal(s[f])) return `${f} is not a decimal number in text`;
  }
  if (s.settlementDays !== undefined && !(Number.isInteger(Number(s.settlementDays)) && Number(s.settlementDays) >= 0)) {
    return 'settlement days is not a whole number';
  }
  return rowsProblem(s.calls, 'price', 'call') || rowsProblem(s.steps, 'coupon', 'coupon step')
    || rowsProblem(s.sinking, 'amount', 'sinking-fund');
}

function benchmarkChoiceProblem(c) {
  if (!isObject(c)) return 'not a { mode, ids, selection } record';
  if (c.mode !== 'otr' && c.mode !== 'custom') return `benchmark set ${c.mode} is not otr or custom`;
  if (c.selection !== 'LinearMaturity' && c.selection !== 'ExactMaturity') return `selection ${c.selection} is not LinearMaturity or ExactMaturity`;
  if (!Array.isArray(c.ids) || !c.ids.every(isText)) return 'ids are not a list of security ids';
  return null;
}

function benchPriceProblem(cusip, text) {
  if (!isText(cusip)) return 'no CUSIP';
  if (typeof text !== 'string') return 'price is not text';
  try {
    return parsePrice(text) > 0 ? null : 'price is not positive';
  } catch {
    return `"${text}" is not a price`;
  }
}

/**
 * Check an imported document; returns `{ datasets, customCurves, custom,
 * benchPrices, spreadCurve, benchmarkChoice, rejected }` (benchmarkChoice
 * undefined when the file has none) or throws when the file is not one of
 * ours. `rejected` lists every record left out: `{ section, id, reason }`.
 * Pass the current custom curves as `existing.customCurves` so a spread-curve
 * choice may name a curve you already have.
 */
export function readImport(json, existing = {}) {
  if (!json || json.kind !== DATA_KIND) throw new Error('Not a Convex Terminal data file.');
  if (json.version !== DATA_VERSION) {
    throw new Error(`This data file is version ${JSON.stringify(json.version)}; this app reads version ${DATA_VERSION} only.`);
  }
  const rejected = [];
  const take = (section, value, problem, keep = (id, v) => v) => {
    if (value === undefined) return {};
    if (!isObject(value)) {
      rejected.push({ section, id: null, reason: `${section} is not a record of entries` });
      return {};
    }
    const out = {};
    for (const [id, v] of Object.entries(value)) {
      const reason = problem(id, v);
      if (reason) rejected.push({ section, id, reason });
      else out[id] = keep(id, v);
    }
    return out;
  };
  const datasets = take('datasets', json.datasets, datasetProblem, (id, d) => ({ ...d, status: 'manual' }));
  const customCurves = take('customCurves', json.customCurves, curveProblem, (id, c) => ({ ...c, status: 'manual' }));
  const custom = take('custom', json.custom, securityProblem);
  const benchPrices = take('benchPrices', json.benchPrices, benchPriceProblem);
  const credit = take('credit', json.credit, creditProblem);
  const known = { ...(existing.customCurves || {}), ...customCurves };
  const spreadCurve = take('spreadCurve', json.spreadCurve, (currency, id) => {
    if (!isCurrency(currency)) return 'not a three-letter currency';
    const curve = typeof id === 'string' ? CURVE_DEFS[id] || known[id] : null;
    if (!curve) return `curve ${JSON.stringify(id)} is neither built in nor one of your curves`;
    return curve.currency === currency ? null : `curve ${id} is not a ${currency} curve`;
  });
  let benchmarkChoice;
  if (json.benchmarkChoice !== undefined) {
    const reason = benchmarkChoiceProblem(json.benchmarkChoice);
    if (reason) rejected.push({ section: 'benchmarkChoice', id: null, reason });
    else {
      const { mode, ids, selection } = json.benchmarkChoice;
      benchmarkChoice = { mode, ids: [...ids], selection };
    }
  }
  return { datasets, customCurves, credit, custom, benchPrices, spreadCurve, benchmarkChoice, rejected };
}
