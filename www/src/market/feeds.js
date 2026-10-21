// Free, keyless public market-data feeds: fetchers plus pure parsers.
// Browser-safe (no DOMParser, no Node APIs). Rates stay as the exact source
// text (percent); nothing is round-tripped through Number. Loaders throw on
// failure and never fall back silently; the caller decides about the snapshot.
//
// Not CORS-enabled (need a user-supplied proxy such as
// "https://my-proxy.example/?url={url}"): Bank of England IADB, Japan MoF.
// FRED (https://fred.stlouisfed.org/graph/fredgraph.csv?id=DGS10) is also not
// CORS-enabled and is deliberately not implemented. Bank of Canada's
// `tbill_all` group mixes stale and current series, so it is skipped too.

const UST_XML = 'https://home.treasury.gov/resource-center/data-chart-center/interest-rates/pages/xml';
const FISCAL_AUCTIONS = 'https://api.fiscaldata.treasury.gov/services/api/fiscal_service/v1/accounting/od/auctions_query';
const NYFED = 'https://markets.newyorkfed.org/api/rates';
const ECB = 'https://data-api.ecb.europa.eu/service/data';
const BOC = 'https://www.bankofcanada.ca/valet';
const BOE_IADB = 'https://www.bankofengland.co.uk/boeapps/database/_iadb-fromshowcolumns.asp';
const MOF_JGB = 'https://www.mof.go.jp/english/policy/jgbs/reference/interest_rate/jgbcme.csv';

const ECB_TENORS = 'SR_3M+SR_6M+SR_1Y+SR_2Y+SR_3Y+SR_5Y+SR_7Y+SR_10Y+SR_15Y+SR_20Y+SR_30Y';
const ECB_PARAMS = 'BETA0+BETA1+BETA2+BETA3+TAU1+TAU2';
const ecbCurveUrl = (issuers, series) =>
  `${ECB}/YC/B.U2.EUR.4F.${issuers}.SV_C_YM.${series}?lastNObservations=1&format=csvdata`;
const ECB_ESTR_URL = `${ECB}/EST/B.EU000A2X2A25.WT?lastNObservations=1&format=csvdata`;
const BOC_BENCH_URL = `${BOC}/observations/group/bond_yields_benchmark/json?recent=1`;
const BOC_CORRA_URL = `${BOC}/observations/AVG.INTWO/json?recent=1`;

// ---------------------------------------------------------------- helpers

const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
const pad2 = (n) => String(n).padStart(2, '0');
const nullish = (v) => v == null || v === '' || v === 'null';
const text = (v) => (nullish(v) ? null : String(v).trim());
const isoDay = (s) => (nullish(s) ? null : String(s).slice(0, 10));

function stamp(now) {
  return now.toISOString().replace(/\.\d{3}Z$/, 'Z');
}

/** Wrap a URL with the user's proxy template; only for corsRequired sources. */
export function proxied(url, proxy) {
  if (typeof proxy !== 'string' || !proxy.includes('{url}')) {
    throw new Error('This source is not CORS-enabled: supply a proxy URL template containing {url}');
  }
  return proxy.replace('{url}', encodeURIComponent(url));
}

async function getText(name, url, { fetchImpl = globalThis.fetch, proxy = null, signal, corsRequired = false } = {}) {
  if (typeof fetchImpl !== 'function') throw new Error(`${name}: no fetch implementation available`);
  let target = url;
  if (corsRequired) {
    try {
      target = proxied(url, proxy);
    } catch (e) {
      throw new Error(`${name}: ${e.message}`);
    }
  }
  let res;
  try {
    res = await fetchImpl(target, { signal });
  } catch (e) {
    if (e?.name === 'AbortError') throw e;
    throw new Error(`${name}: network request failed (${e?.message || e})`);
  }
  if (!res.ok) throw new Error(`${name}: HTTP ${res.status} ${res.statusText || ''}`.trim());
  return res.text();
}

function parseJson(name, body) {
  try {
    return typeof body === 'string' ? JSON.parse(body) : body;
  } catch (e) {
    throw new Error(`${name}: response is not valid JSON (${e.message})`);
  }
}

/** Quote bare JSON numbers so JSON.parse keeps their exact source text (3.80 stays "3.80"). */
function parseJsonKeepNumbers(name, body) {
  return parseJson(name, body.replace(/(:\s*)(-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)(?=\s*[,}\]])/g, '$1"$2"'));
}

