// Bond reference data pasted from Excel, a CSV file or a terminal export.
// The first line names the columns (in any order, common aliases accepted);
// each following line is one bond. Anything a row leaves out gets a stated
// market default, reported per row, so nothing is assumed silently. Rows that
// can't be read are reported with the reason, never guessed.

import { isIso, addMonths } from './dates.js';
import { isDecimal, parsePrice } from './decimal.js';
import { normaliseRate } from './paste.js';
import { isinCheck, couponShort } from '../analytics/securities.js';
import { frnSecurity, frnTicker, SOFR } from './frn.js';

const MONTHS = { JAN: 1, FEB: 2, MAR: 3, APR: 4, MAY: 5, JUN: 6, JUL: 7, AUG: 8, SEP: 9, SEPT: 9, OCT: 10, NOV: 11, DEC: 12 };

/** Columns, with the header names each is recognised by (case, spaces and punctuation ignored). */
export const BOND_COLUMNS = [
  { key: 'identifier', label: 'CUSIP or ISIN', aliases: ['cusip', 'isin', 'id', 'identifier', 'securityid', 'secid', 'figi', 'ticker', 'code'] },
  { key: 'name', label: 'Name', aliases: ['name', 'description', 'security', 'securityname', 'issuer', 'bond', 'desc'] },
  { key: 'coupon', label: 'Coupon %', required: true, aliases: ['coupon', 'cpn', 'couponrate', 'rate', 'couponpct'] },
  { key: 'maturity', label: 'Maturity', required: true, aliases: ['maturity', 'maturitydate', 'mty', 'matdate', 'maturitydt', 'redemptiondate'] },
  { key: 'issueDate', label: 'Issue date', aliases: ['issuedate', 'issue', 'issuedt', 'settledate'] },
  { key: 'datedDate', label: 'Dated date', aliases: ['dateddate', 'accrualdate', 'interestaccrualdate', 'firstaccrualdate', 'intaccrualdate'] },
  { key: 'firstCouponDate', label: 'First coupon', aliases: ['firstcoupon', 'firstcoupondate', 'firstcpn', 'firstcpndt', 'firstpaymentdate'] },
  { key: 'currency', label: 'Currency', aliases: ['currency', 'ccy', 'crncy', 'cur'] },
  { key: 'frequency', label: 'Frequency', aliases: ['frequency', 'freq', 'cpnfreq', 'couponfrequency', 'paymentfrequency'] },
  { key: 'dayCount', label: 'Day count', aliases: ['daycount', 'basis', 'dc', 'daycountbasis', 'accrualbasis', 'daycnt'] },
  { key: 'settlementDays', label: 'Settlement days', aliases: ['settlementdays', 'settledays', 'settle', 'settlement', 'settlementlag'] },
  { key: 'redemption', label: 'Redemption %', aliases: ['redemption', 'redemptionvalue', 'redemptionprice', 'principal'] },
  { key: 'price', label: 'Clean price', aliases: ['price', 'cleanprice', 'px', 'mid', 'bid', 'ask', 'last', 'midprice', 'pxlast'] },
  { key: 'calls', label: 'Calls', aliases: ['calls', 'callschedule', 'call'] },
  // A non-blank Index makes the row a floating-rate note: margin in bp over
  // compounded SOFR, with an optional lookback in business days.
  { key: 'index', label: 'Index', aliases: ['index', 'floatingindex', 'rateindex', 'refindex', 'referenceindex', 'floatindex', 'indexname', 'benchmarkindex'] },
  { key: 'spread', label: 'Spread bp', aliases: ['spread', 'margin', 'spreadbp', 'quotedmargin', 'quotedspread', 'indexspread', 'marginbp', 'floatspread'] },
  { key: 'lookback', label: 'Lookback days', aliases: ['lookback', 'lookbackdays', 'lookbackperiod', 'observationlookback'] },
];

const ALIAS = new Map(BOND_COLUMNS.flatMap((c) => c.aliases.map((a) => [a, c.key])));
const norm = (h) => String(h).toLowerCase().replace(/[^a-z0-9]/g, '');

