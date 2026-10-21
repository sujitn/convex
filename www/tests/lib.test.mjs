// Offline checks of the request-building helpers: dates, prices, calendars,
// bill face scaling, schedule rows and scenario datasets.
import test from 'node:test';
import assert from 'node:assert/strict';
import { parseIso, isIso, ymd, toIso, addMonths, daysInMonth } from '../src/lib/dates.js';
import { parsePrice, formatThirtySeconds } from '../src/lib/decimal.js';
import { isBusinessDay, addBusinessDays, calendarInput, holidaysInYear } from '../src/lib/calendars.js';
import { scaleToFace, billFromPrice, billConversions } from '../src/lib/bills.js';
import { bondTerms } from '../src/lib/terms.js';
import { shiftedDataset } from '../src/analytics/curves.js';
import { keyRateVariants, calibrationRequest } from '../src/analytics/scenarios.js';

test('ISO dates: years below 100 stay themselves, impossible dates are refused', () => {
  assert.equal(parseIso('0099-03-01').getUTCFullYear(), 99);
  assert.equal(toIso(parseIso('0099-03-01')), '0099-03-01');
  assert.equal(ymd(50, 1, 1), '0050-01-01');
  assert.equal(addMonths('0099-01-31', 1), '0099-02-28');
  assert.equal(daysInMonth(0, 2), 29); // year 0 is a leap year; 1900, which Date.UTC would read, is not
  assert.equal(isIso('2026-02-30'), false);
  assert.equal(isIso('2023-02-29'), false);
  assert.equal(isIso('2024-02-29'), true);
  assert.equal(isIso('2026-13-01'), false);
  assert.equal(isIso('2026-00-10'), false);
  assert.equal(isIso('2026-04-31'), false);
  assert.equal(isIso(20261020), false);
  assert.throws(() => parseIso('2026-02-30'), /Invalid date/);
});

test('prices: 32nds digits are + or 0-7, the sign covers the whole price, only plain decimals pass', () => {
  assert.equal(parsePrice('99-16'), 99.5);
  assert.equal(parsePrice('99-16+'), 99 + 16.5 / 32);
  assert.equal(parsePrice('99-162'), 99 + 16.25 / 32);
  assert.equal(parsePrice('99-167'), 99 + 16.875 / 32);
  assert.throws(() => parsePrice('99-168'), /not a price/);
  assert.throws(() => parsePrice('99-169'), /not a price/);
  assert.throws(() => parsePrice('99-32'), /below 32/);
  assert.equal(parsePrice('-99-16'), -99.5);
  assert.equal(parsePrice('+99-16'), 99.5);
  assert.equal(parsePrice(formatThirtySeconds(-99.5)), -99.5);
  for (const p of [-101.2265625, -0.5, 0, 99.5, 100.03125]) assert.equal(parsePrice(formatThirtySeconds(p)), p);
  assert.equal(parsePrice(' 101.25 '), 101.25);
  assert.equal(parsePrice('.5'), 0.5);
  assert.equal(parsePrice('100.'), 100);
  for (const bad of ['0x10', '1e2', 'Infinity', '-Infinity', '', '   ', '99,5', 'NaN', '0b1', null, undefined]) {
    assert.throws(() => parsePrice(bad), /not a price/, String(bad));
  }
});

test('US calendars follow published practice: Juneteenth from 2022, Saturday holidays per calendar', () => {
  for (const code of ['US.SIFMA', 'US.FED']) {
    assert.equal(isBusinessDay(code, '2021-06-18'), true, `${code} open on 18 June 2021`);
    assert.equal(isBusinessDay(code, '2022-06-20'), false, `${code} Juneteenth 2022 observed Monday`);
    assert.equal(isBusinessDay(code, '2021-12-31'), true, `${code} open before a Saturday New Year's Day`);
  }
  // SIFMA closes the Friday before other Saturday holidays; the Fed stays open.
  for (const d of ['2023-11-10', '2026-07-03', '2021-12-24']) {
    assert.equal(isBusinessDay('US.SIFMA', d), false, `SIFMA ${d}`);
    assert.equal(isBusinessDay('US.FED', d), true, `Fed ${d}`);
  }
  assert.equal(isBusinessDay('US.SIFMA', '2025-04-18'), false); // Good Friday
  assert.equal(isBusinessDay('US.FED', '2025-04-18'), true);
  const sifma = calendarInput('US.SIFMA').holidays;
  assert.equal(sifma[0] >= '2000-01-01', true); // 1999-12-31 is outside the table
  assert.ok(!holidaysInYear('US.SIFMA', 2021).some((h) => h.date === '2021-12-31'));
});

