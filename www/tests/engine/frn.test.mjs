// SOFR floating-rate notes against the real engine: compounded-in-arrears
// coupons from published fixings and a projection curve, price and discount
// margin from each other, risk by re-running the engine, and workout paths.
import test from 'node:test';
import assert from 'node:assert/strict';
import { nodeEngine } from './engine-adapter.mjs';
import { SNAPSHOT } from '../../src/market/snapshot.js';
import { buildCurve, customCurveDef } from '../../src/analytics/curves.js';
import { EXAMPLES } from '../../src/analytics/securities.js';
import { analyseFrn } from '../../src/analytics/frn.js';
import { floatingTerms, frnConstruct, accrualCutoffs, historyFixings, frnPublications } from '../../src/lib/frn.js';
import { examplePoints } from '../../src/lib/ois.js';
import { settlementFor } from '../../src/lib/terms.js';
import { daysBetween } from '../../src/lib/dates.js';
import { parseBondTable } from '../../src/lib/bondtable.js';

const TRADE = SNAPSHOT.datasets['ust-par'].asOf;
const HISTORY = SNAPSHOT.datasets['sofr-history'];
const FRN = EXAMPLES.find((e) => e.kind === 'floating');
const SETTLE = settlementFor(FRN, TRADE);
const FACE = '1000000';

const sofrDef = customCurveDef({ id: 'USD.SOFR.OIS', label: 'SOFR OIS', currency: 'USD', method: 'ois', asOf: TRADE,
  points: examplePoints('USD.SOFR'), index: 'USD.SOFR' });
let built;
async function sofr() {
  if (!built) built = await buildCurve(nodeEngine, sofrDef, sofrDef);
  assert.equal(built.error, undefined, JSON.stringify(built.error));
  return built;
}

const analyse = async (opts) => analyseFrn(nodeEngine, {
  sec: FRN, tradeDate: TRADE, settlement: SETTLE, curve: await sofr(), history: HISTORY, face: FACE, ...opts,
});

test('the example FRN is compounded SOFR with an exact margin and stated conventions', async () => {
  const terms = floatingTerms(FRN);
  assert.deepEqual(terms.coupon, { kind: 'overnight', index_id: 'USD.SOFR',
    convention: { lookback_business_days: 2, observation_shift: true, lockout_business_days: 0 } });
  assert.equal(terms.day_count, 'Act360');
  assert.equal(terms.schedule.frequency, 'Quarterly');
  assert.ok(terms.profiles.spread.every((p) => p.value === '0.0085'));
  assert.ok(terms.profiles.gearing.every((p) => p.value === '1'));
  assert.ok(terms.profiles.coupon.every((p) => p.value === '0'));
  assert.deepEqual(terms.dates.payment_delay, { BusinessDays: 0 });
  const res = await nodeEngine.call(frnConstruct(FRN, SETTLE));
  assert.ok(res.ok, JSON.stringify(res.error));
  assert.equal(res.data.periods.length, 16);
  // Settlement falls inside a period, so the shifted accrued needs one explicit cutoff.
  const cutoffs = accrualCutoffs(FRN, res.data.periods, SETTLE);
  assert.equal(cutoffs.length, 1);
});

test('price to discount margin and back: a round trip through the engine', async () => {
  const fromDm = await analyse({ quote: { kind: 'dm', text: '85' } });
  assert.equal(fromDm.fatal, undefined, JSON.stringify(fromDm.fatal));
  assert.ok(Math.abs(fromDm.dm - 0.0085) < 1e-9, `${fromDm.dm}`);
  // The DM inverse lands on the target price.
  assert.ok(Math.abs(fromDm.inverse.residual) < 1e-8, `${fromDm.inverse.residual}`);
  assert.ok(Math.abs(fromDm.clean - fromDm.quotedClean) < 1e-8);

  const fromPrice = await analyse({ quote: { kind: 'price', text: String(fromDm.clean) } });
  assert.equal(fromPrice.fatal, undefined, JSON.stringify(fromPrice.fatal));
  assert.ok(Math.abs(fromPrice.dm - 0.0085) < 1e-9, `${fromPrice.dm}`);
  assert.ok(Math.abs(fromPrice.dirty - fromDm.dirty) < 1e-8);

  // A price off par gives a DM away from the margin, still with a tiny residual.
  const cheap = await analyse({ quote: { kind: 'price', text: '99.5' } });
  assert.equal(cheap.fatal, undefined, JSON.stringify(cheap.fatal));
  assert.ok(cheap.dm > 0.0085);
  assert.ok(Math.abs(cheap.inverse.residual) < 1e-8, `${cheap.inverse.residual}`);
  assert.ok(Math.abs(cheap.clean - 99.5) < 1e-8);
});

