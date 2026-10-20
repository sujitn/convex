// Offline tests: parsers and loaders run against saved real responses.
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import {
  parseUstParXml, parseUstBillRatesXml, parseAuctions, parseNyFedRates, parseSofrHistory,
  parseCsv, parseEcbCsv, parseEcbSpot, parseEcbSvensson, parseEstr, parseBocBenchmarks, parseCorra,
  parseBoeRates, parseJgbCsv, loadUstPar, loadUstBillRates, loadEcbAaaSpot, loadSofrHistory,
  loadJgbCurve, loadTreasuryAuctions, proxied, SOURCES, loadSource,
} from '../src/market/feeds.js';

const fx = (name) =>
  readFileSync(fileURLToPath(new URL(`./fixtures/market/${name}`, import.meta.url)), 'utf8');
const ok = (body) => ({ ok: true, status: 200, text: async () => body });
const fakeFetch = (map) => async (url) => {
  const hit = Object.entries(map).find(([k]) => url.includes(k));
  return hit ? ok(hit[1]) : { ok: false, status: 404, statusText: 'Not Found', text: async () => '' };
};
const NOW = new Date('2026-10-20T22:40:00Z');

test('UST par: latest date, exact text, tenor order', () => {
  const r = parseUstParXml(fx('ust-par.xml'));
  assert.equal(r.asOf, '2026-10-20');
  assert.deepEqual(r.points.slice(0, 3), [
    { tenor: '1M', value: '4.10' }, { tenor: '6W', value: '4.13' }, { tenor: '2M', value: '4.17' },
  ]);
  assert.equal(r.points.at(-1).tenor, '30Y');
  assert.equal(r.points.length, 14); // DISPLAY duplicate and Id ignored
  assert.equal(parseUstParXml(fx('ust-par-empty.xml')), null);
});

test('UST par loader: dataset shape and previous-month fallback', async () => {
  const d = await loadUstPar({ fetchImpl: async () => ok(fx('ust-par.xml')), now: NOW });
  assert.equal(d.id, 'ust-par');
  assert.equal(d.status, 'live');
  assert.equal(d.fetchedAt, '2026-10-20T22:40:00Z');
  assert.equal(d.compounding, 'SemiAnnual');
  assert.equal(d.source.corsRequired, false);

  const calls = [];
  const d2 = await loadUstPar({
    fetchImpl: async (url) => {
      calls.push(url);
      return ok(url.endsWith('202611') ? fx('ust-par-empty.xml') : fx('ust-par.xml'));
    },
    now: new Date('2026-11-02T12:00:00Z'),
  });
  assert.equal(calls.length, 2);
  assert.ok(calls[1].endsWith('202610'));
  assert.equal(d2.source.url, calls[1]);
});

test('UST bill rates', async () => {
  const r = parseUstBillRatesXml(fx('ust-bill-rates.xml'));
  assert.equal(r.asOf, '2026-10-20');
  assert.deepEqual(r.points.map((p) => p.tenor), ['4W', '6W', '8W', '13W', '17W', '26W', '52W']);
  assert.equal(r.extra.cusip['4W'], '912797VX2');
  assert.equal(r.extra.maturity['52W'], '2027-09-30');
  assert.match(r.extra.discount['13W'], /^\d\.\d\d$/);
  const d = await loadUstBillRates({ fetchImpl: fakeFetch({ daily_treasury_bill_rates: fx('ust-bill-rates.xml') }), now: NOW });
  assert.equal(d.id, 'ust-bills');
});

