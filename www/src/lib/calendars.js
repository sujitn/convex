// Rule-generated holiday calendars. The library never infers a calendar from
// a name, so every request carries the actual holiday table; these rules are
// how the browser produces it. Each pin names the rule set and its year span.

import { ymd, weekday, addDays, parseIso, toIso } from './dates.js';

export const CALENDAR_YEARS = [2000, 2080];
const WEEKEND = [5, 6]; // Saturday, Sunday (Monday = 0)

function easterSunday(y) {
  // Anonymous Gregorian algorithm.
  const a = y % 19;
  const b = Math.floor(y / 100);
  const c = y % 100;
  const d = Math.floor(b / 4);
  const e = b % 4;
  const f = Math.floor((b + 8) / 25);
  const g = Math.floor((b - f + 1) / 3);
  const h = (19 * a + b - d - g + 15) % 30;
  const i = Math.floor(c / 4);
  const k = c % 4;
  const l = (32 + 2 * e + 2 * i - h - k) % 7;
  const m = Math.floor((a + 11 * h + 22 * l) / 451);
  const month = Math.floor((h + l - 7 * m + 114) / 31);
  const day = ((h + l - 7 * m + 114) % 31) + 1;
  return ymd(y, month, day);
}

/** n-th (1-based) weekday `wd` (Monday = 0) of a month; n = -1 for the last. */
function nthWeekday(y, m, wd, n) {
  if (n > 0) {
    const first = ymd(y, m, 1);
    const offset = (wd - weekday(first) + 7) % 7;
    return addDays(first, offset + 7 * (n - 1));
  }
  const last = toIso(new Date(Date.UTC(y, m, 0)));
  const back = (weekday(last) - wd + 7) % 7;
  return addDays(last, -back);
}

/** Sunday → Monday; Saturday → Friday when `saturdayToFriday`, else dropped. */
function observed(iso, saturdayToFriday) {
  const wd = weekday(iso);
  if (wd === 6) return addDays(iso, 1);
  if (wd === 5) return saturdayToFriday ? addDays(iso, -1) : null;
  return iso;
}

/** Weekend → following Monday (UK/Canada substitute-day style). */
function nextMonday(iso) {
  const wd = weekday(iso);
  if (wd === 5) return addDays(iso, 2);
  if (wd === 6) return addDays(iso, 1);
  return iso;
}

function usSifma(y) {
  // A Saturday New Year's Day is not observed on the preceding Friday.
  return [
    ["New Year's Day", observed(ymd(y, 1, 1), false)],
    ['Martin Luther King Jr. Day', nthWeekday(y, 1, 0, 3)],
    ["Washington's Birthday", nthWeekday(y, 2, 0, 3)],
    ['Good Friday', addDays(easterSunday(y), -2)],
    ['Memorial Day', nthWeekday(y, 5, 0, -1)],
    ['Juneteenth', y >= 2022 ? observed(ymd(y, 6, 19), true) : null],
    ['Independence Day', observed(ymd(y, 7, 4), true)],
    ['Labor Day', nthWeekday(y, 9, 0, 1)],
    ['Columbus Day', nthWeekday(y, 10, 0, 2)],
    ['Veterans Day', observed(ymd(y, 11, 11), false)],
    ['Thanksgiving', nthWeekday(y, 11, 3, 4)],
    ['Christmas Day', observed(ymd(y, 12, 25), true)],
  ];
}

function usFedwire(y) {
  // Federal Reserve: Saturday holidays are not observed; Sunday → Monday.
  return [
    ["New Year's Day", observed(ymd(y, 1, 1), false)],
    ['Martin Luther King Jr. Day', nthWeekday(y, 1, 0, 3)],
    ["Washington's Birthday", nthWeekday(y, 2, 0, 3)],
    ['Memorial Day', nthWeekday(y, 5, 0, -1)],
    ['Juneteenth', y >= 2022 ? observed(ymd(y, 6, 19), false) : null],
    ['Independence Day', observed(ymd(y, 7, 4), false)],
    ['Labor Day', nthWeekday(y, 9, 0, 1)],
    ['Columbus Day', nthWeekday(y, 10, 0, 2)],
    ['Veterans Day', observed(ymd(y, 11, 11), false)],
    ['Thanksgiving', nthWeekday(y, 11, 3, 4)],
    ['Christmas Day', observed(ymd(y, 12, 25), false)],
  ];
}

function target2(y) {
  const easter = easterSunday(y);
  return [
    ["New Year's Day", ymd(y, 1, 1)],
    ['Good Friday', addDays(easter, -2)],
    ['Easter Monday', addDays(easter, 1)],
    ['Labour Day', ymd(y, 5, 1)],
    ['Christmas Day', ymd(y, 12, 25)],
    ["St Stephen's Day", ymd(y, 12, 26)],
  ];
}