test('England & Wales: moved and one-off bank holidays', () => {
  for (const d of ['2002-06-03', '2002-06-04', '2011-04-29', '2012-06-04', '2012-06-05', '2020-05-08', '2022-06-02',
    '2022-06-03', '2022-09-19', '2023-05-08']) {
    assert.equal(isBusinessDay('GB.LON', d), false, d);
  }
  assert.equal(isBusinessDay('GB.LON', '2002-05-27'), true); // spring holiday moved to 4 June
  assert.equal(isBusinessDay('GB.LON', '2012-05-28'), true); // spring holiday moved to 4 June
  assert.equal(isBusinessDay('GB.LON', '2020-05-04'), true); // the early May holiday moved to VE Day
  assert.equal(isBusinessDay('GB.LON', '2022-05-30'), true); // the spring holiday moved to 2 June
  assert.equal(isBusinessDay('GB.LON', '2027-12-27'), false);
  assert.equal(isBusinessDay('GB.LON', '2027-12-28'), false);
  assert.equal(isBusinessDay('EU.TARGET2', '2027-12-31'), true); // TARGET2 moves nothing
});

test('business days count back as well as forward', () => {
  assert.equal(addBusinessDays('US.SIFMA', '2026-10-20', -1), '2026-10-19');
  assert.equal(addBusinessDays('US.SIFMA', '2026-10-19', -1), '2026-10-16'); // over the weekend
  assert.equal(addBusinessDays('US.SIFMA', '2026-11-27', -1), '2026-11-25'); // over Thanksgiving
  assert.equal(addBusinessDays('US.SIFMA', '2026-10-17', 0), '2026-10-17'); // zero keeps even a Saturday
  for (let n = -6; n <= 6; n++) {
    assert.equal(addBusinessDays('GB.LON', addBusinessDays('GB.LON', '2026-12-23', n), -n), '2026-12-23', `n=${n}`);
  }
  assert.throws(() => addBusinessDays('US.SIFMA', '2026-10-20', 1.5), /whole number/);
});

test('bill prices scale exactly to any positive face, decimal faces included', () => {
  assert.equal(scaleToFace('98.8', '100'), '98.8');
  assert.equal(scaleToFace('98.8', '100.5'), '99.294');
  assert.equal(scaleToFace('99.123456', '1000000'), '991234.56');
  assert.equal(scaleToFace('98.8', '0.01'), '0.00988');
  assert.equal(scaleToFace('-1.5', '200'), '-3');
  assert.throws(() => scaleToFace('98.8', '0'), /positive/);
  assert.throws(() => scaleToFace('98.8', '-100'), /positive/);
  const bill = { cusip: '912797XX0', issueDate: '2026-07-23', maturity: '2027-01-21', face: '100.5' };
  assert.equal(billFromPrice(bill, '2026-10-21', '98.8').operation.action.quote.price_original_face, '99.294');
  assert.equal(billConversions(bill, '2026-10-21', '98.8').operation.action.price_original_face, '99.294');
});

const bond = {
  id: 'B', currency: 'USD', coupon: '5', issueDate: '2026-01-15', datedDate: '2026-01-15', maturity: '2036-01-15',
  frequency: 'SemiAnnual', dayCount: 'Thirty360US', face: '100', calendar: 'US.SIFMA', settlementDays: 1,
  paymentAdjustment: 'Following', eom: 'auto',
};

test('schedule rows: empty lines are skipped, partly entered rows are errors that name the row', () => {
  const terms = bondTerms({ ...bond, calls: [{ date: '2031-01-15', price: '100' }, { date: '', price: '' }],
    steps: [{ date: '', coupon: '' }], sinking: [{ date: '', amount: '' }] });
  assert.equal(terms.exercise.length, 1);
  assert.throws(() => bondTerms({ ...bond, calls: [{ date: '2031-01-15', price: '100' }, { date: '2032-01-15', price: '' }] }),
    /Call row 2 is incomplete: enter its price/);
  assert.throws(() => bondTerms({ ...bond, calls: [{ date: '', price: '101' }] }), /Call row 1 is incomplete: enter its date/);
  assert.throws(() => bondTerms({ ...bond, steps: [{ date: '2030-01-15', coupon: ' ' }] }), /Coupon step row 1 is incomplete/);
  assert.throws(() => bondTerms({ ...bond, sinking: [{ date: '2030-01-15' }] }), /Sinking-fund row 1 is incomplete: enter its amount/);
});

const dataset = {
  id: 'ust-par', asOf: '2026-10-20', status: 'snapshot',
  points: [{ tenor: '1Y', value: '3.60' }, { tenor: '2Y', value: '' }, { tenor: '5Y', value: null }, { tenor: '10Y', value: '4.10' },
    { tenor: '30Y' }],
};

test('scenario datasets: tenors without a quote stay without one and are never bumped', () => {
  const up = shiftedDataset(dataset, () => 25, 'p+25');
  assert.deepEqual(up.points.map((p) => p.value), ['3.85', '', null, '4.35', undefined]);
  assert.deepEqual(keyRateVariants(dataset).map((v) => v.tenor), ['1Y', '10Y']);
  const def = { id: 'USD.UST', currency: 'USD', calendar: 'US.SIFMA', settlementDays: 1, benchmarkId: 'US.TREASURY',
    frequency: 'SemiAnnual', dayCount: 'ActActIcma' };
  const req = calibrationRequest(def, up, 'CubicSpline');
  const recipe = req.operation.input.cash[0].recipe;
  assert.equal(recipe.curve.interpolation, 'CubicSpline');
  assert.deepEqual(recipe.helpers.map((h) => h.input.id), ['USD.UST.1Y', 'USD.UST.10Y']);
});
