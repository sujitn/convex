// Bond reference data pasted from a spreadsheet.
import test from 'node:test';
import assert from 'node:assert/strict';
import { parseBondTable, parseDate, cusipValid, isinValid, bondTemplate } from '../src/lib/bondtable.js';

const TRADE = '2026-10-20';

test('identifiers: CUSIP and ISIN check digits', () => {
  assert.ok(cusipValid('91282CRF0'));
  assert.ok(cusipValid('037833DX5'));
  assert.ok(!cusipValid('91282CRF1'));
  assert.ok(isinValid('US0378331005'));
  assert.ok(isinValid('GB00B24FF097'));
  assert.ok(!isinValid('US0378331006'));
});

test('dates in the forms spreadsheets and terminals produce', () => {
  assert.equal(parseDate('2034-03-15'), '2034-03-15');
  assert.equal(parseDate('03/15/2034'), '2034-03-15');
  assert.equal(parseDate('15/03/2034', { dayFirst: true }), '2034-03-15');
  assert.equal(parseDate('3/5/34'), '2034-03-05');
  assert.equal(parseDate('15-Mar-2034'), '2034-03-15');
  assert.equal(parseDate('15 Mar 34'), '2034-03-15');
  assert.equal(parseDate('Mar 15, 2034'), '2034-03-15');
  assert.equal(parseDate('20340315'), '2034-03-15');
  assert.equal(parseDate('45000'), '2023-03-15'); // Excel serial
  assert.equal(parseDate('02/30/2030'), null);
  assert.equal(parseDate('soon'), null);
});

test('an Excel paste: headers in any order, aliases, 32nds prices, defaults reported, bad rows explained', () => {
  const tsv = [
    'Px\tCUSIP\tDescription\tCpn\tMaturity\tExtra',
    '96-16+\t91282CRF0\tUS Treasury Note\t4.625\t08/15/2036\tx',
    '93.25\t037833DX5\tApple 2.2 2029\t2.2%\t11-Sep-2029\t',
    '\tBAD\tFoo\tx\t2030-01-01\t',
    '100\t\tNo date\t5\tlater\t',
  ].join('\n');
  const out = parseBondTable(tsv, { tradeDate: TRADE });
  assert.deepEqual(out.unknown, ['Extra']);
  assert.equal(out.rows.length, 2);
  const [ust, corp] = out.rows.map((r) => r.sec);
  assert.equal(ust.cusip, '91282CRF0');
  assert.equal(ust.dayCount, 'ActActIcma'); // Treasury CUSIP
  assert.equal(ust.price, '96-16+');
  assert.equal(ust.maturity, '2036-08-15');
  assert.equal(ust.issueDate, '2026-08-15'); // last coupon on or before the trade date
  assert.equal(corp.dayCount, 'Thirty360US');
  assert.equal(corp.coupon, '2.2');
  assert.ok(out.rows[0].assumed.some((a) => /issue date/.test(a)));
  assert.deepEqual(out.errors.map((e) => e.line), [4, 5]);
  assert.match(out.errors[0].reason, /coupon/);
  assert.match(out.errors[1].reason, /maturity/);
});

test('a CSV export: quoted names, day-first dates, explicit conventions and calls', () => {
  const csv = 'ISIN,Name,Coupon,Maturity,Issue Date,Currency,Frequency,Basis,Settle,Calls\n'
    + 'GB00B24FF097,"UK Gilt, 4.75% 2038",4.75,07/12/2038,07/03/2007,GBP,2,ACT/ACT,1,\n'
    + 'XS0000000000,Corp,5.25,15/03/2034,15/03/2024,EUR,annual,30E/360,T+2,15/03/2029@101; 15/03/2030@100\n';
  const out = parseBondTable(csv, { tradeDate: TRADE });
  assert.equal(out.dayFirst, true);
  assert.equal(out.errors.length, 0);
  const [gilt, corp] = out.rows.map((r) => r.sec);
  assert.equal(gilt.name, 'UK Gilt, 4.75% 2038');
  assert.equal(gilt.isin, 'GB00B24FF097');
  assert.equal(gilt.maturity, '2038-12-07');
  assert.equal(gilt.calendar, 'GB.LON');
  assert.equal(corp.isin, undefined);
  assert.ok(out.rows[1].warnings.length === 1);
  assert.equal(corp.frequency, 'Annual');
  assert.equal(corp.dayCount, 'Thirty360E');
  assert.equal(corp.settlementDays, 2);
  assert.deepEqual(corp.calls, [{ date: '2029-03-15', price: '101' }, { date: '2030-03-15', price: '100' }]);
  assert.deepEqual(out.rows[1].assumed, []);
});

test('the header must name coupon and maturity', () => {
  const out = parseBondTable('CUSIP\tPrice\n91282CRF0\t99', { tradeDate: TRADE });
  assert.equal(out.rows.length, 0);
  assert.match(out.errors[0].reason, /Coupon % and Maturity not found/);
});

