// CDS request builders, offline: standard dates, combined calendars, the
// credit curves you supply and the exact shape of what goes to the engine.
import test from 'node:test';
import assert from 'node:assert/strict';
import * as C from '../src/lib/cds.js';
import { calendarInput } from '../src/lib/calendars.js';

const TRADE = '2026-10-20';

test('standard maturities follow the semi-annual roll; tenors must be whole quarters', () => {
  assert.deepEqual(C.CDS_TENORS.map((t) => C.standardMaturity(TRADE, t)),
    ['2027-06-20', '2027-12-20', '2028-12-20', '2029-12-20', '2030-12-20', '2031-12-20', '2033-12-20', '2036-12-20']);
  // Before the March roll, the December anchor of the previous year.
  assert.equal(C.standardMaturity('2027-02-15', '5Y'), '2031-12-20');
  assert.equal(C.standardMaturity('2027-03-22', '5Y'), '2032-06-20');
  assert.equal(C.tenorQuarters('9M'), 9);
  assert.equal(C.tenorQuarters('2W'), null);
  assert.equal(C.tenorQuarters('1.5Y'), 18);
  assert.throws(() => C.standardMaturity(TRADE, '1M'), /quarters/);
  assert.ok(C.isQuarterlyTwentieth('2031-12-20'));
  assert.ok(!C.isQuarterlyTwentieth('2031-11-20'));
  assert.ok(!C.isQuarterlyTwentieth('2031-12-21'));
});

test('combined calendars close on either centre and pin the merged tables', () => {
  const both = C.cdsCalendar('TARGET+LON');
  const target = calendarInput('EU.TARGET2');
  const london = calendarInput('GB.LON');
  for (const d of [...target.holidays, ...london.holidays]) assert.ok(both.input.holidays.includes(d), d);
  assert.equal(both.input.holidays.length, new Set([...target.holidays, ...london.holidays]).size);
  assert.match(both.input.pin.id, /^CDS\.EU\.TARGET2_GB\.LON\.rules\.\d{4}-\d{4}$/);
  // A single-centre choice is that centre's own table and pin.
  assert.deepEqual(C.cdsCalendar('TARGET').input, target);
  // Good Friday is closed in London and TARGET2, so the settlement date skips it.
  assert.equal(both.addBusinessDays('2026-04-01', 2), '2026-04-07');
  // Christmas 2026 is a Friday; London's Boxing Day moves to Monday 28th.
  assert.equal(both.following('2026-12-25'), '2026-12-29');
  assert.equal(C.cdsCalendar('TARGET').following('2026-12-25'), '2026-12-28');
});

test('a full first coupon: accrual from the last roll date, adjusted; settlement T+3', () => {
  const d = C.contractDates('NY+LON', '2026-12-21');
  assert.equal(d.accrualStart, '2026-12-21'); // 20 Dec 2026 is a Sunday
  assert.equal(d.stepIn, '2026-12-22');
  assert.equal(d.cashSettlement, '2026-12-24');
  const ctx = C.pricingContext(d);
  assert.deepEqual(ctx, { cash_settlement_date: '2026-12-24', initial_payment_state: 'NotContracted', step_in_date: '2026-12-22', valuation_date: '2026-12-21' });
});

test('a new credit curve takes the standard recovery, clause and calendar for its currency', () => {
  const usd = C.creditCurveDef({ entity: 'Example Corp.', currency: 'USD' });
  assert.equal(usd.id, 'EXAMPLE.CORP.SNRFOR.USD.XR14');
  assert.equal(usd.entityId, 'EXAMPLE.CORP');
  assert.deepEqual([usd.recovery, usd.docClause, usd.calendar, usd.discount], ['40', 'XR14', 'NY+LON', 'auto']);
  const sub = C.creditCurveDef({ entity: 'Example Corp.', currency: 'EUR', seniority: 'Subordinated', existing: {} });
  assert.deepEqual([sub.id, sub.recovery, sub.docClause, sub.calendar], ['EXAMPLE.CORP.SUBLT2.EUR.MM14', '25', 'MM14', 'TARGET+LON']);
  const again = C.creditCurveDef({ entity: 'Example Corp.', currency: 'USD', existing: { [usd.id]: usd } });
  assert.equal(again.id, 'EXAMPLE.CORP.SNRFOR.USD.XR14.2');
  assert.equal(C.exampleSpreads('USD').length, C.CDS_TENORS.length);
  assert.deepEqual(C.exampleSpreads('EUR').map((p) => p.tenor), C.CDS_TENORS);
});

test('quotes are checked before they reach the engine', () => {
  const def = C.creditCurveDef({ entity: 'X', currency: 'USD', points: [
    { tenor: '5Y', value: '96' }, { tenor: '1Y', value: '40.50' }, { tenor: '2Y', value: '' },
  ] });
  const ok = C.creditQuotes(def, TRADE);
  assert.deepEqual(ok.problems, []);
  assert.deepEqual(ok.quotes.map((q) => [q.tenor, q.bp, q.maturity]), [['1Y', '40.5', '2027-12-20'], ['5Y', '96', '2031-12-20']]);
  const bad = C.creditQuotes({ ...def, points: [{ tenor: '1M', value: '10' }, { tenor: '12M', value: '20' }, { tenor: '1Y', value: '21' }, { tenor: '2Y', value: '-5' }] }, TRADE);
  assert.equal(bad.problems.length, 3, bad.problems.join(' / '));
  assert.match(bad.problems.join(' '), /whole quarters/);
  assert.match(bad.problems.join(' '), /same date/);
  assert.match(bad.problems.join(' '), /positive/);
});

