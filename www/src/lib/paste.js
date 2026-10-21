// Tenor/rate tables pasted from a spreadsheet, a terminal or a web page.
// Accepts one point per line, tenor then rate. A line with a tab is split on
// tabs only and one with a semicolon on semicolons only (so a European "3,85"
// survives), keeping empty cells between values so a gap is reported; anything else is split on commas and spaces (except that a row ending in
// "space integer,digits", as in "1Y 3,85", keeps that last cell whole). Header and blank
// lines are skipped; anything else that doesn't parse is reported, never guessed.

import { decimal } from './decimal.js';
import { tenorMonths } from './dates.js';

const UNITS = { D: 'D', DAY: 'D', DAYS: 'D', W: 'W', WK: 'W', WEEK: 'W', WEEKS: 'W', M: 'M', MO: 'M', MON: 'M', MONTH: 'M',
  MONTHS: 'M', Y: 'Y', YR: 'Y', YRS: 'Y', YEAR: 'Y', YEARS: 'Y' };

// Longest tenor accepted: 100 years. Anything longer is a typo, not a curve point.
const MAX_MONTHS = 1200;

/** "10 yr", "10Y", "3 Mo", "1w", "1.5Y" → "10Y", "3M", "1W", "1.5Y"; anything else → null. */
export function normaliseTenor(text) {
  const m = /^(\d+(?:\.\d+)?)\s*([a-z]+)$/i.exec(String(text ?? '').trim());
  if (!m) return null;
  const unit = UNITS[m[2].toUpperCase()];
  if (!unit) return null;
  // Months and years may be fractional (1.5Y = 18M); days and weeks are whole.
  if (m[1].includes('.') && (unit === 'D' || unit === 'W')) return null;
  const tenor = `${decimal(m[1])}${unit}`; // the typed digits, never Number() text such as 1e+21
  const months = tenorMonths(tenor);
  // At least one day: 0.001Y would round to a zero-day maturity.
  return months > 0 && months <= MAX_MONTHS && Math.round(months * 30.4375) > 0 ? tenor : null;
}

/** A tenor's length in months, so "12M" and "1Y" (or "18M" and "1.5Y") are the same point. */
export function tenorKey(tenor) {
  return Number(tenorMonths(tenor).toFixed(9));
}

const PLAIN = /^[+-]?(?:\d+\.?\d*|\.\d+)$/;

/**
 * A rate as typed → plain decimal text, or null. Surrounding spaces and a
 * trailing % go; with both "," and "." the commas are thousands separators
 * ("1,234.5"), a single "," with no "." is a decimal comma ("3,85" → "3.85"),
 * and several commas alone are thousands separators ("1,000,000"). The digits
 * are otherwise kept exactly as typed ("3.90" stays "3.90").
 */
export function normaliseRate(text) {
  let s = String(text ?? '').trim().replace(/\s*%$/, '');
  const commas = (s.match(/,/g) || []).length;
  if (commas === 1 && !s.includes('.')) s = s.replace(',', '.');
  else if (commas) {
    if (!/^[+-]?\d{1,3}(?:,\d{3})+(?:\.\d*)?$/.test(s)) return null;
    s = s.replace(/,/g, '');
  }
  return PLAIN.test(s) ? s : null;
}

// Words that mark a column-heading line rather than a data line.
const HEADER = /\b(?:tenors?|terms?|maturity|maturities|rates?|yields?|par|zero|spot|swaps?|ois|coupon|value|mid|bid|ask|curve|date|pct|percent|laufzeit|laufzeiten|rendite|renditen|zins|zinssatz)\b/i;

function split(line, separator) {
  const out = line.split(separator).map((c) => c.trim());
  while (out.length && !out[out.length - 1]) out.pop();
  while (out.length && !out[0]) out.shift();
  return out;
}

function cells(line) {
  if (line.includes('\t')) return split(line, '\t');
  if (line.includes(';')) return split(line, ';');
  // "1Y 3,85": a space-separated row ending in integer,digits is a decimal comma.
  const eu = /^(.*[^\s,])\s+([+-]?\d+,\d+\s*%?)$/.exec(line);
  if (eu && !/,\s*$/.test(eu[1])) return [...eu[1].split(/[,\s]+/).filter(Boolean), eu[2].replace(/\s+/g, '')];
  return line.split(/[,\s]+/).filter(Boolean);
}

/** Parse pasted text → { points: [{ tenor, value }], errors: [{ line, text, reason }] }. */
export function parseTenorTable(text) {
  const byKey = new Map();
  const errors = [];
  String(text).split(/\r?\n/).forEach((raw, i) => {
    const line = raw.trim();
    if (!line || line.startsWith('#')) return;
    const fail = (reason) => errors.push({ line: i + 1, text: raw, reason });
    // "10Y 4.25", "10 yr, 4.25%", tab-separated: the tenor may be one cell or two.
    const tokens = cells(line);
    let tenor = normaliseTenor(tokens[0]);
    let rest = tokens.slice(1);
    if (!tenor && tokens.length >= 2) {
      tenor = normaliseTenor(`${tokens[0]}${tokens[1]}`);
      if (tenor) rest = tokens.slice(2);
    }
    if (!tenor) {
      if (!/^[+-]?\d/.test(line) && HEADER.test(line)) return; // column headings
      fail('no tenor such as 3M, 1.5Y or 10Y (at most 100 years)');
      return;
    }
    if (!rest.length || rest[0] === '') {
      fail('no rate after the tenor');
      return;
    }
    if (rest.length > 1) {
      fail(`unexpected extra column "${rest[1]}"`);
      return;
    }
    const value = normaliseRate(rest[0]);
    if (value === null) {
      fail('rate is not a plain decimal number');
      return;
    }
    // A later line for the same tenor (12M after 1Y) replaces the earlier one.
    const key = tenorKey(tenor);
    byKey.delete(key);
    byKey.set(key, { tenor, value });
  });
  const points = [...byKey.entries()].sort((a, b) => a[0] - b[0]).map(([, p]) => p);
  return { points, errors };
}

/** Points → tab-separated text, the inverse of parseTenorTable. */
export function formatTenorTable(points) {
  return points.map((p) => `${p.tenor}\t${p.value}`).join('\n');
}
