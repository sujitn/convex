// Data you supply: pasted tenor tables and the export/import document.
import test from 'node:test';
import assert from 'node:assert/strict';
import { parseTenorTable, normaliseTenor, normaliseRate, tenorKey, formatTenorTable } from '../src/lib/paste.js';
import { exportData, readImport } from '../src/lib/userdata.js';
import {
  oisSchedule, oisPublications, oisCurveCalibration, oisEffectiveDate, zonedTimestamp, OIS_INDICES, MAX_OIS_QUOTES,
} from '../src/lib/ois.js';
import { customCurveDef } from '../src/analytics/curves.js';
import * as R from '../src/lib/requests.js';

test('tenors in the forms terminals and spreadsheets use', () => {
  assert.equal(normaliseTenor('10Y'), '10Y');
  assert.equal(normaliseTenor('10 yr'), '10Y');
  assert.equal(normaliseTenor('3 Mo'), '3M');
  assert.equal(normaliseTenor('1w'), '1W');
  assert.equal(normaliseTenor('30 YEARS'), '30Y');
  assert.equal(normaliseTenor('1.5Y'), '1.5Y');
  assert.equal(normaliseTenor('1.50 yrs'), '1.5Y');
  assert.equal(normaliseTenor('18M'), '18M');
  assert.equal(normaliseTenor('007Y'), '7Y');
  assert.equal(normaliseTenor('0Y'), null);
  assert.equal(normaliseTenor('10'), null);
  assert.equal(normaliseTenor('abc'), null);
  assert.equal(normaliseTenor('1.5D'), null); // days and weeks are whole
  assert.equal(normaliseTenor('101Y'), null); // past 100 years
  assert.equal(normaliseTenor('99999999999999999999999Y'), null); // never "1e+23Y"
});

test('tenorKey: 12M is 1Y, 18M is 1.5Y, without floating-point residue', () => {
  assert.equal(tenorKey('12M'), tenorKey('1Y'));
  assert.equal(tenorKey('18M'), tenorKey('1.5Y'));
  assert.equal(tenorKey('1.2M'), tenorKey('0.1Y'));
  assert.notEqual(tenorKey('52W'), tenorKey('1Y'));
  assert.equal(tenorKey('3M'), 3);
});

test('rates as typed: thousands separators, decimal commas and percent signs', () => {
  assert.equal(normaliseRate('4.25'), '4.25');
  assert.equal(normaliseRate(' 3.90 % '), '3.90');
  assert.equal(normaliseRate('3,85'), '3.85');
  assert.equal(normaliseRate('-0,125%'), '-0.125');
  assert.equal(normaliseRate('1,234.5'), '1234.5');
  assert.equal(normaliseRate('1,000,000'), '1000000');
  assert.equal(normaliseRate('1,23.5'), null);
  assert.equal(normaliseRate('1e3'), null);
  assert.equal(normaliseRate('abc'), null);
  assert.equal(normaliseRate(''), null);
});

test('a pasted table: headers skipped, separators mixed, duplicates replaced, errors reported', () => {
  const text = 'Tenor\tRate\n1W\t3.92\n1 Mo 3.90\n10 yr, 4.25%\n10Y 4.30\n 2y  3.45\n5Y abc\n7 x\n30Y;4.6\n';
  const { points, errors } = parseTenorTable(text);
  assert.deepEqual(points, [
    { tenor: '1W', value: '3.92' },
    { tenor: '1M', value: '3.90' },
    { tenor: '2Y', value: '3.45' },
    { tenor: '10Y', value: '4.30' },
    { tenor: '30Y', value: '4.6' },
  ]);
  assert.deepEqual(errors.map((e) => e.line), [7, 8]);
  // Rates keep their typed digits exactly.
  assert.deepEqual(parseTenorTable(formatTenorTable(points)).points, points);
});

test('European copies keep their decimal commas; tabs and semicolons split alone', () => {
  assert.deepEqual(parseTenorTable('1Y\t3,85').points, [{ tenor: '1Y', value: '3.85' }]);
  assert.deepEqual(parseTenorTable('1Y;3,85').points, [{ tenor: '1Y', value: '3.85' }]);
  assert.deepEqual(parseTenorTable('10 yr, 4.25%').points, [{ tenor: '10Y', value: '4.25' }]);
  assert.deepEqual(parseTenorTable('Laufzeit;Rendite\n2 Y;2,10\n10 Y;2,65 %').points,
    [{ tenor: '2Y', value: '2.10' }, { tenor: '10Y', value: '2.65' }]);
  assert.deepEqual(parseTenorTable('5Y\t1,234.5\t').points, [{ tenor: '5Y', value: '1234.5' }]);
});

