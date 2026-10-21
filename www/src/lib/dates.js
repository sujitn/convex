// ISO calendar-date arithmetic on UTC midnights. Dates cross the library
// boundary as "YYYY-MM-DD" strings; nothing here depends on the local zone.

const DAY_MS = 86400000;

// Date.UTC reads years 0-99 as 1900-1999; setUTCFullYear takes the year as
// given. Month and day overflow still roll over (day 0 = last of the month before).
function utc(y, monthIndex, day) {
  const d = new Date(0);
  d.setUTCFullYear(y, monthIndex, day);
  return d;
}

export function parseIso(iso) {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(typeof iso === 'string' ? iso : '');
  if (!m) throw new Error(`Invalid date "${iso}" (expected YYYY-MM-DD)`);
  const [y, mo, day] = [+m[1], +m[2], +m[3]];
  const d = utc(y, mo - 1, day);
  if (d.getUTCFullYear() !== y || d.getUTCMonth() !== mo - 1 || d.getUTCDate() !== day) {
    throw new Error(`Invalid date "${iso}"`);
  }
  return d;
}

export function toIso(d) {
  return d.toISOString().slice(0, 10);
}

export function isIso(iso) {
  try {
    parseIso(iso);
    return true;
  } catch {
    return false;
  }
}

export function ymd(y, m, d) {
  return toIso(utc(y, m - 1, d));
}

export function addDays(iso, n) {
  return toIso(new Date(parseIso(iso).getTime() + n * DAY_MS));
}

/** Calendar-month shift with end-of-month clamping (Jan 31 + 1M = Feb 28/29). */
export function addMonths(iso, n) {
  const d = parseIso(iso);
  const y = d.getUTCFullYear();
  const m = d.getUTCMonth() + n;
  const target = utc(y, m, 1);
  const last = daysInMonth(target.getUTCFullYear(), target.getUTCMonth() + 1);
  target.setUTCDate(Math.min(d.getUTCDate(), last));
  return toIso(target);
}

export function addYears(iso, n) {
  return addMonths(iso, 12 * n);
}

export function daysInMonth(y, m) {
  return utc(y, m, 0).getUTCDate();
}

export function daysBetween(a, b) {
  return Math.round((parseIso(b) - parseIso(a)) / DAY_MS);
}

/** Monday = 0 … Sunday = 6, the library's weekend numbering. */
export function weekday(iso) {
  return (parseIso(iso).getUTCDay() + 6) % 7;
}

export function compareIso(a, b) {
  return a < b ? -1 : a > b ? 1 : 0;
}

export function todayIso() {
  const now = new Date();
  return ymd(now.getFullYear(), now.getMonth() + 1, now.getDate());
}

/** Year fraction on an Act/365F clock, for chart axes and tenor labels only. */
export function yearsBetween(a, b) {
  return daysBetween(a, b) / 365;
}

/** "3M", "10Y", "1.5Y" → months. */
export function tenorMonths(tenor) {
  const m = /^(\d+(?:\.\d+)?)\s*([DWMY])$/i.exec(String(tenor).trim());
  if (!m) throw new Error(`Invalid tenor "${tenor}"`);
  const n = parseFloat(m[1]);
  switch (m[2].toUpperCase()) {
    case 'D': return n / 30.4375;
    case 'W': return (n * 7) / 30.4375;
    case 'M': return n;
    default: return n * 12;
  }
}

export function tenorYears(tenor) {
  return tenorMonths(tenor) / 12;
}

export function addTenor(iso, tenor) {
  const months = tenorMonths(tenor);
  if (Number.isInteger(months)) return addMonths(iso, months);
  const m = /^(\d+)\s*([DW])$/i.exec(String(tenor).trim());
  if (m) return addDays(iso, +m[1] * (m[2].toUpperCase() === 'W' ? 7 : 1));
  return addDays(iso, Math.round(months * 30.4375));
}

const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];

/** "2031-10-15" → "15 Oct 2031". */
export function formatLong(iso) {
  if (!iso) return '';
  const d = parseIso(iso);
  return `${d.getUTCDate()} ${MONTHS[d.getUTCMonth()]} ${d.getUTCFullYear()}`;
}

/** "2031-10-15" → "10/15/31", the security-description style. */
export function formatShort(iso) {
  if (!iso) return '';
  const [y, m, d] = iso.split('-');
  return `${m}/${d}/${y.slice(2)}`;
}