const FREQUENCIES = new Map([
  ['1', 'Annual'], ['a', 'Annual'], ['annual', 'Annual'], ['annually', 'Annual'], ['yearly', 'Annual'], ['12m', 'Annual'],
  ['2', 'SemiAnnual'], ['s', 'SemiAnnual'], ['sa', 'SemiAnnual'], ['semi', 'SemiAnnual'], ['semiannual', 'SemiAnnual'],
  ['semiannually', 'SemiAnnual'], ['6m', 'SemiAnnual'],
  ['4', 'Quarterly'], ['q', 'Quarterly'], ['quarterly', 'Quarterly'], ['3m', 'Quarterly'],
  ['12', 'Monthly'], ['m', 'Monthly'], ['monthly', 'Monthly'], ['1m', 'Monthly'],
]);

const DAY_COUNTS = new Map([
  ['actact', 'ActActIcma'], ['actacticma', 'ActActIcma'], ['icma', 'ActActIcma'], ['actactisma', 'ActActIcma'], ['isma99', 'ActActIcma'],
  ['actactisda', 'ActActIsda'], ['isda', 'ActActIsda'],
  ['30360', 'Thirty360US'], ['30360us', 'Thirty360US'], ['bond', 'Thirty360US'], ['30u360', 'Thirty360US'], ['us30360', 'Thirty360US'],
  ['30e360', 'Thirty360E'], ['30360e', 'Thirty360E'], ['eurobond', 'Thirty360E'], ['isma30360', 'Thirty360E'],
  ['act365', 'Act365Fixed'], ['act365f', 'Act365Fixed'], ['act365fixed', 'Act365Fixed'], ['actual365fixed', 'Act365Fixed'],
  ['act360', 'Act360'], ['actual360', 'Act360'],
]);

// Market defaults applied when a column is absent or blank, each reported.
const BY_CURRENCY = {
  USD: { calendar: 'US.SIFMA', settlementDays: 1, frequency: 'SemiAnnual', dayCount: 'Thirty360US' },
  EUR: { calendar: 'EU.TARGET2', settlementDays: 2, frequency: 'Annual', dayCount: 'ActActIcma' },
  GBP: { calendar: 'GB.LON', settlementDays: 1, frequency: 'SemiAnnual', dayCount: 'ActActIcma' },
  CAD: { calendar: 'CA.TOR', settlementDays: 1, frequency: 'SemiAnnual', dayCount: 'Act365Fixed' },
};
const FREQ_MONTHS = { Annual: 12, SemiAnnual: 6, Quarterly: 3, Monthly: 1 };

// Index names a floating row may give; SOFR is the one the terminal prices.
const INDICES = new Map([['sofr', SOFR], ['usdsofr', SOFR], ['compoundedsofr', SOFR], ['sofrcompounded', SOFR],
  ['sofrcompoundedinarrears', SOFR], ['sofrrate', SOFR], ['sofrindex', SOFR]]);

/** Margin in basis points: "85", "+85", "85 bp", "-12.5bps". Percent is refused, not converted. */
function parseSpreadBp(text) {
  const t = String(text || '').trim().replace(/\s*bps?$/i, '').replace(/^\+/, '').replace(/\s/g, '');
  return !/%/.test(t) && isDecimal(t) ? t : null;
}