test('accrued compounds the published fixings up to the shifted cutoff', async () => {
  const r = await analyse({ quote: { kind: 'dm', text: '85' } });
  assert.equal(r.fatal, undefined, JSON.stringify(r.fatal));
  const cur = r.current;
  const cutoff = r.market.cutoffs[0];
  assert.equal(cur.accrualStart, cutoff.period_start);
  assert.ok(cur.published > 0 && cur.projected > 0, `${cur.published}/${cur.projected}`);
  // Every observation inside the accrued window is a published NY Fed fixing.
  const inWindow = cur.observations.filter((o) => o.weightEnd <= cutoff.weight_end);
  assert.ok(inWindow.length > 30);
  assert.ok(inWindow.every((o) => o.origin === 'published'), JSON.stringify(inWindow.find((o) => o.origin !== 'published')));
  const fixings = new Map(historyFixings(HISTORY).map((f) => [f.observation_date, f.rate]));
  assert.ok(inWindow.every((o) => o.rate === fixings.get(o.date)));
  assert.ok(Math.abs(Number(cur.accrued) - r.accrued) < 1e-12);

  // With every fixing at 4%, accrued is the compounded index over the shifted
  // weights plus the margin over accrual days: an independent check.
  const flat = { ...HISTORY, points: HISTORY.points.map((p) => ({ ...p, value: '4.00' })) };
  const f = await analyse({ quote: { kind: 'dm', text: '85' }, history: flat });
  assert.equal(f.fatal, undefined, JSON.stringify(f.fatal));
  let factor = 1;
  for (const o of f.current.observations) {
    if (o.weightStart >= cutoff.weight_end) break;
    const end = o.weightEnd < cutoff.weight_end ? o.weightEnd : cutoff.weight_end;
    factor *= 1 + 0.04 * (daysBetween(o.weightStart, end) / 360);
  }
  const expected = 100 * (factor - 1) + 100 * 0.0085 * (daysBetween(cutoff.period_start, SETTLE) / 360);
  assert.ok(Math.abs(f.accrued - expected) < 1e-10, `${f.accrued} vs ${expected}`);
  assert.notEqual(f.accrued.toFixed(8), r.accrued.toFixed(8));
});

test('a fixing the history lacks is reported missing, never forecast', async () => {
  // Drop the current period's first published fixing from the history.
  const full = await analyse({ quote: { kind: 'dm', text: '85' } });
  const first = full.current.observations.find((o) => o.origin === 'published').date;
  const short = { ...HISTORY, points: HISTORY.points.filter((p) => p.date > first) };
  const r = await analyse({ quote: { kind: 'dm', text: '85' }, history: short });
  assert.ok(r.fatal);
  assert.equal(r.fatal.code, 'missing_input');
  assert.ok(r.missing && r.missing.count > 0);
  assert.equal(r.missing.first, first);
  assert.equal(r.missing.index, 'USD.SOFR');
});

test('the trade-date fixing, published the next morning, is forecast as awaiting publication', async () => {
  const r = await analyse({ quote: { kind: 'dm', text: '85' } });
  assert.equal(r.fatal, undefined, JSON.stringify(r.fatal));
  assert.equal(r.market.knownAt, `${TRADE}T23:59:59Z`);
  assert.equal(r.current.observations.find((o) => o.date === TRADE).origin, 'awaiting');
  assert.ok(r.current.observations.filter((o) => o.date > TRADE).every((o) => o.origin === 'projected'));
  // Yesterday's fixing was due by the cutoff: without it the engine reports it missing.
  const last = HISTORY.points[HISTORY.points.length - 1].date;
  const stale = { ...HISTORY, points: HISTORY.points.filter((p) => p.date !== last) };
  const s = await analyse({ quote: { kind: 'dm', text: '85' }, history: stale });
  assert.equal(s.fatal && s.fatal.code, 'missing_input');
  assert.deepEqual([s.missing.count, s.missing.first, s.missing.last], [1, last, last]);
});