function dataset(fields, opts, now) {
  return {
    id: fields.id,
    label: fields.label,
    currency: fields.currency,
    kind: fields.kind,
    ...(fields.compounding ? { compounding: fields.compounding } : {}),
    asOf: fields.asOf,
    fetchedAt: stamp(now),
    status: 'live',
    source: { name: fields.sourceName, url: fields.url, corsRequired: !!fields.corsRequired },
    ...(fields.points ? { points: fields.points } : {}),
    ...(fields.securities ? { securities: fields.securities } : {}),
    ...(fields.extra ? { extra: fields.extra } : {}),
  };
}

/** Tenor like "18M", "6W", "10Y" -> approximate years, for ordering only. */
export function tenorYears(tenor) {
  const m = /^(\d+(?:\.\d+)?)([DWMY])$/.exec(tenor);
  if (!m) return NaN;
  return +m[1] * { D: 1 / 365, W: 7 / 365, M: 1 / 12, Y: 1 }[m[2]];
}

const byTenor = (a, b) => tenorYears(a.tenor) - tenorYears(b.tenor);

// ------------------------------------------------------- US Treasury (XML)

const UST_PAR_FIELDS = {
  BC_1MONTH: '1M', BC_1_5MONTH: '6W', BC_2MONTH: '2M', BC_3MONTH: '3M', BC_4MONTH: '4M',
  BC_6MONTH: '6M', BC_1YEAR: '1Y', BC_2YEAR: '2Y', BC_3YEAR: '3Y', BC_5YEAR: '5Y',
  BC_7YEAR: '7Y', BC_10YEAR: '10Y', BC_20YEAR: '20Y', BC_30YEAR: '30Y',
};

/** Each <m:properties> block of the OData Atom feed as a { NAME: text } map. */
export function parseOdataEntries(xml) {
  const entries = [];
  const blocks = /<m:properties>([\s\S]*?)<\/m:properties>/g;
  let b;
  while ((b = blocks.exec(xml))) {
    const rec = {};
    const fields = /<d:([A-Za-z0-9_]+)(?:\s[^>]*)?>([^<]*)<\/d:\1>/g;
    let f;
    while ((f = fields.exec(b[1]))) rec[f[1]] = f[2].trim();
    entries.push(rec);
  }
  return entries;
}

const latestBy = (entries, key) =>
  entries.filter((e) => e[key]).reduce((best, e) => (!best || e[key] > best[key] ? e : best), null);

/** Latest daily par yield curve in the feed, or null when the feed has no entries. */
export function parseUstParXml(xml) {
  const row = latestBy(parseOdataEntries(xml), 'NEW_DATE');
  if (!row) return null;
  const points = [];
  for (const [field, tenor] of Object.entries(UST_PAR_FIELDS)) {
    if (!nullish(row[field])) points.push({ tenor, value: row[field] });
  }
  return { asOf: isoDay(row.NEW_DATE), points: points.sort(byTenor) };
}

const BILL_WEEKS = [4, 6, 8, 13, 17, 26, 52];

/**
 * Latest daily bill rates: points are coupon-equivalent yields; discount-basis
 * rates, CUSIPs and maturities of the on-the-run bills ride in `extra`.
 */
export function parseUstBillRatesXml(xml) {
  const row = latestBy(parseOdataEntries(xml), 'INDEX_DATE');
  if (!row) return null;
  const points = [];
  const discount = {};
  const cusip = {};
  const maturity = {};
  for (const w of BILL_WEEKS) {
    const tenor = `${w}W`;
    const y = row[`ROUND_B1_YIELD_${w}WK_2`];
    if (nullish(y)) continue;
    points.push({ tenor, value: y });
    if (!nullish(row[`ROUND_B1_CLOSE_${w}WK_2`])) discount[tenor] = row[`ROUND_B1_CLOSE_${w}WK_2`];
    if (!nullish(row[`CUSIP_${w}WK`])) cusip[tenor] = row[`CUSIP_${w}WK`];
    if (!nullish(row[`MATURITY_DATE_${w}WK`])) maturity[tenor] = isoDay(row[`MATURITY_DATE_${w}WK`]);
  }
  return { asOf: isoDay(row.INDEX_DATE), points, extra: { discount, cusip, maturity } };
}

