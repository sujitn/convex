// Tenor/rate tables pasted from a spreadsheet, a terminal or a web page.
// Accepts one point per line, tenor then rate, separated by tabs, commas,
// semicolons or spaces. Header lines and blank lines are skipped; anything
// else that doesn't parse is reported, never guessed.

import { isDecimal } from './decimal.js';
import { tenorYears } from './dates.js';

const UNITS = { D: 'D', DAY: 'D', DAYS: 'D', W: 'W', WK: 'W', WEEK: 'W', WEEKS: 'W', M: 'M', MO: 'M', MON: 'M', MONTH: 'M',
  MONTHS: 'M', Y: 'Y', YR: 'Y', YRS: 'Y', YEAR: 'Y', YEARS: 'Y' };

/** "10 yr", "10Y", "3 Mo", "1w" → "10Y", "3M", "1W"; anything else → null. */
export function normaliseTenor(text) {
  const m = /^(\d+)\s*([a-z]+)$/i.exec(String(text).trim());
  if (!m) return null;
  const unit = UNITS[m[2].toUpperCase()];
  if (!unit || Number(m[1]) <= 0) return null;
  return `${Number(m[1])}${unit}`;
}

/** Parse pasted text → { points: [{ tenor, value }], errors: [{ line, text, reason }] }. */
export function parseTenorTable(text) {
  const points = [];
  const errors = [];
  const seen = new Map();
  String(text).split(/\r?\n/).forEach((raw, i) => {
    const line = raw.trim();
    if (!line || line.startsWith('#')) return;
    // "10Y 4.25", "10 yr, 4.25%", tab-separated: the tenor may be one token or two.
    const tokens = line.split(/[,;\s]+/).filter(Boolean);
    let tenor = normaliseTenor(tokens[0]);
    let value = tokens[1];
    if (!tenor && tokens.length >= 2) {
      tenor = normaliseTenor(`${tokens[0]}${tokens[1]}`);
      value = tokens[2];
    }
    if (value) value = value.replace(/%$/, '');
    if (!tenor && !/^-?\d/.test(tokens[0])) return; // header line
    if (!tenor) {
      errors.push({ line: i + 1, text: raw, reason: 'no tenor such as 3M or 10Y' });
      return;
    }
    if (!value || !isDecimal(value)) {
      errors.push({ line: i + 1, text: raw, reason: 'rate is not a plain decimal number' });
      return;
    }
    if (seen.has(tenor)) points[seen.get(tenor)] = { tenor, value };
    else {
      seen.set(tenor, points.length);
      points.push({ tenor, value });
    }
  });
  points.sort((a, b) => tenorYears(a.tenor) - tenorYears(b.tenor));
  return { points, errors };
}

/** Points → tab-separated text, the inverse of parseTenorTable. */
export function formatTenorTable(points) {
  return points.map((p) => `${p.tenor}\t${p.value}`).join('\n');
}