test('the calibration request carries complete terms and one helper per tenor', () => {
  const def = C.creditCurveDef({ entity: 'Example Corp', currency: 'USD', points: C.exampleSpreads('USD') });
  const discountKey = { id: 'USD.SOFR.OIS.DF', version: 'v1' };
  const key = C.survivalKey(def, TRADE, discountKey);
  const { quotes } = C.creditQuotes(def, TRADE);
  const recipe = C.creditRecipe({ def, tradeDate: TRADE, key, discountKey, quotes });
  assert.equal(recipe.helpers.length, 8);
  assert.deepEqual(recipe.pillars, quotes.map((q) => q.maturity));
  assert.deepEqual(recipe.curve.role, { entity_id: 'EXAMPLE.CORP', kind: 'Credit', recovery: 0.4, seniority: 'senior_unsecured' });
  assert.equal(recipe.curve.extrapolation, 'None');
  assert.deepEqual(recipe.curve.dependencies, [discountKey]);
  const h = recipe.helpers[5];
  assert.deepEqual(h.quote, { unit: 'BasisPoints', value: 96 });
  assert.equal(h.quote_kind, 'RunningSpread');
  assert.equal(h.pillar, '2031-12-20');
  const t = h.terms;
  assert.equal(t.premium_schedule.start_date, '2026-09-21');
  assert.equal(t.premium_schedule.end_date, '2031-12-20');
  assert.equal(t.maturity_rule, 'QuarterlyTwentieth');
  assert.equal(t.protection_start_date, '2026-10-21');
  assert.equal(t.step_in_date, '2026-10-21');
  assert.equal(t.cash_settlement_date, '2026-10-23');
  assert.equal(t.reference.recovery_fraction, 0.4);
  assert.deepEqual(t.reference.restructuring_clause, { id: 'ISDA.2014.XR14', version: 1 });
  assert.equal(t.premium_calendar.id, t.settlement_calendar.id);
  assert.deepEqual(t.survival_curve, key);
  const req = C.creditCalibration(C.cdsCalendar(def.calendar), { key: discountKey }, recipe);
  assert.equal(req.inputs.calendars[0].pin.id, t.premium_calendar.id);
  assert.equal(req.operation.input.credit[0], recipe);
  // Recovery travels as a fraction made from the typed percent.
  assert.equal(C.creditReference({ ...def, recovery: '37.5' }).recovery_fraction, 0.375);
});

test('risk requests name the factor the engine checks: dated tenor, document version, curve', () => {
  const def = C.creditCurveDef({ entity: 'Example Corp', currency: 'USD', points: C.exampleSpreads('USD') });
  const discount = { key: { id: 'D', version: '1' } };
  const key = C.survivalKey(def, TRADE, discount.key);
  const { quotes } = C.creditQuotes(def, TRADE);
  const recipe = C.creditRecipe({ def, tradeDate: TRADE, key, discountKey: discount.key, quotes });
  const cal = C.cdsCalendar(def.calendar);
  const dates = C.contractDates(def.calendar, TRADE);
  const terms = C.cdsTerms({ tradeId: 'T', currency: 'USD', notional: '10,000,000', couponDecimal: '0.01',
    reference: C.creditReference(def), maturity: '2031-12-20', dates, cal, discountKey: discount.key, survival: key });
  assert.equal(terms.notional, '10000000');
  const input = C.creditRiskInput({ recipe, terms, context: C.pricingContext(dates) });
  const cs01 = C.quoteCs01(cal, discount, input, recipe.helpers[1]).operation.action;
  assert.equal(cs01.factor.tenor, '426D'); // 20 Oct 2026 → 20 Dec 2027
  assert.equal(cs01.factor.convention_version, 1);
  assert.deepEqual(cs01.factor.target, { kind: 'curve', curve_id: key.id });
  assert.notEqual(cs01.revisions.plus_credit.version, cs01.revisions.minus_credit.version);
  assert.equal(cs01.revisions.plus_discount, null);
  const rec = C.recoveryRisk(cal, discount, input, 'HoldSurvival').operation.action;
  assert.deepEqual(rec.factor.target, { kind: 'entity', entity_id: 'EXAMPLE.CORP' });
  const ir = C.discountRisk(cal, discount, input).operation.action;
  assert.equal(ir.revisions.plus_discount.id, 'D');
  const up = C.quoteScenario(cal, discount, input, 1).operation.action.request;
  assert.equal(up.shock.Quotes.moves.length, 8);
  assert.ok(up.shock.Quotes.moves.every((m) => m.delta.unit === 'BasisPoints' && m.delta.value === 1));
});