function ukEnglandWales(y) {
  const easter = easterSunday(y);
  const xmas = ymd(y, 12, 25);
  const boxing = ymd(y, 12, 26);
  let xmasObs = xmas;
  let boxingObs = boxing;
  if (weekday(xmas) === 5) { xmasObs = ymd(y, 12, 27); boxingObs = ymd(y, 12, 28); }
  else if (weekday(xmas) === 6) { xmasObs = ymd(y, 12, 27); boxingObs = ymd(y, 12, 26); }
  else if (weekday(boxing) === 5) boxingObs = ymd(y, 12, 28);
  return [
    ["New Year's Day", nextMonday(ymd(y, 1, 1))],
    ['Good Friday', addDays(easter, -2)],
    ['Easter Monday', addDays(easter, 1)],
    ['Early May bank holiday', nthWeekday(y, 5, 0, 1)],
    ['Spring bank holiday', nthWeekday(y, 5, 0, -1)],
    ['Summer bank holiday', nthWeekday(y, 8, 0, -1)],
    ['Christmas Day', xmasObs],
    ['Boxing Day', boxingObs],
  ];
}

function canada(y) {
  const easter = easterSunday(y);
  const xmas = ymd(y, 12, 25);
  let xmasObs = nextMonday(xmas);
  let boxingObs = ymd(y, 12, 26);
  if (weekday(xmas) === 5) { xmasObs = ymd(y, 12, 27); boxingObs = ymd(y, 12, 28); }
  else if (weekday(xmas) === 6) { xmasObs = ymd(y, 12, 26); boxingObs = ymd(y, 12, 27); }
  else if (weekday(xmas) === 4) boxingObs = ymd(y, 12, 28);
  // Victoria Day: the Monday preceding 25 May.
  const may24 = ymd(y, 5, 24);
  const victoria = addDays(may24, -weekday(may24));
  return [
    ["New Year's Day", nextMonday(ymd(y, 1, 1))],
    ['Good Friday', addDays(easter, -2)],
    ['Victoria Day', victoria],
    ['Canada Day', nextMonday(ymd(y, 7, 1))],
    ['Civic Holiday', nthWeekday(y, 8, 0, 1)],
    ['Labour Day', nthWeekday(y, 9, 0, 1)],
    ['Truth and Reconciliation Day', y >= 2021 ? nextMonday(ymd(y, 9, 30)) : null],
    ['Thanksgiving', nthWeekday(y, 10, 0, 2)],
    ['Remembrance Day', nextMonday(ymd(y, 11, 11))],
    ['Christmas Day', xmasObs],
    ['Boxing Day', boxingObs],
  ];
}

export const CALENDAR_RULES = {
  'US.SIFMA': { label: 'US bond market (SIFMA full closes)', rule: usSifma },
  'US.FED': { label: 'US Federal Reserve / Fedwire', rule: usFedwire },
  'EU.TARGET2': { label: 'Euro area TARGET2', rule: target2 },
  'GB.LON': { label: 'England & Wales bank holidays', rule: ukEnglandWales },
  'CA.TOR': { label: 'Canada federal statutory (Bank of Canada)', rule: canada },
};

const cache = new Map();

/** The full `CalendarInput` the library expects, generated for `CALENDAR_YEARS`. */
export function calendarInput(code) {
  if (cache.has(code)) return cache.get(code);
  const def = CALENDAR_RULES[code];
  if (!def) throw new Error(`Unknown calendar ${code}`);
  const set = new Set();
  for (let y = CALENDAR_YEARS[0]; y <= CALENDAR_YEARS[1]; y++) {
    for (const [, d] of def.rule(y)) if (d && !WEEKEND.includes(weekday(d))) set.add(d);
  }
  const input = {
    pin: calendarPin(code),
    holidays: [...set].sort(),
    weekend_days: WEEKEND,
  };
  cache.set(code, input);
  return input;
}

/** The pin is a statement of exactly which table was supplied. */
export function calendarPin(code) {
  return { id: `${code}.rules.${CALENDAR_YEARS[0]}-${CALENDAR_YEARS[1]}`, version: 1 };
}

export function isBusinessDay(code, iso) {
  if (WEEKEND.includes(weekday(iso))) return false;
  return !calendarInput(code).holidays.includes(iso);
}

export function addBusinessDays(code, iso, n) {
  let d = iso;
  let left = n;
  while (left > 0) {
    d = addDays(d, 1);
    if (isBusinessDay(code, d)) left -= 1;
  }
  return d;
}

export function adjustFollowing(code, iso) {
  let d = iso;
  while (!isBusinessDay(code, d)) d = addDays(d, 1);
  return d;
}

export function previousBusinessDay(code, iso) {
  let d = addDays(iso, -1);
  while (!isBusinessDay(code, d)) d = addDays(d, -1);
  return d;
}

/** Named holidays for display: [{ date, name }] within one year. */
export function holidaysInYear(code, year) {
  return CALENDAR_RULES[code]
    .rule(year)
    .filter(([, d]) => d && !WEEKEND.includes(weekday(d)) && parseIso(d).getUTCFullYear() === year)
    .map(([name, date]) => ({ date, name }))
    .sort((a, b) => (a.date < b.date ? -1 : 1));
}