test('auctions: nominal, completed only, source text kept', () => {
  const secs = parseAuctions(fx('ust-auctions.json'));
  assert.ok(secs.length > 5);
  for (const s of secs) {
    assert.ok(['Bill', 'Note', 'Bond'].includes(s.type));
    assert.notEqual(s.pricePer100, null);
  }
  const note = secs.find((s) => s.cusip === '91282CRF0');
  assert.equal(note.coupon, '4.625000');
  assert.equal(note.frequency, 'SemiAnnual');
  assert.equal(note.reopening, true);
  assert.equal(note.highYield, '5.3000');
  assert.equal(note.highDiscountRate, null);
  const bill = secs.find((s) => s.type === 'Bill');
  assert.equal(bill.coupon, null);
  assert.equal(bill.frequency, null);
  assert.ok(bill.highDiscountRate);

  const base = { price_per100: '99', inflation_index_security: 'No', floating_rate: 'No', cash_management_bill_cmb: 'No' };
  const rows = [
    { ...base, security_type: 'Note', inflation_index_security: 'Yes' },
    { ...base, security_type: 'Note', floating_rate: 'Yes' },
    { ...base, security_type: 'Bill', cash_management_bill_cmb: 'Yes' },
    { ...base, security_type: 'Note', price_per100: 'null' },
    { ...base, security_type: 'TIPS' },
  ];
  assert.equal(parseAuctions({ data: rows }).length, 0);
});

test('auctions loader builds a filtered, size-limited URL', async () => {
  let seen;
  const d = await loadTreasuryAuctions({
    size: 50,
    fetchImpl: async (u) => { seen = u; return ok(fx('ust-auctions.json')); },
  });
  assert.match(seen, /page%5Bsize%5D=50/);
  assert.match(seen, /inflation_index_security:eq:No/);
  assert.equal(d.kind, 'securities');
  assert.equal(d.asOf, d.securities[0].auctionDate);
});

test('NY Fed rates keep trailing zeros and expose SOFR averages', () => {
  const { points, sofrAverages } = parseNyFedRates(fx('nyfed-latest.json'));
  assert.deepEqual(points.map((p) => p.id).sort(), ['BGCR', 'EFFR', 'OBFR', 'SOFR', 'TGCR']);
  assert.equal(points.find((p) => p.id === 'OBFR').extra.p1, '3.80');
  const sofr = points.find((p) => p.id === 'SOFR');
  assert.equal(sofr.tenor, 'ON');
  assert.equal(sofr.value, '3.87');
  assert.equal(sofr.date, '2026-10-19');
  assert.equal(sofr.extra.volumeBn, 2888);
  assert.equal(sofrAverages.index, '1.26357728');
});

test('SOFR history: one per date, newest last', async () => {
  const pts = parseSofrHistory(fx('nyfed-sofr-last5.json'));
  assert.ok(pts.length >= 2);
  for (let i = 1; i < pts.length; i++) assert.ok(pts[i].date > pts[i - 1].date);
  const d = await loadSofrHistory({ count: 5, fetchImpl: fakeFetch({ 'sofr/last/5.json': fx('nyfed-sofr-last5.json') }) });
  assert.equal(d.asOf, d.points.at(-1).date);
});

test('CSV parser handles quoted commas, escaped quotes, CRLF', () => {
  assert.deepEqual(parseCsv('a,b,c\r\n1,"x, y","say ""hi"""\r\n'), [['a', 'b', 'c'], ['1', 'x, y', 'say "hi"']]);
  assert.deepEqual(parseCsv('a,b\n"multi\nline",2'), [['a', 'b'], ['multi\nline', '2']]);
});

test('ECB spot curve, Svensson parameters, euro short-term rate', async () => {
  const rows = parseEcbCsv(fx('ecb-aaa-spot.csv'));
  assert.ok(rows.every((r) => r.TIME_PERIOD === '2026-10-19'));
  assert.ok(rows.some((r) => r.TITLE_COMPL.includes(',')), 'fixture has quoted commas');
  const spot = parseEcbSpot(fx('ecb-aaa-spot.csv'));
  assert.deepEqual(spot.points.map((p) => p.tenor), ['3M', '6M', '1Y', '2Y', '3Y', '5Y', '7Y', '10Y', '15Y', '20Y', '30Y']);
  assert.equal(spot.points.find((p) => p.tenor === '10Y').value, '3.4690828084');
  assert.deepEqual(parseEcbSvensson(fx('ecb-aaa-svensson.csv')), {
    beta0: '1.7266694017', beta1: '0.6686649799', beta2: '2.0452288278',
    beta3: '6.4974632629', tau1: '1.0348868854', tau2: '14.0151420896',
  });
  assert.equal(parseEstr(fx('ecb-estr.csv')).value, '2.441');
  const d = await loadEcbAaaSpot({
    fetchImpl: fakeFetch({ 'G_N_A.SV_C_YM.SR_3M': fx('ecb-aaa-spot.csv'), 'G_N_A.SV_C_YM.BETA0': fx('ecb-aaa-svensson.csv') }),
  });
  assert.equal(d.kind, 'zero');
  assert.equal(d.compounding, 'Continuous');
  assert.equal(d.extra.svensson.tau2, '14.0151420896');
  assert.equal(parseEcbSpot(fx('ecb-all-spot.csv')).points.length, 11);
  assert.throws(() => parseEcbCsv('A,B\n1,2'), /OBS_VALUE/);
});