/** CUSIP check digit (modulus 10 "double add double"). */
export function cusipValid(id) {
  if (!/^[0-9A-Z*@#]{8}[0-9]$/.test(id)) return false;
  let sum = 0;
  for (let i = 0; i < 8; i++) {
    const c = id[i];
    let v = /\d/.test(c) ? Number(c) : c === '*' ? 36 : c === '@' ? 37 : c === '#' ? 38 : c.charCodeAt(0) - 55;
    if (i % 2 === 1) v *= 2;
    sum += Math.floor(v / 10) + (v % 10);
  }
  return (10 - (sum % 10)) % 10 === Number(id[8]);
}

export function isinValid(id) {
  return /^[A-Z]{2}[0-9A-Z]{9}[0-9]$/.test(id) && isinCheck(id.slice(0, 11)) === id[11];
}

/** Split lines on tabs (Excel), semicolons or commas, honouring CSV quotes. */
function splitLine(line, sep) {
  if (sep === '\t') return line.split('\t').map((c) => c.trim());
  const out = [];
  let cell = '';
  let quoted = false;
  for (let i = 0; i < line.length; i++) {
    const ch = line[i];
    if (quoted) {
      if (ch === '"' && line[i + 1] === '"') { cell += '"'; i++; } else if (ch === '"') quoted = false; else cell += ch;
    } else if (ch === '"') quoted = true;
    else if (ch === sep) { out.push(cell.trim()); cell = ''; } else cell += ch;
  }
  out.push(cell.trim());
  return out;
}

function detectSeparator(header) {
  if (header.includes('\t')) return '\t';
  if (header.includes(';')) return ';';
  return ',';
}

const pad = (n) => String(n).padStart(2, '0');
function ymd(y, m, d) {
  const iso = `${String(y).padStart(4, '0')}-${pad(m)}-${pad(d)}`;
  return isIso(iso) ? iso : null;
}
const fullYear = (y) => (y.length === 2 ? 2000 + Number(y) - (Number(y) > 70 ? 100 : 0) : Number(y));

/**
 * A date as typed or copied from a sheet: 2034-03-15, 03/15/2034, 15/03/2034
 * (dayFirst), 15-Mar-2034, 15 Mar 34, Mar 15, 2034, 20340315 or an Excel
 * serial day number. Returns ISO or null.
 */
export function parseDate(text, { dayFirst = false } = {}) {
  const s = String(text || '').trim();
  if (!s) return null;
  let m = /^(\d{4})-(\d{1,2})-(\d{1,2})$/.exec(s);
  if (m) return ymd(+m[1], +m[2], +m[3]);
  m = /^(\d{4})(\d{2})(\d{2})$/.exec(s);
  if (m) return ymd(+m[1], +m[2], +m[3]);
  m = /^(\d{1,2})[/.-](\d{1,2})[/.-](\d{2}|\d{4})$/.exec(s);
  if (m) return dayFirst ? ymd(fullYear(m[3]), +m[2], +m[1]) : ymd(fullYear(m[3]), +m[1], +m[2]);
  m = /^(\d{1,2})[\s-]([A-Za-z]{3,4})[a-z]*[\s-](\d{2}|\d{4})$/.exec(s);
  if (m && MONTHS[m[2].toUpperCase()]) return ymd(fullYear(m[3]), MONTHS[m[2].toUpperCase()], +m[1]);
  m = /^([A-Za-z]{3,4})[a-z]*\.?\s+(\d{1,2}),?\s+(\d{4})$/.exec(s);
  if (m && MONTHS[m[1].toUpperCase()]) return ymd(+m[3], MONTHS[m[1].toUpperCase()], +m[2]);
  if (/^\d{5}$/.test(s)) {
    // Excel serial: day 1 is 1900-01-01, with the 1900 leap-year bug.
    const n = Number(s);
    if (n > 60 && n < 80000) {
      const d = new Date(Date.UTC(1899, 11, 30) + n * 86400000);
      return ymd(d.getUTCFullYear(), d.getUTCMonth() + 1, d.getUTCDate());
    }
  }
  return null;
}

/** Whether slash dates in the date columns read day-first (any first part above 12). */
function guessDayFirst(rows, dateIndexes) {
  for (const cells of rows) {
    for (const i of dateIndexes) {
      const m = /^(\d{1,2})[/.](\d{1,2})[/.]\d{2,4}$/.exec(cells[i] || '');
      if (m && +m[1] > 12) return true;
      if (m && +m[2] > 12) return false;
    }
  }
  return false;
}

// "2,5" is a decimal comma (European Excel), "1,000.5" has thousands
// separators; any other comma form is refused rather than guessed.
function cleanDecimal(text) {
  const t = normaliseRate(String(text || '').replace(/\s/g, ''));
  return t !== null && isDecimal(t) ? t : null;
}

/** A clean price as typed: 32nds pass through, decimals go through the comma rules. */
function cleanPrice(text) {
  const s = String(text || '').trim();
  if (/^\d+-\d{2}[+0-7]?$/.test(s)) return s;
  return cleanDecimal(s);
}

/** "2027-03-15@102; 03/15/2028 @ 101" → [{ date, price }]. */
function parseCalls(text, opts) {
  const out = [];
  for (const part of String(text || '').split(/[;|]/).map((p) => p.trim()).filter(Boolean)) {
    const m = /^(.+?)\s*@\s*([\d.]+)$/.exec(part);
    const date = m && parseDate(m[1], opts);
    if (!m || !date || !isDecimal(m[2])) throw new Error(`call "${part}" is not date@price`);
    out.push({ date, price: m[2] });
  }
  return out.sort((a, b) => (a.date < b.date ? -1 : 1));
}

/** Last regular coupon date on or before `date`, stepping back from maturity. */
function couponOnOrBefore(maturity, months, date) {
  let d = maturity;
  for (let k = 1; d > date && k < 2000; k++) d = addMonths(maturity, -k * months);
  return d;
}

function slug(text) {
  return String(text).toUpperCase().replace(/[^A-Z0-9]+/g, '-').replace(/^-+|-+$/g, '').slice(0, 40) || 'BOND';
}

/**
 * Parse pasted bond rows. Returns { columns, unknown, dayFirst, rows, errors }
 * where each row is { line, sec, assumed: [field labels], warnings }.
 * `tradeDate` anchors the default issue date when none is given.
 */
export function parseBondTable(text, { tradeDate, dayFirst: forced } = {}) {
  const lines = String(text || '').split(/\r?\n/).map((l, i) => ({ text: l, line: i + 1 })).filter((l) => l.text.trim());
  if (!lines.length) return { columns: [], unknown: [], rows: [], errors: [] };
  const sep = detectSeparator(lines[0].text);
  const header = splitLine(lines[0].text, sep);
  const columns = header.map((h) => ALIAS.get(norm(h)) || null);
  const unknown = header.filter((h, i) => h && !columns[i]);
  // A table of floaters names Index (and Spread) instead of Coupon.
  const floaters = columns.includes('index');
  const missing = BOND_COLUMNS.filter((c) => c.required && !columns.includes(c.key) && !(c.key === 'coupon' && floaters))
    .map((c) => c.label);
  if (missing.length) {
    const hint = missing.includes('Coupon %') ? ' (a floating-rate note names Index and Spread instead)' : '';
    return { columns, unknown, rows: [], errors: [{ line: lines[0].line, text: lines[0].text,
      reason: `the first line must name the columns; ${missing.join(' and ')} not found${hint}` }] };
  }
  const body = lines.slice(1).map((l) => ({ ...l, cells: splitLine(l.text, sep) }));
  const dateIdx = columns.map((c, i) => (['maturity', 'issueDate', 'datedDate', 'firstCouponDate'].includes(c) ? i : -1)).filter((i) => i >= 0);
  const dayFirst = forced ?? guessDayFirst(body.map((b) => b.cells), dateIdx);
  const rows = [];
  const errors = [];
  const ids = new Set();
  for (const { line, text: raw, cells } of body) {
    const get = (key) => {
      const i = columns.indexOf(key);
      return i >= 0 && cells[i] !== undefined ? cells[i].trim() : '';
    };
    const fail = (reason) => errors.push({ line, text: raw, reason });
    const assumed = [];
    const warnings = [];
    const floating = !!get('index');
    let index;
    let spreadBp;
    let coupon = null;
    if (floating) {
      index = INDICES.get(norm(get('index')));
      if (!index) { fail(`index "${get('index')}" is not supported; floating rows take SOFR (compounded in arrears)`); continue; }
      spreadBp = parseSpreadBp(get('spread'));
      if (spreadBp === null) { fail(`spread "${get('spread')}" is not a margin in basis points (e.g. 85)`); continue; }
      if (get('coupon')) warnings.push(`coupon ${get('coupon')} ignored: the row names an index`);
    } else {
      coupon = cleanDecimal(get('coupon'));
      if (coupon === null) { fail(`coupon "${get('coupon')}" is not a number`); continue; }
    }
    const maturity = parseDate(get('maturity'), { dayFirst });
    if (!maturity) { fail(`maturity "${get('maturity')}" is not a date`); continue; }
    const currency = (get('currency') || 'USD').toUpperCase();
    if (!get('currency')) assumed.push('currency USD');
    const d = BY_CURRENCY[currency];
    if (!d) { fail(`currency ${currency} has no calendar here (USD, EUR, GBP, CAD)`); continue; }
    if (floating && currency !== 'USD') { fail(`a SOFR floating-rate note pays USD, not ${currency}`); continue; }

    const idText = get('identifier').toUpperCase().replace(/\s/g, '');
    const cusip = cusipValid(idText) ? idText : undefined;
    const isin = !cusip && isinValid(idText) ? idText : undefined;
    if (idText && !cusip && !isin) warnings.push(`${idText} is not a valid CUSIP or ISIN; kept as a label`);

    let frequency = floating ? 'Quarterly' : d.frequency;
    if (get('frequency')) {
      frequency = FREQUENCIES.get(norm(get('frequency')));
      if (!frequency) { fail(`frequency "${get('frequency')}" not recognised (annual, semi-annual, quarterly, monthly or 1/2/4/12)`); continue; }
    } else assumed.push(`${frequency === 'SemiAnnual' ? 'semi-annual' : frequency.toLowerCase()} coupons`);

    // Treasuries (CUSIP 912...) and gilts accrue Act/Act ICMA; corporates 30/360.
    let dayCount = floating ? 'Act360' : cusip && cusip.startsWith('912') ? 'ActActIcma' : d.dayCount;
    if (get('dayCount')) {
      dayCount = DAY_COUNTS.get(norm(get('dayCount')));
      if (!dayCount) { fail(`day count "${get('dayCount')}" not recognised (e.g. ACT/ACT, 30/360, 30E/360, ACT/365, ACT/360)`); continue; }
    } else assumed.push(`day count ${dayCount.replace('Thirty360US', '30/360').replace('ActActIcma', 'Act/Act ICMA').replace('Act365Fixed', 'Act/365F').replace('Act360', 'Act/360')}`);

    let lookbackDays = 2;
    if (floating) {
      if (get('lookback')) {
        lookbackDays = Number(get('lookback').replace(/\s*(bd|days?|business days?)$/i, ''));
        if (!Number.isInteger(lookbackDays) || lookbackDays < 0 || lookbackDays > 10) { fail(`lookback "${get('lookback')}" is not 0 to 10 business days`); continue; }
      } else assumed.push('2-day lookback');
      assumed.push('observation shift, no lockout, paid on the period end (Modified Following)');
    }

    let settlementDays = d.settlementDays;
    if (get('settlementDays')) {
      settlementDays = Number(get('settlementDays').replace(/^T\+/i, ''));
      if (!Number.isInteger(settlementDays) || settlementDays < 0 || settlementDays > 10) { fail(`settlement days "${get('settlementDays')}" is not 0 to 10`); continue; }
    } else assumed.push(`T+${settlementDays}`);

    const dates = {};
    let bad = null;
    for (const key of ['issueDate', 'datedDate', 'firstCouponDate']) {
      if (!get(key)) continue;
      dates[key] = parseDate(get(key), { dayFirst });
      if (!dates[key]) bad = `${BOND_COLUMNS.find((c) => c.key === key).label.toLowerCase()} "${get(key)}" is not a date`;
    }
    if (bad) { fail(bad); continue; }
    let issueDate = dates.issueDate || dates.datedDate;
    if (!issueDate) {
      // Without an issue date the bond is taken as regular: accrual from the
      // last coupon date on or before the trade date.
      issueDate = couponOnOrBefore(maturity, FREQ_MONTHS[frequency], tradeDate || maturity);
      assumed.push('issue date = last coupon date (regular schedule)');
    }
    if (!(maturity > issueDate)) { fail('maturity must be after the issue date'); continue; }

    const redemption = get('redemption') ? cleanDecimal(get('redemption')) : '100';
    if (redemption === null) { fail(`redemption "${get('redemption')}" is not a number`); continue; }

    let price;
    if (get('price')) {
      try {
        const text = cleanPrice(get('price'));
        if (text === null || !(parsePrice(text) > 0)) throw new Error();
        price = text;
      } catch {
        fail(`price "${get('price')}" is not a clean price (decimal or 32nds such as 99-16+)`);
        continue;
      }
    }
    let calls = [];
    if (get('calls')) {
      try {
        calls = parseCalls(get('calls'), { dayFirst });
      } catch (e) {
        fail(e.message);
        continue;
      }
    }

    const label = cusip || isin || idText || (floating ? `FRN ${maturity}` : `${couponShort(coupon)} ${maturity}`);
    let id = `IMP-${slug(label)}`;
    for (let n = 2; ids.has(id); n++) id = `IMP-${slug(label)}-${n}`;
    ids.add(id);
    const short = (idText && !cusip && !isin ? idText : (get('name') || currency).split(/\s+/)[0]).toUpperCase().slice(0, 10);
    if (floating) {
      const sec = frnSecurity({
        id, source: 'custom', imported: true, issuer: 'Imported from your table',
        name: get('name') || `${currency} SOFR + ${spreadBp} FRN ${maturity}${calls.length ? ', callable' : ''}`,
        ticker: frnTicker(short, maturity), cusip, isin, currency, index, spreadBp, lookbackDays,
        issueDate, datedDate: dates.datedDate || issueDate, firstCouponDate: dates.firstCouponDate || null, maturity,
        frequency, dayCount, settlementDays, calendar: d.calendar, redemption, calls, ...(price ? { price } : {}),
      });
      rows.push({ line, sec, assumed, warnings });
      continue;
    }
    const name = get('name') || `${currency} ${coupon}% ${maturity}${calls.length ? ', callable' : ''}`;
    const sec = {
      id,
      source: 'custom',
      imported: true,
      kind: 'fixed',
      issuer: 'Imported from your table',
      name,
      ticker: `${short} ${couponShort(coupon)} ${maturity.slice(5, 7)}/${maturity.slice(8, 10)}/${maturity.slice(2, 4)}`,
      cusip,
      isin,
      currency,
      coupon,
      issueDate,
      datedDate: dates.datedDate || issueDate,
      firstCouponDate: dates.firstCouponDate || null,
      maturity,
      frequency,
      dayCount,
      face: '100',
      calendar: d.calendar,
      settlementDays,
      paymentAdjustment: 'Following',
      eom: 'auto',
      redemption,
      calls,
      ...(price ? { price } : {}),
    };
    rows.push({ line, sec, assumed, warnings });
  }
  return { columns, unknown, dayFirst, rows, errors };
}

/** Header plus rows, tab-separated, ready to paste into Excel and back. */
export function bondTemplate(secs = []) {
  const head = ['CUSIP', 'Name', 'Coupon', 'Maturity', 'Issue Date', 'Currency', 'Frequency', 'Day Count', 'Price'];
  const freq = { Annual: 'Annual', SemiAnnual: 'Semi-annual', Quarterly: 'Quarterly', Monthly: 'Monthly' };
  const dc = { ActActIcma: 'ACT/ACT', Thirty360US: '30/360', Thirty360E: '30E/360', Act365Fixed: 'ACT/365', Act360: 'ACT/360', ActActIsda: 'ACT/ACT ISDA' };
  // Floating-rate notes add their index columns; fixed rows leave them blank.
  const floaters = secs.some((s) => s.kind === 'floating');
  const rows = secs.map((s) => {
    const floating = s.kind === 'floating';
    const row = [s.cusip || s.isin || '', s.name, floating ? '' : s.coupon, s.maturity, s.issueDate, s.currency,
      freq[s.frequency] || s.frequency, dc[s.dayCount] || s.dayCount, s.price || ''];
    return floaters ? [...row, floating ? 'SOFR' : '', floating ? s.spreadBp : '', floating ? s.lookbackDays : ''] : row;
  });
  return [floaters ? [...head, 'Index', 'Spread', 'Lookback'] : head, ...rows].map((r) => r.join('\t')).join('\n');
}