test('extra columns, unreadable lines and absurd tenors are reported, not ignored', () => {
  const { points, errors } = parseTenorTable([
    'Maturity Yield', // a header: skipped
    '10Y 4.25 4.30', // a trailing extra column
    'see notes below', // not a header, not data
    '1000Y 4', // past 100 years
    '1Y\t3.6\t3.7',
    '2Y',
    '3Y 3.5',
  ].join('\n'));
  assert.deepEqual(points, [{ tenor: '3Y', value: '3.5' }]);
  assert.deepEqual(errors.map((e) => [e.line, e.reason]), [
    [2, 'unexpected extra column "4.30"'],
    [3, 'no tenor such as 3M, 1.5Y or 10Y (at most 100 years)'],
    [4, 'no tenor such as 3M, 1.5Y or 10Y (at most 100 years)'],
    [5, 'unexpected extra column "3.7"'],
    [6, 'no rate after the tenor'],
  ]);
});

test('fractional tenors, and one point per length whatever its spelling (last one wins)', () => {
  const { points, errors } = parseTenorTable('1Y 3.60\n6M 3.70\n12M 3.65\n1.5Y 3.55\n18 months 3.50\n2Y 3.40');
  assert.deepEqual(errors, []);
  assert.deepEqual(points, [
    { tenor: '6M', value: '3.70' },
    { tenor: '12M', value: '3.65' },
    { tenor: '18M', value: '3.50' },
    { tenor: '2Y', value: '3.40' },
  ]);
});

const TRADE = '2026-10-20';
const sofrCurve = customCurveDef({ id: 'USD.SOFR', label: 'SOFR OIS', currency: 'USD', method: 'ois', asOf: TRADE,
  points: [{ tenor: '1Y', value: '3.60' }, { tenor: '2Y', value: '3.45' }], index: 'USD.SOFR' });
const security = {
  id: 'MY-BOND', source: 'custom', kind: 'fixed', name: 'My 5% 2031', currency: 'USD', coupon: '5', issueDate: '2026-01-15',
  datedDate: '2026-01-15', firstCouponDate: null, maturity: '2031-01-15', frequency: 'SemiAnnual', dayCount: 'Thirty360US',
  face: '100', calendar: 'US.SIFMA', settlementDays: 1, paymentAdjustment: 'Following', eom: 'auto', redemption: '100',
  calls: [{ date: '2029-01-15', price: '100' }, { date: '', price: '' }],
};

test('export and import round trip, with every record intact', () => {
  const state = {
    datasets: {
      'ust-par': { id: 'ust-par', status: 'manual', asOf: TRADE, revision: 42, points: [{ tenor: '1Y', value: '4' }, { tenor: '2Y', value: '' }] },
      'ecb-aaa': { id: 'ecb-aaa', status: 'live', asOf: TRADE, points: [] },
    },
    custom: { 'MY-BOND': security },
    customCurves: { 'USD.SOFR': sofrCurve },
    spreadCurve: { USD: 'USD.SOFR', EUR: 'EUR.AAA' },
    benchPrices: { '91282CXX1': '99-16' },
  };
  const doc = JSON.parse(JSON.stringify(exportData(state)));
  assert.equal(doc.version, 1);
  assert.deepEqual(Object.keys(doc.datasets), ['ust-par']);
  // Imported datasets are yours: the status is forced to 'manual' whatever the file says.
  doc.datasets['ust-par'].status = 'live';
  const back = readImport(doc);
  assert.deepEqual(back.rejected, []);
  assert.deepEqual(back.datasets, { 'ust-par': { ...state.datasets['ust-par'], status: 'manual' } });
  assert.deepEqual(back.custom, state.custom);
  assert.deepEqual(back.customCurves, state.customCurves);
  assert.deepEqual(back.spreadCurve, state.spreadCurve);
  assert.deepEqual(back.benchPrices, state.benchPrices);
});