const monthKey = (now, back = 0) => {
  const d = new Date(Date.UTC(now.getUTCFullYear(), now.getUTCMonth() - back, 1));
  return `${d.getUTCFullYear()}${pad2(d.getUTCMonth() + 1)}`;
};

/** Newest non-empty month of an OData feed; falls back one month at month start. */
async function loadUstMonthly(name, data, parse, { now = new Date(), ...opts }) {
  for (let back = 0; back < 2; back++) {
    const url = `${UST_XML}?data=${data}&field_tdr_date_value_month=${monthKey(now, back)}`;
    const parsed = parse(await getText(name, url, opts));
    if (parsed && parsed.points.length) return { ...parsed, url, now };
  }
  throw new Error(`${name}: no data found for the current or previous month`);
}

export async function loadUstPar(opts = {}) {
  const name = 'US Treasury par yield curve';
  const r = await loadUstMonthly(name, 'daily_treasury_yield_curve', parseUstParXml, opts);
  return dataset({
    id: 'ust-par', label: name, currency: 'USD', kind: 'par', compounding: 'SemiAnnual',
    asOf: r.asOf, sourceName: 'U.S. Department of the Treasury', url: r.url, points: r.points,
  }, opts, opts.now ?? new Date());
}

export async function loadUstBillRates(opts = {}) {
  const name = 'US Treasury daily bill rates';
  const r = await loadUstMonthly(name, 'daily_treasury_bill_rates', parseUstBillRatesXml, opts);
  return dataset({
    id: 'ust-bills', label: 'US Treasury bill rates (coupon-equivalent)', currency: 'USD', kind: 'par',
    compounding: 'Simple', asOf: r.asOf, sourceName: 'U.S. Department of the Treasury', url: r.url,
    points: r.points, extra: r.extra,
  }, opts, opts.now ?? new Date());
}

// -------------------------------------------- Treasury auctions (FiscalData)

const FREQ = { 'Semi-Annual': 'SemiAnnual', Annual: 'Annual', Quarterly: 'Quarterly', Monthly: 'Monthly' };

/** Nominal, already-held Bill/Note/Bond auctions, newest first. Accepts JSON text or object. */
export function parseAuctions(json) {
  const rows = parseJson('Treasury auctions', json)?.data;
  if (!Array.isArray(rows)) throw new Error('Treasury auctions: response has no "data" array');
  const out = [];
  for (const r of rows) {
    if (!['Bill', 'Note', 'Bond'].includes(r.security_type)) continue;
    if (r.inflation_index_security === 'Yes' || r.floating_rate === 'Yes') continue;
    if (r.cash_management_bill_cmb === 'Yes') continue;
    if (nullish(r.price_per100)) continue; // not yet auctioned
    const isBill = r.security_type === 'Bill';
    out.push({
      cusip: r.cusip,
      type: r.security_type,
      term: r.security_term,
      auctionDate: isoDay(r.auction_date),
      issueDate: isoDay(r.issue_date),
      datedDate: isoDay(r.dated_date),
      maturity: isoDay(r.maturity_date),
      firstCouponDate: isoDay(r.first_int_payment_date),
      frequency: FREQ[r.int_payment_frequency] ?? null,
      coupon: isBill ? null : text(r.int_rate),
      highYield: text(r.high_yield),
      highDiscountRate: text(r.high_discnt_rate),
      highInvestmentRate: text(r.high_investment_rate),
      pricePer100: text(r.price_per100),
      reopening: r.reopening === 'Yes',
    });
  }
  return out;
}

export async function loadTreasuryAuctions({ size = 200, ...opts } = {}) {
  const name = 'US Treasury auctions';
  const filter = 'security_type:in:(Bill,Note,Bond),inflation_index_security:eq:No,floating_rate:eq:No,cash_management_bill_cmb:eq:No';
  const url = `${FISCAL_AUCTIONS}?sort=-auction_date&page%5Bsize%5D=${size}&filter=${filter}`;
  const securities = parseAuctions(await getText(name, url, opts));
  if (!securities.length) throw new Error(`${name}: no completed nominal auctions returned`);
  return dataset({
    id: 'ust-auctions', label: 'US Treasury recent auctions (nominal)', currency: 'USD', kind: 'securities',
    asOf: securities.reduce((m, s) => (s.auctionDate > m ? s.auctionDate : m), ''),
    sourceName: 'U.S. Treasury FiscalData', url, securities,
  }, opts, new Date());
}