test('the template parses back to the same bonds', () => {
  const out = parseBondTable('CUSIP\tCoupon\tMaturity\tIssue Date\n91282CRF0\t4.625\t2036-08-15\t2026-08-15', { tradeDate: TRADE });
  const again = parseBondTable(bondTemplate(out.rows.map((r) => r.sec)), { tradeDate: TRADE });
  assert.equal(again.errors.length, 0);
  for (const k of ['cusip', 'coupon', 'maturity', 'issueDate', 'currency', 'frequency', 'dayCount']) {
    assert.equal(again.rows[0].sec[k], out.rows[0].sec[k], k);
  }
});

test('an Index column makes a row a SOFR floating-rate note, alongside fixed rows', () => {
  const tsv = [
    'CUSIP\tName\tCoupon\tMaturity\tIndex\tSpread\tLookback\tPrice',
    '037833DX5\tApple 2.2 2029\t2.2\t09/11/2029\t\t\t\t93.25',
    '\tBank FRN\t\t08/15/2029\tSOFR\t85\t\t100.10',
    '\tAgency FRN\t\t2028-02-20\tCompounded SOFR\t+12.5 bp\t5\t',
  ].join('\n');
  const out = parseBondTable(tsv, { tradeDate: TRADE });
  assert.equal(out.errors.length, 0, JSON.stringify(out.errors));
  const [fixed, bank, agency] = out.rows.map((r) => r.sec);
  assert.equal(fixed.kind, 'fixed');
  assert.equal(bank.kind, 'floating');
  assert.equal(bank.index, 'USD.SOFR');
  assert.equal(bank.spreadBp, '85');
  assert.equal(bank.lookbackDays, 2);
  assert.equal(bank.observationShift, true);
  assert.equal(bank.frequency, 'Quarterly');
  assert.equal(bank.dayCount, 'Act360');
  assert.equal(bank.price, '100.10');
  assert.equal(bank.issueDate, '2026-08-15'); // last quarterly date on or before the trade date
  assert.equal(bank.coupon, undefined);
  assert.ok(out.rows[1].assumed.includes('2-day lookback'));
  assert.ok(out.rows[1].assumed.some((a) => /Act\/360/.test(a)));
  assert.equal(agency.spreadBp, '12.5');
  assert.equal(agency.lookbackDays, 5);
  assert.ok(!out.rows[2].assumed.includes('2-day lookback'));
});

test('floating rows need SOFR, a margin in basis points and USD', () => {
  const tsv = [
    'Name\tMaturity\tIndex\tSpread\tCurrency\tCoupon',
    'Libor FRN\t2029-08-15\tLIBOR\t50\tUSD\t',
    'No spread\t2029-08-15\tSOFR\t\tUSD\t',
    'Percent spread\t2029-08-15\tSOFR\t0.85%\tUSD\t',
    'Euro FRN\t2029-08-15\tSOFR\t40\tEUR\t',
    'Coupon too\t2029-08-15\tSOFR\t40\tUSD\t5',
  ].join('\n');
  const out = parseBondTable(tsv, { tradeDate: TRADE });
  assert.deepEqual(out.errors.map((e) => e.line), [2, 3, 4, 5]);
  assert.match(out.errors[0].reason, /SOFR/);
  assert.match(out.errors[1].reason, /basis points/);
  assert.match(out.errors[2].reason, /basis points/);
  assert.match(out.errors[3].reason, /USD/);
  assert.equal(out.rows.length, 1);
  assert.ok(out.rows[0].warnings.some((w) => /coupon 5 ignored/.test(w)));
});

test('a table of floaters names Index instead of Coupon', () => {
  const ok = parseBondTable('Name\tMaturity\tIndex\tSpread\nFRN\t2029-08-15\tSOFR\t85', { tradeDate: TRADE });
  assert.equal(ok.errors.length, 0, JSON.stringify(ok.errors));
  assert.equal(ok.rows[0].sec.kind, 'floating');
  const neither = parseBondTable('Name\tMaturity\nFRN\t2029-08-15', { tradeDate: TRADE });
  assert.match(neither.errors[0].reason, /Coupon % not found \(a floating-rate note names Index and Spread instead\)/);
});

test('the template carries floaters and parses back to the same notes', () => {
  const src = parseBondTable([
    'CUSIP\tCoupon\tMaturity\tIssue Date\tIndex\tSpread\tLookback',
    '91282CRF0\t4.625\t2036-08-15\t2026-08-15\t\t\t',
    '\t\t2029-08-15\t2025-08-15\tSOFR\t85\t3',
  ].join('\n'), { tradeDate: TRADE });
  assert.equal(src.errors.length, 0, JSON.stringify(src.errors));
  const again = parseBondTable(bondTemplate(src.rows.map((r) => r.sec)), { tradeDate: TRADE });
  assert.equal(again.errors.length, 0, JSON.stringify(again.errors));
  for (const k of ['kind', 'coupon', 'maturity', 'issueDate', 'frequency', 'dayCount', 'index', 'spreadBp', 'lookbackDays']) {
    assert.equal(again.rows[0].sec[k], src.rows[0].sec[k], `fixed ${k}`);
    assert.equal(again.rows[1].sec[k], src.rows[1].sec[k], `floating ${k}`);
  }
});