test('foreign files, other versions and malformed records are refused or left out and listed', () => {
  assert.throws(() => readImport({ datasets: {} }), /Not a Convex Terminal data file/);
  assert.throws(() => readImport({ kind: 'convex-terminal.data', datasets: {} }), /version undefined/);
  assert.throws(() => readImport({ kind: 'convex-terminal.data', version: 2 }), /version 2/);
  const doc = {
    kind: 'convex-terminal.data',
    version: 1,
    datasets: {
      'ust-par': { id: 'ust-par', asOf: '2026-02-30', points: [] }, // not a real date
      'ecb-aaa': { id: 'ecb-aaa', asOf: TRADE, points: [{ tenor: 'ten years', value: '3' }] },
      'boc-benchmarks': { id: 'boc-benchmarks', asOf: TRADE, points: [{ tenor: '2Y', value: 3.3 }] },
      'nyfed-rates': { id: 'nyfed-rates', asOf: TRADE, points: [] }, // not a curve dataset
      'ecb-all': { id: 'ecb-all', asOf: TRADE, points: [{ tenor: '1Y', value: '2.5' }] },
    },
    customCurves: {
      'USD.SOFR': sofrCurve,
      'USD.ODD': { ...sofrCurve, id: 'USD.ODD', index: 'USD.LIBOR' },
      'USD.UST': { ...sofrCurve, id: 'USD.UST' }, // a built-in id
      A: { ...sofrCurve, id: 'B' },
      'USD.BADPT': { ...sofrCurve, id: 'USD.BADPT', points: [{ tenor: '1Y', value: '' }] },
      'USD.BADDATE': { ...sofrCurve, id: 'USD.BADDATE', asOf: '2026-13-01' },
    },
    custom: {
      'MY-BOND': security,
      NOCOUPON: { ...security, id: 'NOCOUPON', coupon: 5 },
      BADDATE: { ...security, id: 'BADDATE', maturity: '2031-02-29' },
      BADCAL: { ...security, id: 'BADCAL', calendar: 'XX.NOWHERE' },
      BADDC: { ...security, id: 'BADDC', dayCount: 'Act/Act' },
      HALFCALL: { ...security, id: 'HALFCALL', calls: [{ date: '2029-01-15', price: '' }] },
    },
    spreadCurve: { USD: 'USD.SOFR', EUR: 'USD.SOFR', GBP: 'GBP.MINE', CAD: 'CAD.GOC', usd: 'USD.UST' },
    benchPrices: { '91282CXX1': '99-16', C: 5, D: '0x10' },
  };
  const back = readImport(doc, { customCurves: { 'GBP.MINE': { id: 'GBP.MINE', currency: 'GBP' } } });
  assert.deepEqual(Object.keys(back.datasets), ['ecb-all']);
  assert.equal(back.datasets['ecb-all'].status, 'manual');
  assert.deepEqual(Object.keys(back.customCurves), ['USD.SOFR']);
  assert.deepEqual(Object.keys(back.custom), ['MY-BOND']);
  assert.deepEqual(back.spreadCurve, { USD: 'USD.SOFR', GBP: 'GBP.MINE', CAD: 'CAD.GOC' });
  assert.deepEqual(back.benchPrices, { '91282CXX1': '99-16' });
  const ids = (section) => back.rejected.filter((r) => r.section === section).map((r) => r.id);
  assert.deepEqual(ids('datasets'), ['ust-par', 'ecb-aaa', 'boc-benchmarks', 'nyfed-rates']);
  assert.deepEqual(ids('customCurves'), ['USD.ODD', 'USD.UST', 'A', 'USD.BADPT', 'USD.BADDATE']);
  assert.deepEqual(ids('custom'), ['NOCOUPON', 'BADDATE', 'BADCAL', 'BADDC', 'HALFCALL']);
  assert.deepEqual(ids('spreadCurve'), ['EUR', 'usd']);
  assert.deepEqual(ids('benchPrices'), ['C', 'D']);
  assert.match(back.rejected.find((r) => r.id === 'USD.UST').reason, /built-in/);
  assert.equal(back.rejected.length, 18);
  // A section that is not a record is one rejection, not a crash.
  assert.deepEqual(readImport({ kind: 'convex-terminal.data', version: 1, datasets: [] }).rejected,
    [{ section: 'datasets', id: null, reason: 'datasets is not a record of entries' }]);
});

const sofr = { ...OIS_INDICES['USD.SOFR'], index: 'USD.SOFR' };
const corra = { ...OIS_INDICES['CAD.CORRA'], index: 'CAD.CORRA' };
const sonia = { ...OIS_INDICES['GBP.SONIA'], index: 'GBP.SONIA' };

test('OIS dates: spot lag, final payment pillar and one publication per business day', () => {
  const [oneY, twoY] = oisSchedule(sofr, TRADE, ['1Y', '2Y']);
  assert.equal(oneY.effective, '2026-10-22');
  assert.equal(oneY.maturity, '2027-10-22');
  assert.equal(oneY.pillar, '2027-10-26'); // Friday + 2 business days
  // A Sunday maturity: two business days from Sunday is Tuesday (the engine
  // counts the delay from the unadjusted date, then adjusts), not Wednesday.
  assert.equal(twoY.maturity, '2028-10-22');
  assert.equal(twoY.pillar, '2028-10-24');
  const pubs = oisPublications(sofr, TRADE, '2026-10-27');
  assert.deepEqual(pubs.map((p) => p.observation_date), ['2026-10-20', '2026-10-21', '2026-10-22', '2026-10-23', '2026-10-26', '2026-10-27']);
  assert.equal(pubs[3].expected_publication, '2026-10-26T12:00:00Z');
});