// ------------------------------------------------------------- NY Fed rates

const NYFED_LABELS = {
  SOFR: 'Secured Overnight Financing Rate',
  EFFR: 'Effective Federal Funds Rate',
  OBFR: 'Overnight Bank Funding Rate',
  TGCR: 'Tri-Party General Collateral Rate',
  BGCR: 'Broad General Collateral Rate',
};

const num = (v) => (nullish(v) ? undefined : Number(v));

function nyfedPoint(r) {
  const extra = {
    volumeBn: num(r.volumeInBillions),
    p1: text(r.percentPercentile1), p25: text(r.percentPercentile25),
    p75: text(r.percentPercentile75), p99: text(r.percentPercentile99),
    targetFrom: text(r.targetRateFrom), targetTo: text(r.targetRateTo),
  };
  for (const k of Object.keys(extra)) if (extra[k] == null) delete extra[k];
  return {
    tenor: 'ON', id: r.type, label: NYFED_LABELS[r.type] ?? r.type,
    value: text(r.percentRate), date: isoDay(r.effectiveDate), extra,
  };
}

function nyfedRows(body) {
  const rows = parseJsonKeepNumbers('NY Fed rates', body)?.refRates;
  if (!Array.isArray(rows)) throw new Error('NY Fed rates: response has no "refRates" array');
  return rows;
}

/** Latest fixings (SOFR, EFFR, OBFR, TGCR, BGCR); SOFR averages and index go in `extra`. */
export function parseNyFedRates(body) {
  const rows = nyfedRows(body);
  const points = rows.filter((r) => NYFED_LABELS[r.type] && !nullish(r.percentRate)).map(nyfedPoint);
  const ai = rows.find((r) => r.type === 'SOFRAI');
  const sofrAverages = ai && {
    date: isoDay(ai.effectiveDate), average30day: text(ai.average30day),
    average90day: text(ai.average90day), average180day: text(ai.average180day), index: text(ai.index),
  };
  return { points, sofrAverages };
}

/** SOFR history, one point per date, oldest first (newest last). */
export function parseSofrHistory(body) {
  return nyfedRows(body)
    .filter((r) => r.type === 'SOFR' && !nullish(r.percentRate))
    .map(nyfedPoint)
    .sort((a, b) => (a.date < b.date ? -1 : a.date > b.date ? 1 : 0));
}

export async function loadNyFedRates(opts = {}) {
  const name = 'NY Fed reference rates';
  const url = `${NYFED}/all/latest.json`;
  const { points, sofrAverages } = parseNyFedRates(await getText(name, url, opts));
  if (!points.length) throw new Error(`${name}: no rates in response`);
  const sofr = points.find((p) => p.id === 'SOFR') ?? points[0];
  return dataset({
    id: 'nyfed-rates', label: name, currency: 'USD', kind: 'rates', asOf: sofr.date,
    sourceName: 'Federal Reserve Bank of New York', url, points,
    extra: sofrAverages ? { sofrAverages } : undefined,
  }, opts, new Date());
}

// Enough business days for a semi-annual coupon period plus its lookback, so
// a SOFR floating-rate note's current coupon is covered by published fixings.
export const SOFR_HISTORY_COUNT = 140;

export async function loadSofrHistory({ count = SOFR_HISTORY_COUNT, ...opts } = {}) {
  const name = 'NY Fed SOFR history';
  const url = `${NYFED}/secured/sofr/last/${count}.json`;
  const points = parseSofrHistory(await getText(name, url, opts));
  if (!points.length) throw new Error(`${name}: no observations in response`);
  return dataset({
    id: 'sofr-history', label: `SOFR history (last ${points.length} fixings)`, currency: 'USD', kind: 'rates',
    asOf: points[points.length - 1].date, sourceName: 'Federal Reserve Bank of New York', url, points,
  }, opts, new Date());
}

// ---------------------------------------------------------------- ECB (CSV)

