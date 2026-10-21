// Data you supply: pasted tenor tables and the export/import document.
import test from 'node:test';
import assert from 'node:assert/strict';
import { parseTenorTable, normaliseTenor, formatTenorTable } from '../src/lib/paste.js';
import { exportData, readImport } from '../src/lib/userdata.js';
import { oisSchedule, oisPublications, OIS_INDICES } from '../src/lib/ois.js';

test('tenors in the forms terminals and spreadsheets use', () => {
  assert.equal(normaliseTenor('10Y'), '10Y');
  assert.equal(normaliseTenor('10 yr'), '10Y');
  assert.equal(normaliseTenor('3 Mo'), '3M');
  assert.equal(normaliseTenor('1w'), '1W');
  assert.equal(normaliseTenor('30 YEARS'), '30Y');
  assert.equal(normaliseTenor('0Y'), null);
  assert.equal(normaliseTenor('10'), null);
  assert.equal(normaliseTenor('abc'), null);
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

test('export and import round trip; foreign or malformed content is refused or dropped', () => {
  const state = {
    datasets: { 'ust-par': { id: 'ust-par', status: 'manual', points: [{ tenor: '1Y', value: '4' }] },
      'ecb-aaa': { id: 'ecb-aaa', status: 'live', points: [] } },
    custom: { X: { id: 'X', coupon: '5' } },
    customCurves: {
      'USD.SOFR': { id: 'USD.SOFR', custom: true, method: 'ois', index: 'USD.SOFR', asOf: '2026-10-20', points: [] },
      'USD.ODD': { id: 'USD.ODD', custom: true, method: 'ois', index: 'USD.LIBOR', asOf: '2026-10-20', points: [] },
    },
    benchPrices: { '91282CXX1': '99-16' },
  };
  const doc = JSON.parse(JSON.stringify(exportData(state)));
  assert.deepEqual(Object.keys(doc.datasets), ['ust-par']);
  const back = readImport(doc);
  assert.deepEqual(Object.keys(back.customCurves), ['USD.SOFR']);
  assert.equal(back.benchPrices['91282CXX1'], '99-16');
  assert.throws(() => readImport({ datasets: {} }), /Not a Convex Terminal data file/);
  const bad = readImport({ kind: 'convex-terminal.data', customCurves: { A: { id: 'B', custom: true, points: [] } },
    benchPrices: { C: 5 }, datasets: [] });
  assert.deepEqual(bad.customCurves, {});
  assert.deepEqual(bad.benchPrices, {});
  assert.deepEqual(bad.datasets, {});
});

test('OIS dates: spot lag, final payment pillar and one publication per business day', () => {
  const sofr = { ...OIS_INDICES['USD.SOFR'], index: 'USD.SOFR' };
  const [oneY] = oisSchedule(sofr, '2026-10-20', ['1Y']);
  assert.equal(oneY.effective, '2026-10-22');
  assert.equal(oneY.maturity, '2027-10-22');
  assert.equal(oneY.pillar, '2027-10-26'); // Friday + 2 business days
  const pubs = oisPublications(sofr, '2026-10-20', '2026-10-27');
  assert.deepEqual(pubs.map((p) => p.observation_date), ['2026-10-20', '2026-10-21', '2026-10-22', '2026-10-23', '2026-10-26', '2026-10-27']);
  assert.equal(pubs[3].expected_publication, '2026-10-26T12:00:00Z');
});