test('risk: DM and parallel curve bumps re-run the engine', async () => {
  const r = await analyse({ quote: { kind: 'dm', text: '85' } });
  assert.equal(r.fatal, undefined, JSON.stringify(r.fatal));
  assert.deepEqual(r.errors, {});
  assert.ok(r.dmRisk.dm01Per100 < 0);
  assert.ok(r.dmRisk.duration > 2 && r.dmRisk.duration < 3.5, `${r.dmRisk.duration}`);
  // A floater's rate risk is a small fraction of its spread risk: coupons reset.
  assert.ok(Math.abs(r.curveRisk.ir01Per100) < Math.abs(r.dmRisk.dm01Per100) / 10);
  assert.ok(Number(r.curveRisk.nextCoupon.up) > Number(r.curveRisk.nextCoupon.down));
  for (let i = 1; i < r.ladder.length; i++) assert.ok(r.ladder[i].clean < r.ladder[i - 1].clean);
});

test('a callable FRN: discount margin to every legal path through floating_workout', async () => {
  const callable = { ...FRN, id: 'EX-FRN-CALL', calls: [
    { date: '2027-08-16', price: '100' }, // a coupon date
    { date: '2028-03-01', price: '100' }, // inside a period: needs the shifted cutoff
  ] };
  const r = await analyse({ sec: callable, quote: { kind: 'price', text: '100.25' } });
  assert.equal(r.fatal, undefined, JSON.stringify(r.fatal));
  assert.equal(r.callsIgnored, true);
  assert.equal(r.workouts.length, 3);
  for (const w of r.workouts) {
    assert.equal(w.error, undefined, `${w.kind} ${w.date}: ${JSON.stringify(w.error)}`);
    assert.ok(Number.isFinite(w.dm) && Math.abs(w.residual) < 1e-8);
  }
  // To maturity the workout DM is the straight DM.
  assert.ok(Math.abs(r.workouts[0].dm - r.dm) < 1e-9, `${r.workouts[0].dm} vs ${r.dm}`);
  // Redeemed at par sooner, a premium price yields a lower margin.
  assert.ok(r.workouts[1].dm < r.workouts[0].dm);
});

test('an FRN pasted from a spreadsheet prices at its pasted price', async () => {
  const { rows, errors } = parseBondTable(['Name\tMaturity\tIndex\tSpread\tLookback\tPrice',
    'Bank FRN\t11/15/2028\tSOFR\t62.5\t2\t99.85'].join('\n'), { tradeDate: TRADE });
  assert.equal(errors.length, 0, JSON.stringify(errors));
  const sec = rows[0].sec;
  const r = await analyse({ sec, settlement: settlementFor(sec, TRADE), quote: { kind: 'price', text: sec.price } });
  assert.equal(r.fatal, undefined, JSON.stringify(r.fatal));
  assert.ok(Math.abs(r.clean - 99.85) < 1e-8);
  assert.ok(r.dm > 0.00625, `${r.dm}`); // below par: DM above the margin
  assert.ok(r.coupons.length > 4 && r.coupons[0].origin === 'partial');
});

test('publications start at the business day on or before a weekend accrual start, as the engine observes', () => {
  const periods = [{ accrual_start: '2026-08-15', accrual_end: '2026-11-16', payment_date: '2026-11-16' }]; // a Saturday
  const lag = frnPublications({ ...FRN, observationShift: false, lookbackDays: 2 }, periods, '2026-10-21');
  assert.equal(lag[0].observation_date, '2026-08-12'); // Friday 14 August, two business days back
  assert.equal(lag[lag.length - 1].observation_date, '2026-11-10'); // Friday 13 November, two back over Veterans Day
});

test('other conventions price too: lag without shift, lockout, monthly, payment delay, a weekend maturity', async () => {
  const variants = [
    { observationShift: false, lookbackDays: 5 },
    { lockoutDays: 2 },
    { frequency: 'Monthly', issueDate: '2026-09-15', datedDate: '2026-09-15', maturity: '2028-09-15' },
    { paymentDelayDays: 2, lookbackDays: 0 },
    { maturity: '2028-10-15', issueDate: '2026-07-15', datedDate: '2026-07-15' }, // a Sunday
  ];
  for (const v of variants) {
    const sec = { ...FRN, id: `EX-FRN-${JSON.stringify(v)}`, ...v };
    const r = await analyse({ sec, settlement: settlementFor(sec, TRADE), quote: { kind: 'dm', text: '85' } });
    assert.equal(r.fatal, undefined, `${JSON.stringify(v)}: ${JSON.stringify(r.fatal)}`);
    assert.ok(Math.abs(r.dm - 0.0085) < 1e-9 && r.accrued > 0, JSON.stringify(v));
    assert.deepEqual(r.errors, {}, JSON.stringify(v));
  }
});