/** RFC 4180 CSV into an array of string arrays (quoted commas, "" escapes, CRLF). */
export function parseCsv(src) {
  const rows = [];
  let row = [];
  let cell = '';
  let quoted = false;
  for (let i = 0; i < src.length; i++) {
    const c = src[i];
    if (quoted) {
      if (c === '"') {
        if (src[i + 1] === '"') { cell += '"'; i++; } else quoted = false;
      } else cell += c;
    } else if (c === '"') quoted = true;
    else if (c === ',') { row.push(cell); cell = ''; }
    else if (c === '\n' || c === '\r') {
      if (c === '\r' && src[i + 1] === '\n') i++;
      row.push(cell); rows.push(row); row = []; cell = '';
    } else cell += c;
  }
  if (cell !== '' || row.length) { row.push(cell); rows.push(row); }
  return rows.filter((r) => r.length > 1 || r[0] !== '');
}

/** ECB SDMX csvdata into objects keyed by header (KEY, TIME_PERIOD, OBS_VALUE, ...). */
export function parseEcbCsv(src) {
  const [header, ...rows] = parseCsv(src);
  if (!header || !header.includes('OBS_VALUE') || !header.includes('TIME_PERIOD')) {
    throw new Error('ECB: CSV is missing the OBS_VALUE/TIME_PERIOD columns');
  }
  return rows.map((r) => Object.fromEntries(header.map((h, i) => [h, r[i] ?? ''])));
}

const lastKeyPart = (key) => key.slice(key.lastIndexOf('.') + 1);

/** Spot-curve points from ECB rows; `SR_10Y` -> tenor `10Y`. */
export function parseEcbSpot(src) {
  const rows = parseEcbCsv(src).filter((r) => /^SR_/.test(lastKeyPart(r.KEY)) && r.OBS_VALUE !== '');
  if (!rows.length) throw new Error('ECB: no spot-rate observations in response');
  const points = rows
    .map((r) => ({ tenor: lastKeyPart(r.KEY).slice(3), value: r.OBS_VALUE }))
    .sort(byTenor);
  return { asOf: rows.map((r) => r.TIME_PERIOD).sort().pop(), points };
}

/** Svensson parameters as { beta0..beta3, tau1, tau2 } source text. */
export function parseEcbSvensson(src) {
  const out = {};
  for (const r of parseEcbCsv(src)) {
    const k = lastKeyPart(r.KEY);
    if (/^(BETA[0-3]|TAU[12])$/.test(k) && r.OBS_VALUE !== '') out[k.toLowerCase()] = r.OBS_VALUE;
  }
  for (const k of ['beta0', 'beta1', 'beta2', 'beta3', 'tau1', 'tau2']) {
    if (!(k in out)) throw new Error(`ECB: Svensson parameter ${k.toUpperCase()} missing`);
  }
  return out;
}

/** Latest €STR fixing as a rates point. */
export function parseEstr(src) {
  const rows = parseEcbCsv(src).filter((r) => r.OBS_VALUE !== '');
  if (!rows.length) throw new Error('ECB: no €STR observation in response');
  const r = rows.reduce((a, b) => (b.TIME_PERIOD > a.TIME_PERIOD ? b : a));
  return { tenor: 'ON', id: 'ESTR', label: 'Euro short-term rate', value: r.OBS_VALUE, date: r.TIME_PERIOD };
}

async function loadEcbSpot(issuers, { id, label, rating }, opts) {
  const name = label;
  const spotUrl = ecbCurveUrl(issuers, ECB_TENORS);
  const [spotText, paramText] = await Promise.all([
    getText(name, spotUrl, opts),
    getText(`${name} (Svensson parameters)`, ecbCurveUrl(issuers, ECB_PARAMS), opts),
  ]);
  const { asOf, points } = parseEcbSpot(spotText);
  return dataset({
    id, label, currency: 'EUR', kind: 'zero', compounding: 'Continuous', asOf,
    sourceName: 'European Central Bank', url: spotUrl, points,
    extra: { svensson: parseEcbSvensson(paramText), rating, issuers },
  }, opts, new Date());
}

export const loadEcbAaaSpot = (opts = {}) =>
  loadEcbSpot('G_N_A', { id: 'ecb-aaa', label: 'ECB euro-area AAA government spot curve', rating: 'AAA' }, opts);

export const loadEcbAllSpot = (opts = {}) =>
  loadEcbSpot('G_N_C', { id: 'ecb-all', label: 'ECB euro-area all-issuers government spot curve', rating: 'all' }, opts);

export async function loadEstr(opts = {}) {
  const name = 'ECB euro short-term rate';
  const point = parseEstr(await getText(name, ECB_ESTR_URL, opts));
  return dataset({
    id: 'estr', label: '€STR', currency: 'EUR', kind: 'rates', asOf: point.date,
    sourceName: 'European Central Bank', url: ECB_ESTR_URL, points: [point],
  }, opts, new Date());
}