test('Bank of Canada benchmarks and CORRA', () => {
  const b = parseBocBenchmarks(fx('boc-benchmarks.json'));
  assert.equal(b.asOf, '2026-10-19');
  assert.deepEqual(b.points.map((p) => [p.tenor, p.value]), [
    ['2Y', '3.33'], ['3Y', '3.44'], ['5Y', '3.64'], ['7Y', '3.72'], ['10Y', '3.93'], ['30Y', '4.25'],
  ]);
  const c = parseCorra(fx('boc-corra.json'));
  assert.equal(c.value, '2.2600');
  assert.equal(c.date, '2026-10-19');
});

test('BoE rates: latest non-empty value per series', () => {
  const r = parseBoeRates(fx('boe-rates.csv'));
  const sonia = r.points.find((p) => p.id === 'SONIA');
  assert.deepEqual([sonia.date, sonia.value], ['2026-10-16', '3.7344']);
  const bank = r.points.find((p) => p.id === 'BANKRATE');
  assert.deepEqual([bank.date, bank.value], ['2026-10-19', '3.75']);
});

test('JGB CSV: latest row, empty cells skipped, proxy required', async () => {
  const r = parseJgbCsv(fx('mof-jgbcme.csv'));
  assert.equal(r.asOf, '2026-10-19');
  assert.equal(r.points[0].tenor, '1Y');
  assert.equal(r.points.find((p) => p.tenor === '10Y').value, '2.935');
  assert.equal(r.points.at(-1).tenor, '40Y');
  assert.deepEqual(parseJgbCsv('Date,1Y,2Y\n2026/10/8,1.657,\n').points, [{ tenor: '1Y', value: '1.657' }]);
  await assert.rejects(() => loadJgbCurve({ fetchImpl: fakeFetch({}) }), /proxy/);
  const d = await loadJgbCurve({
    proxy: 'https://p.example/?url={url}',
    fetchImpl: async (u) => {
      assert.match(u, /^https:\/\/p\.example\/\?url=https%3A%2F%2Fwww\.mof/);
      return ok(fx('mof-jgbcme.csv'));
    },
  });
  assert.equal(d.source.corsRequired, true);
});

test('errors are clear', async () => {
  await assert.rejects(() => loadUstPar({ fetchImpl: fakeFetch({}) }), /US Treasury par yield curve: HTTP 404/);
  assert.throws(() => parseAuctions('not json'), /Treasury auctions: .*JSON/);
  assert.throws(() => proxied('https://x', 'nope'), /\{url\}/);
  assert.throws(() => loadSource('nope'), /Unknown/);
});

test('source registry is complete and well-formed', () => {
  const ids = new Set();
  for (const s of SOURCES) {
    assert.ok(!ids.has(s.id));
    ids.add(s.id);
    for (const k of ['label', 'currency', 'kind', 'loader', 'url', 'homepage', 'description']) assert.ok(s[k], `${s.id}.${k}`);
    assert.equal(typeof s.corsRequired, 'boolean');
  }
  for (const id of ['ust-par', 'ust-auctions', 'nyfed-rates', 'sofr-history', 'ecb-aaa', 'estr', 'boc-benchmarks', 'corra']) {
    assert.ok(ids.has(id), id);
  }
});