test('CORRA: one business day after a Saturday maturity is Monday', () => {
  const [m] = oisSchedule(corra, '2026-10-16', ['1Y']); // spot Monday 2026-10-19, 1Y Tuesday
  assert.equal(m.effective, '2026-10-19');
  const [w] = oisSchedule(corra, '2026-10-22', ['1Y']); // spot Friday 2026-10-23, 1Y Saturday 2027-10-23
  assert.equal(w.maturity, '2027-10-23');
  assert.equal(w.pillar, '2027-10-25');
  assert.equal(m.pillar, '2027-10-20');
});

test('publication instants follow each administrator\'s local clock through daylight saving', () => {
  assert.equal(zonedTimestamp('2027-01-15', '08:00', 'America/New_York'), '2027-01-15T13:00:00Z');
  assert.equal(zonedTimestamp('2027-07-15', '08:00', 'America/New_York'), '2027-07-15T12:00:00Z');
  assert.equal(zonedTimestamp('2027-01-15', '08:00', 'Europe/Berlin'), '2027-01-15T07:00:00Z');
  assert.equal(zonedTimestamp('2027-07-15', '08:00', 'Europe/Berlin'), '2027-07-15T06:00:00Z');
  assert.equal(zonedTimestamp('2027-01-15', '09:00', 'Europe/London'), '2027-01-15T09:00:00Z');
  assert.equal(zonedTimestamp('2027-07-15', '09:00', 'Europe/London'), '2027-07-15T08:00:00Z');
  assert.equal(zonedTimestamp('2027-01-15', '11:00', 'America/Toronto'), '2027-01-15T16:00:00Z');
  assert.equal(zonedTimestamp('2027-07-15', '11:00', 'America/Toronto'), '2027-07-15T15:00:00Z');
  // The day after the US clocks change (Sunday 2026-11-01): EST again.
  assert.equal(zonedTimestamp('2026-11-02', '08:00', 'America/New_York'), '2026-11-02T13:00:00Z');
  // Every publication is after the valuation cutoff, including the trade date's own.
  const pubs = oisPublications(sonia, TRADE, '2026-12-31');
  assert.equal(pubs[0].observation_date, TRADE);
  assert.ok(pubs.every((p) => p.expected_publication > `${TRADE}T12:00:00Z`));
  assert.equal(pubs.find((p) => p.observation_date === '2026-10-30').expected_publication, '2026-11-02T09:00:00Z');
});

test('spot lag 0 on a weekend: the swaps start on Monday and publications start there too', () => {
  assert.equal(oisEffectiveDate(sonia, '2026-10-17'), '2026-10-19'); // Saturday
  const key = R.curveKey('GBP.OIS', '1');
  const req = oisCurveCalibration({ key, projectionKey: R.curveKey('GBP.OIS.ON', '1'), def: sonia, tradeDate: '2026-10-17',
    quotes: [{ tenor: '1M', value: '3.7' }, { tenor: '1Y', value: '3.5' }] });
  const helpers = req.operation.input.rates[0].recipe.helpers;
  assert.ok(helpers.every((h) => h.terms.effective_date === '2026-10-19'));
  assert.equal(req.inputs.publications[0].observation_date, '2026-10-19');
});