// ---------------------------------------------------- Bank of Canada (Valet)

const BOC_BENCH_SERIES = {
  'BD.CDN.2YR.DQ.YLD': '2Y', 'BD.CDN.3YR.DQ.YLD': '3Y', 'BD.CDN.5YR.DQ.YLD': '5Y',
  'BD.CDN.7YR.DQ.YLD': '7Y', 'BD.CDN.10YR.DQ.YLD': '10Y',
  // The "long-term" benchmark is the Government of Canada 30-year bond.
  'BD.CDN.LONG.DQ.YLD': '30Y',
};

/** Latest observation of a Valet response as { date, values: { SERIES: "text" } }. */
export function parseValet(json) {
  const obs = parseJson('Bank of Canada', json)?.observations;
  if (!Array.isArray(obs) || !obs.length) throw new Error('Bank of Canada: no observations in response');
  const latest = obs.reduce((a, b) => (b.d > a.d ? b : a));
  const values = {};
  for (const [k, v] of Object.entries(latest)) if (k !== 'd' && !nullish(v?.v)) values[k] = String(v.v);
  return { date: latest.d, values };
}

export function parseBocBenchmarks(json) {
  const { date, values } = parseValet(json);
  const points = Object.entries(BOC_BENCH_SERIES)
    .filter(([series]) => series in values)
    .map(([series, tenor]) => ({ tenor, value: values[series], id: series }))
    .sort(byTenor);
  if (!points.length) throw new Error('Bank of Canada: no benchmark bond yields in response');
  return { asOf: date, points };
}

export function parseCorra(json) {
  const { date, values } = parseValet(json);
  if (!('AVG.INTWO' in values)) throw new Error('Bank of Canada: CORRA (AVG.INTWO) missing in response');
  return { tenor: 'ON', id: 'CORRA', label: 'Canadian Overnight Repo Rate Average', value: values['AVG.INTWO'], date };
}

export async function loadBocBenchmarks(opts = {}) {
  const name = 'Bank of Canada benchmark bond yields';
  const { asOf, points } = parseBocBenchmarks(await getText(name, BOC_BENCH_URL, opts));
  return dataset({
    id: 'boc-benchmarks', label: 'Government of Canada benchmark bond yields', currency: 'CAD', kind: 'par',
    compounding: 'SemiAnnual', asOf, sourceName: 'Bank of Canada', url: BOC_BENCH_URL, points,
  }, opts, new Date());
}

export async function loadCorra(opts = {}) {
  const name = 'Bank of Canada CORRA';
  const point = parseCorra(await getText(name, BOC_CORRA_URL, opts));
  return dataset({
    id: 'corra', label: 'CORRA', currency: 'CAD', kind: 'rates', asOf: point.date,
    sourceName: 'Bank of Canada', url: BOC_CORRA_URL, points: [point],
  }, opts, new Date());
}

// ------------------------------------------------ CORS-restricted (proxy only)

const MON_NUM = Object.fromEntries(MONTHS.map((m, i) => [m, i + 1]));

/** BoE IADB CSV ("DATE,IUDSOIA,IUDBEDR", dates "19 Oct 2026"): latest non-empty value per series. */
export function parseBoeRates(src) {
  const [header, ...rows] = parseCsv(src);
  if (!header || header[0].trim() !== 'DATE') throw new Error('Bank of England: unexpected CSV header');
  const info = { IUDSOIA: ['SONIA', 'Sterling Overnight Index Average'], IUDBEDR: ['BANKRATE', 'Bank of England Bank Rate'] };
  const points = [];
  header.slice(1).forEach((code, col) => {
    if (!info[code.trim()]) return;
    for (let i = rows.length - 1; i >= 0; i--) {
      const v = (rows[i][col + 1] ?? '').trim();
      const m = /^(\d{1,2}) (\w{3}) (\d{4})$/.exec(rows[i][0].trim());
      if (v === '' || !m || !MON_NUM[m[2]]) continue;
      points.push({
        tenor: 'ON', id: info[code.trim()][0], label: info[code.trim()][1], value: v,
        date: `${m[3]}-${pad2(MON_NUM[m[2]])}-${pad2(+m[1])}`,
      });
      break;
    }
  });
  if (!points.length) throw new Error('Bank of England: no SONIA/Bank Rate observations in response');
  return { asOf: points.map((p) => p.date).sort().pop(), points };
}

export async function loadBoeRates(opts = {}) {
  const name = 'Bank of England SONIA and Bank Rate';
  const from = new Date((opts.now ?? new Date()).getTime() - 21 * 86400000);
  const url = `${BOE_IADB}?csv.x=yes&Datefrom=${pad2(from.getUTCDate())}/${MONTHS[from.getUTCMonth()]}/${from.getUTCFullYear()}` +
    '&Dateto=now&SeriesCodes=IUDSOIA,IUDBEDR&CSVF=TN&UsingCodes=Y&VPD=Y&VFD=N';
  const { asOf, points } = parseBoeRates(await getText(name, url, { ...opts, corsRequired: true }));
  return dataset({
    id: 'boe-rates', label: 'SONIA and Bank Rate', currency: 'GBP', kind: 'rates', asOf,
    sourceName: 'Bank of England', url, corsRequired: true, points,
  }, opts, opts.now ?? new Date());
}

/** MoF JGB CSV (title row, "Date,1Y,2Y,...", dates "2026/10/19"): latest row with data. */
export function parseJgbCsv(src) {
  const rows = parseCsv(src);
  const hi = rows.findIndex((r) => r[0].trim() === 'Date');
  if (hi < 0) throw new Error('Japan MoF: header row ("Date,1Y,...") not found');
  const header = rows[hi].map((h) => h.trim());
  let best = null;
  for (const r of rows.slice(hi + 1)) {
    const m = /^(\d{4})\/(\d{1,2})\/(\d{1,2})$/.exec(r[0].trim());
    if (!m) continue;
    const date = `${m[1]}-${pad2(+m[2])}-${pad2(+m[3])}`;
    if (!best || date > best.date) best = { date, row: r };
  }
  if (!best) throw new Error('Japan MoF: no dated rows in CSV');
  const points = [];
  header.forEach((tenor, i) => {
    const v = (best.row[i] ?? '').trim();
    if (i > 0 && /^\d+Y$/.test(tenor) && /^-?\d+(\.\d+)?$/.test(v)) points.push({ tenor, value: v });
  });
  if (!points.length) throw new Error('Japan MoF: latest row has no yields');
  return { asOf: best.date, points: points.sort(byTenor) };
}

export async function loadJgbCurve(opts = {}) {
  const name = 'Japan MoF JGB yields';
  const { asOf, points } = parseJgbCsv(await getText(name, MOF_JGB, { ...opts, corsRequired: true }));
  return dataset({
    id: 'jgb', label: 'Japan government bond yields', currency: 'JPY', kind: 'par', compounding: 'SemiAnnual',
    asOf, sourceName: 'Japan Ministry of Finance', url: MOF_JGB, corsRequired: true, points,
  }, opts, opts.now ?? new Date());
}

// ----------------------------------------------------------- source registry