test('OIS helpers go in payment-date order; same-date quotes and the item limits are refused by name', () => {
  const key = R.curveKey('USD.OIS', '1');
  const projectionKey = R.curveKey('USD.OIS.ON', '1');
  // SONIA from Friday 2025-01-31: 1M pays 28 February, 30D (Sunday 2 March) pays 3 March.
  const req = oisCurveCalibration({ key, projectionKey, def: sonia, tradeDate: '2025-01-31',
    quotes: [{ tenor: '30D', value: '3.95' }, { tenor: '1M', value: '3.85' }, { tenor: '1Y', value: '3.7' }] });
  const recipe = req.operation.input.rates[0].recipe;
  assert.deepEqual(recipe.helpers.map((h) => h.id), ['USD.OIS.1M', 'USD.OIS.30D', 'USD.OIS.1Y']);
  // Each helper carries its own quote: 1M (3.85%) pays before 30D (3.95%); quotes are decimals.
  assert.deepEqual(recipe.helpers.map((h) => String(h.quote.value)), ['0.0385', '0.0395', '0.037']);
  assert.deepEqual(recipe.pillars, ['2025-02-28', '2025-03-03', '2026-02-02']);
  assert.deepEqual(recipe.pillars, [...recipe.pillars].sort());
  assert.deepEqual(recipe.curve.source.Helpers, recipe.helpers.map((h) => h.id));
  assert.throws(() => oisCurveCalibration({ key, projectionKey, def: sofr, tradeDate: TRADE,
    quotes: [{ tenor: '1W', value: '3.9' }, { tenor: '7D', value: '3.9' }] }), /1W and 7D both pay/);
  const many = Array.from({ length: MAX_OIS_QUOTES + 1 }, (_, i) => ({ tenor: `${i + 1}W`, value: '3.9' }));
  assert.throws(() => oisCurveCalibration({ key, projectionKey, def: sofr, tradeDate: TRADE, quotes: many }),
    (e) => e.code === 'input_limit' && e.limit === 'quotes' && /at most 128/.test(e.message));
  assert.throws(() => oisCurveCalibration({ key, projectionKey, def: sofr, tradeDate: TRADE,
    quotes: [{ tenor: '1Y', value: '3.6' }, { tenor: '20Y', value: '3.9' }] }),
  (e) => e.code === 'input_limit' && e.limit === 'publications');
});

test('credit curves export and import; malformed ones are skipped with a reason', async () => {
  const { creditCurveDef, exampleSpreads } = await import('../src/lib/cds.js');
  const good = creditCurveDef({ entity: 'Example Co', currency: 'USD', points: exampleSpreads('USD'), example: true });
  const state = { datasets: {}, custom: {}, customCurves: {}, benchPrices: {}, spreadCurve: {},
    credit: { [good.id]: good, BAD: { id: 'BAD', entity: 'X', currency: 'JPY', seniority: 'SeniorUnsecured', recovery: '40', docClause: 'XR14', calendar: 'NY', points: [] } } };
  const back = readImport(JSON.parse(JSON.stringify(exportData(state))));
  assert.deepEqual(Object.keys(back.credit), [good.id]);
  assert.deepEqual(back.credit[good.id].points, good.points);
  assert.ok(back.rejected.some((r) => r.section === 'credit' && r.id === 'BAD'));
});

test('par and zero curves are checked for their calibration fields; built-in example ids are not taken', () => {
  const par = customCurveDef({ id: 'USD.PAR', label: 'Par', currency: 'USD', method: 'par', asOf: TRADE,
    points: [{ tenor: '1Y', value: '4' }, { tenor: '2Y', value: '4.1' }] });
  const zero = customCurveDef({ id: 'USD.ZERO', label: 'Zero', currency: 'USD', method: 'zero', asOf: TRADE,
    points: [{ tenor: '1Y', value: '4' }, { tenor: '2Y', value: '4.1' }] });
  const curves = {
    'USD.PAR': par, 'USD.ZERO': zero,
    'P.FREQ': { ...par, id: 'P.FREQ', frequency: 'Weekly' },
    'P.DC': { ...par, id: 'P.DC', dayCount: undefined },
    'P.LAG': { ...par, id: 'P.LAG', settlementDays: -1 },
    'P.LAGTEXT': { ...par, id: 'P.LAGTEXT', settlementDays: '1' },
    'Z.ROLE': { ...zero, id: 'Z.ROLE', role: 'other' },
    'Z.NOROLE': { ...zero, id: 'Z.NOROLE', role: undefined },
    'Z.INTERP': { ...zero, id: 'Z.INTERP', interpolation: 'Quintic' },
  };
  const back = readImport({ kind: 'convex-terminal.data', version: 1, customCurves: curves });
  assert.deepEqual(Object.keys(back.customCurves), ['USD.PAR', 'USD.ZERO']);
  assert.deepEqual(back.rejected.map((r) => r.id), ['P.FREQ', 'P.DC', 'P.LAG', 'P.LAGTEXT', 'Z.ROLE', 'Z.NOROLE', 'Z.INTERP']);

  const edited = { ...security, id: 'EX-CALL-2034', source: 'example' };
  const clash = { ...security, id: 'EX-CALL-2034' };
  const kept = readImport({ kind: 'convex-terminal.data', version: 1, custom: { 'EX-CALL-2034': edited } });
  assert.deepEqual(kept.rejected, []); // an edited example round-trips
  const refused = readImport({ kind: 'convex-terminal.data', version: 1, custom: { 'EX-CALL-2034': clash } });
  assert.deepEqual(Object.keys(refused.custom), []);
  assert.match(refused.rejected[0].reason, /built-in example/);
});