export const SOURCES = [
  { id: 'ust-par', label: 'US Treasury par yield curve', currency: 'USD', kind: 'par', loader: 'loadUstPar',
    url: `${UST_XML}?data=daily_treasury_yield_curve`, corsRequired: false,
    homepage: 'https://home.treasury.gov/resource-center/data-chart-center/interest-rates',
    description: 'Daily Treasury par yield curve (CMT), 1 month to 30 years.' },
  { id: 'ust-bills', label: 'US Treasury bill rates', currency: 'USD', kind: 'par', loader: 'loadUstBillRates',
    url: `${UST_XML}?data=daily_treasury_bill_rates`, corsRequired: false,
    homepage: 'https://home.treasury.gov/resource-center/data-chart-center/interest-rates',
    description: 'Daily bill coupon-equivalent yields and discount rates, 4 to 52 weeks.' },
  { id: 'ust-auctions', label: 'US Treasury auctions', currency: 'USD', kind: 'securities', loader: 'loadTreasuryAuctions',
    url: FISCAL_AUCTIONS, corsRequired: false, homepage: 'https://fiscaldata.treasury.gov/datasets/treasury-securities-auctions-data/',
    description: 'Recent nominal Bill, Note and Bond auction results (CUSIP, coupon, dates, high yield).' },
  { id: 'nyfed-rates', label: 'NY Fed reference rates', currency: 'USD', kind: 'rates', loader: 'loadNyFedRates',
    url: `${NYFED}/all/latest.json`, corsRequired: false, homepage: 'https://www.newyorkfed.org/markets/reference-rates',
    description: 'Latest SOFR, EFFR, OBFR, TGCR and BGCR fixings with SOFR averages.' },
  { id: 'sofr-history', label: 'SOFR history', currency: 'USD', kind: 'rates', loader: 'loadSofrHistory',
    url: `${NYFED}/secured/sofr/last/${SOFR_HISTORY_COUNT}.json`, corsRequired: false, homepage: 'https://www.newyorkfed.org/markets/reference-rates/sofr',
    description: 'Daily SOFR fixings for about the last six months, oldest first.' },
  { id: 'ecb-aaa', label: 'ECB AAA euro spot curve', currency: 'EUR', kind: 'zero', loader: 'loadEcbAaaSpot',
    url: ecbCurveUrl('G_N_A', ECB_TENORS), corsRequired: false,
    homepage: 'https://www.ecb.europa.eu/stats/financial_markets_and_interest_rates/euro_area_yield_curves/html/index.en.html',
    description: 'Euro-area AAA government spot curve (Svensson, continuously compounded) with model parameters.' },
  { id: 'ecb-all', label: 'ECB all-issuers euro spot curve', currency: 'EUR', kind: 'zero', loader: 'loadEcbAllSpot',
    url: ecbCurveUrl('G_N_C', ECB_TENORS), corsRequired: false,
    homepage: 'https://www.ecb.europa.eu/stats/financial_markets_and_interest_rates/euro_area_yield_curves/html/index.en.html',
    description: 'Euro-area all-ratings government spot curve (Svensson, continuously compounded).' },
  { id: 'estr', label: 'Euro short-term rate', currency: 'EUR', kind: 'rates', loader: 'loadEstr',
    url: ECB_ESTR_URL, corsRequired: false, homepage: 'https://www.ecb.europa.eu/stats/financial_markets_and_interest_rates/euro_short-term_rate/html/index.en.html',
    description: 'Latest euro short-term rate (€STR) fixing.' },
  { id: 'boc-benchmarks', label: 'Canada benchmark bond yields', currency: 'CAD', kind: 'par', loader: 'loadBocBenchmarks',
    url: BOC_BENCH_URL, corsRequired: false, homepage: 'https://www.bankofcanada.ca/rates/interest-rates/canadian-bonds/',
    description: 'Government of Canada 2Y/3Y/5Y/7Y/10Y/long benchmark bond yields (not a par curve).' },
  { id: 'corra', label: 'CORRA', currency: 'CAD', kind: 'rates', loader: 'loadCorra',
    url: BOC_CORRA_URL, corsRequired: false, homepage: 'https://www.bankofcanada.ca/rates/interest-rates/corra/',
    description: 'Latest Canadian Overnight Repo Rate Average (Valet series AVG.INTWO).' },
  { id: 'boe-rates', label: 'SONIA and Bank Rate', currency: 'GBP', kind: 'rates', loader: 'loadBoeRates',
    url: BOE_IADB, corsRequired: true, homepage: 'https://www.bankofengland.co.uk/boeapps/database/',
    description: 'Latest SONIA and Bank Rate from the Bank of England IADB. Needs a CORS proxy.' },
  { id: 'jgb', label: 'JGB yields', currency: 'JPY', kind: 'par', loader: 'loadJgbCurve',
    url: MOF_JGB, corsRequired: true, homepage: 'https://www.mof.go.jp/english/policy/jgbs/reference/interest_rate/index.htm',
    description: 'Japan MoF JGB yields, 1Y to 40Y, latest day of the current month. Needs a CORS proxy.' },
];

/** Run a registered loader by source id. */
export function loadSource(id, opts = {}) {
  const src = SOURCES.find((s) => s.id === id);
  if (!src) throw new Error(`Unknown market-data source "${id}"`);
  return LOADERS[src.loader](opts);
}

const LOADERS = {
  loadUstPar, loadUstBillRates, loadTreasuryAuctions, loadNyFedRates, loadSofrHistory,
  loadEcbAaaSpot, loadEcbAllSpot, loadEstr, loadBocBenchmarks, loadCorra, loadBoeRates, loadJgbCurve,
};
