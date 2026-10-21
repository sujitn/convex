import React, { useEffect, useMemo, useState } from 'react';
import { useWorkspace, useComputation } from '../state/workspace';
import { useStore } from '../state/store';
import * as C from '../lib/cds';
import {
  discountChoices, resolveDiscount, discountHorizon, discountForCds, buildCredit, creditTable, priceContract,
  convertQuote, contractRisk,
} from '../analytics/cds';
import { Panel, Field, Seg, Select, Figure, KV, EngineError, Empty, Prov, Spinner, Icon } from '../ui/primitives';
import { LineChart } from '../ui/charts';
import ScreenHead from '../ui/ScreenHead';
import PointsEditor from '../ui/PointsEditor';
import { fmt, signed, money, ms } from '../lib/format';
import { formatLong, isIso, tenorYears } from '../lib/dates';
import { isDecimal, fromNumber } from '../lib/decimal';

const SIDES = [['ProtectionBuyer', 'Buy protection'], ['ProtectionSeller', 'Sell protection']];
const SENIORITY_OPTIONS = Object.entries(C.SENIORITIES).map(([k, v]) => [k, v.label]);
const DOC_OPTIONS = Object.entries(C.DOC_CLAUSES);

/** Debounce typed text so every keystroke doesn't fire engine calls. */
function useDebounced(value, delay = 350) {
  const [v, setV] = useState(value);
  useEffect(() => {
    const t = setTimeout(() => setV(value), delay);
    return () => clearTimeout(t);
  }, [value, delay]);
  return v;
}

const validRecovery = (text) => isDecimal(text) && Number(text) >= 0 && Number(text) < 100;
const pct = (v, dp = 4) => fmt(v * 100, dp);
const bps = (v, dp = 2) => fmt(v * 1e4, dp);

// ---- the credit curves you supply ---------------------------------------------

function CurveFields({ value, onChange, locked }) {
  const set = (patch) => onChange({ ...value, ...patch });
  return (
    <div className="form form--3">
      <Field label="Reference entity">
        <input value={value.entity} placeholder="e.g. Example Corp" onChange={(e) => set({ entity: e.target.value })}
          onBlur={value.onEntityBlur} />
      </Field>
      <Field label="Currency" as="div">
        {locked ? <input value={value.currency} readOnly />
          : <Seg value={value.currency} label="Currency" options={C.CDS_CURRENCIES}
            onChange={(currency) => set({ currency, docClause: C.DEFAULT_DOC[currency], calendar: C.DEFAULT_CALENDAR[currency] })} />}
      </Field>
      <Field label="Seniority">
        <Select value={value.seniority} options={SENIORITY_OPTIONS}
          onChange={(seniority) => set({ seniority, recovery: C.SENIORITIES[seniority].recovery })} />
      </Field>
      <Field label="Recovery rate %" hint={`Standard ${C.SENIORITIES[value.seniority].recovery}% for ${C.SENIORITIES[value.seniority].label.toLowerCase()}`}>
        <input className="input--num" value={value.recovery} aria-invalid={!validRecovery(value.recovery)}
          onChange={(e) => set({ recovery: e.target.value.trim() })} onBlur={value.onRecoveryBlur}
          onKeyDown={(e) => e.key === 'Enter' && e.currentTarget.blur()} />
      </Field>
      <Field label="Restructuring clause" hint="Identifies the contract; the model does not value it">
        <Select value={value.docClause} options={DOC_OPTIONS} onChange={(docClause) => set({ docClause })} />
      </Field>
      <Field label="Business days" hint="Premium and settlement dates">
        <Select value={value.calendar} options={C.CALENDAR_CHOICES[value.currency].map((k) => [k, C.CDS_CALENDARS[k].label])}
          onChange={(calendar) => set({ calendar })} />
      </Field>
    </div>
  );
}

function ExampleFill({ currency, onFill }) {
  return (
    <div className="row" style={{ gap: 10, alignItems: 'center' }}>
      <button type="button" className="btn btn--sm" onClick={() => onFill(C.exampleSpreads(currency))}>
        Fill with example spreads (hypothetical)
      </button>
      <span className="dim" style={{ fontSize: 12 }}>Illustrative levels to try the screen, not market data. The curve is marked as an example.</span>
    </div>
  );
}

function NewCredit({ existing, onCreate, onCancel }) {
  const [form, setForm] = useState({ entity: '', currency: 'USD', seniority: 'SeniorUnsecured', recovery: '40',
    docClause: 'XR14', calendar: C.DEFAULT_CALENDAR.USD });
  const [points, setPoints] = useState([]);
  const [example, setExample] = useState(false);
  const entity = form.entity.trim() || (example ? 'Example Corp' : '');
  const ok = entity && validRecovery(form.recovery) && points.length >= 1;
  const create = () => onCreate(C.creditCurveDef({ ...form, entity, points, example, existing }));
  return (
    <Panel title="New credit curve"
      actions={onCancel && <button type="button" className="btn btn--ghost btn--sm" onClick={onCancel}>Cancel</button>}>
      <div className="stack">
        <CurveFields value={form} onChange={setForm} />
        <div className="dim" style={{ fontSize: 12.5 }}>
          Enter par CDS spreads in basis points by tenor, or paste two columns from Excel. Each tenor is a standard
          contract maturing on the 20 June or 20 December roll date; the engine bootstraps a hazard rate per tenor
          so that every contract reprices to its spread.
        </div>
        <div className="cols cols--2" style={{ gap: 16 }}>
          <PointsEditor points={points} valueLabel="Par spread bp" maxHeight={260} templateTenors={C.CDS_TENORS} pasteOpen
            onChange={(p) => { setPoints(p); setExample(false); }} />
          <SpreadChart points={points} />
        </div>
        <ExampleFill currency={form.currency} onFill={(p) => { setPoints(p); setExample(true); }} />
        <div className="row">
          <div className="grow" />
          <button type="button" className="btn btn--primary" disabled={!ok} onClick={create}>Create and calibrate</button>
        </div>
      </div>
    </Panel>
  );
}

function CreditEditor({ def, onRemove, choices, discountId }) {
  const { dispatch } = useStore();
  // Name and recovery are typed here and reach the curve on blur, a rate only when valid.
  const [entity, setEntity] = useState(def.entity);
  const [recovery, setRecovery] = useState(def.recovery);
  useEffect(() => {
    setEntity(def.entity);
    setRecovery(def.recovery);
  }, [def.id, def.entity, def.recovery]);
  const update = (patch) => dispatch({ type: 'creditCurve', curve: { ...def, ...patch, revision: (def.revision || 1) + 1 } });
  const fields = {
    ...def,
    entity,
    recovery,
    onEntityBlur: () => entity.trim() && entity.trim() !== def.entity && update({ entity: entity.trim() }),
    onRecoveryBlur: () => validRecovery(recovery) && recovery !== def.recovery && update({ recovery }),
  };
  const onFields = (next) => {
    if (next.entity !== entity) return setEntity(next.entity);
    setRecovery(next.recovery);
    const patch = {};
    for (const k of ['seniority', 'docClause', 'calendar']) if (next[k] !== def[k]) patch[k] = next[k];
    // A new seniority brings its standard recovery; a typed rate commits on blur.
    if (patch.seniority) patch.recovery = next.recovery;
    return Object.keys(patch).length ? update(patch) : undefined;
  };
  return (
    <Panel title={def.entity}
      note={`${C.SENIORITIES[def.seniority].label}, ${def.currency}, ${def.docClause}`}
      actions={(
        <>
          {def.example && <Prov status="derived">Example spreads</Prov>}
          <button type="button" className="icon-btn" aria-label={`Delete ${def.entity}`} title="Delete credit curve" onClick={onRemove}>
            <Icon name="trash" />
          </button>
        </>
      )}>
      <div className="stack">
        <CurveFields value={fields} onChange={onFields} locked />
        <div className="form form--2">
          <Field label="Discount curve" hint="Your SOFR or €STR curve from MKT → Your data, else the government curve">
            <Select value={def.discount || 'auto'} onChange={(discount) => update({ discount })}
              options={[['auto', `Automatic${discountId ? `: ${(choices.find((c) => c.id === discountId) || {}).label}` : ''}`],
                ...choices.map((c) => [c.id, c.label])]} />
          </Field>
          <Field label="Curve id" hint="Survival curve key sent to the engine"><input value={`CDS.${def.id}`} readOnly /></Field>
        </div>
        <div className="cols cols--2" style={{ gap: 16 }}>
          <PointsEditor points={def.points} valueLabel="Par spread bp" maxHeight={300} templateTenors={C.CDS_TENORS}
            onChange={(points) => update({ points, example: false })} />
          <SpreadChart points={def.points} />
        </div>
        <ExampleFill currency={def.currency} onFill={(points) => update({ points, example: true })} />
      </div>
    </Panel>
  );
}

function SpreadChart({ points }) {
  const input = points.filter((p) => isDecimal(p.value))
    .map((p) => ({ x: tenorYears(p.tenor), y: Number(p.value) }))
    .sort((a, b) => a.x - b.x);
  if (input.length < 1) return <Empty>Par spreads appear here.</Empty>;
  return (
    <LineChart height={260} series={[{ id: 's', label: 'Par spread bp', color: '--s3', points: input, markers: input }]}
      xFormat={(v) => `${Math.round(v)}Y`} yFormat={(v) => v.toFixed(0)} yIncludeZero
      tipX={(v) => `${fmt(v, 2)} years`} tipY={(v) => `${fmt(v, 2)} bp`} />
  );
}

function CreditList({ credits, editing, onSelect, onNew }) {
  return (
    <Panel title="Credit curves" flush
      actions={<button type="button" className="btn btn--sm" onClick={onNew}><Icon name="plus" /> New</button>}>
      {credits.length === 0 && <div className="empty">No credit curves yet.</div>}
      <div role="listbox" aria-label="Credit curves">
        {credits.map((d) => (
          <div key={d.id} role="option" tabIndex={0} aria-selected={editing === d.id}
            className={`list-row${editing === d.id ? ' is-active' : ''}`}
            onClick={() => onSelect(d.id)} onKeyDown={(e) => e.key === 'Enter' && onSelect(d.id)}>
            <div>
              <div><b>{d.entity}</b></div>
              <div className="dim" style={{ fontSize: 11.5 }}>
                {C.SENIORITIES[d.seniority].tier}, {d.currency}, {d.docClause}, recovery {d.recovery}%, {d.points.length} tenors
              </div>
            </div>
            {d.example ? <Prov status="derived">Example</Prov> : <Prov status="manual">Yours</Prov>}
          </div>
        ))}
      </div>
    </Panel>
  );
}

// ---- calibration ------------------------------------------------------------------

function DiscountNote({ discount, choice, currency }) {
  if (!choice) {
    return <div className="note note--error">No {currency} curve is available to discount on. Build one under MKT → Your data.</div>;
  }
  const src = discount && discount.source;
  const asOf = src && src.asOf && isIso(src.asOf) ? formatLong(src.asOf) : '';
  if (choice.kind === 'government') {
    return (
      <div className="note note--warn">
        <b>Government curve standing in for the swap curve.</b> {choice.def.label}{asOf ? `, ${asOf}` : ''}, discounts
        these contracts. CDS discount on the {currency === 'EUR' ? '€STR' : 'SOFR'} swap curve; build one under
        MKT → Your data (OIS swap rates) and it is used here instead.
      </div>
    );
  }
  return (
    <div className="source-line">
      <Prov status={src && src.example ? 'derived' : 'manual'}>{src && src.example ? 'Example rates' : 'Yours'}</Prov>
      <span>Discounting on your swap curve <b>{choice.def.label}</b>{asOf ? `, ${asOf}` : ''}</span>
    </div>
  );
}

function Calibration({ def, discount, built, table, choice }) {
  const b = built;
  const maxResidual = b && b.repricing ? Math.max(...b.repricing.map((r) => Math.abs(r.residual))) : null;
  const rows = table && table.rows ? table.rows : [];
  const survival = rows.length && b.grid
    ? [{ x: b.grid.tenors[0], y: b.grid.values[0] * 100 }, ...rows.map((r) => ({ x: r.years, y: r.survival * 100 }))] : [];
  // Piecewise-constant hazard: flat over each interval, drawn as steps.
  const hazard = rows.flatMap((r, i) => [{ x: i ? rows[i - 1].years : 0, y: r.hazard * 100 }, { x: r.years, y: r.hazard * 100 }]);
  return (
    <>
      <div className="cols cols--wide" style={{ marginBottom: 16 }}>
        <Panel title="Calibration repricing" note="every par spread repriced by the engine on the fitted curve" flush
          foot={b && b.survival ? `Piecewise bootstrap, one hazard rate per tenor (Brent per pillar), log-linear survival, Act/365F, no extrapolation. Discount: ${b.discount.key.id}.` : null}>
          {b && b.repricing ? (
            <div className="table-wrap">
              <table className="grid">
                <thead><tr><th>Tenor</th><th>Maturity</th><th className="num">Par spread bp</th><th className="num">Model bp</th><th className="num">Residual bp</th><th className="num">Tolerance</th></tr></thead>
                <tbody>
                  {b.repricing.map((r, i) => (
                    <tr key={r.helper_id}>
                      <td>{b.quotes[i].tenor}</td><td>{formatLong(r.pillar)}</td>
                      <td className="num">{fmt(r.input.value, 2)}</td>
                      <td className="num">{fmt(r.model.value, 6)}</td>
                      <td className="num dim">{r.residual.toExponential(2)}</td>
                      <td className="num dim">{r.tolerance.toExponential(0)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          ) : (
            <div className="panel__body">
              {b && b.error ? <EngineError error={b.error} context={discount && discount.error ? 'Discount curve' : 'Calibration'} />
                : !choice ? <span className="dim">No discount curve.</span>
                  : <span className="dim"><Spinner /> {discount ? 'Calibrating…' : 'Waiting for the discount curve…'}</span>}
            </div>
          )}
        </Panel>
        <Panel title="Fit">
          <div className="stack">
            <DiscountNote discount={discount} choice={choice} currency={def.currency} />
            {b && b.survival && (
              <KV rows={[
                ['Status', <Prov key="s" status="live">Calibrated</Prov>, `${ms(b.ms)} in the engine`],
                ['Spreads', def.example ? <Prov key="e" status="derived">Example (hypothetical)</Prov> : <Prov key="e" status="manual">Yours</Prov>],
                ['Contracts repriced', `${b.repricing.length}`, `max residual ${maxResidual.toExponential(1)} bp`],
                ['Solver', b.diagnostics ? `${b.diagnostics.method}, ${b.diagnostics.iterations} iterations` : '—'],
                ['Price evaluations', b.evaluations],
                ['Recovery', `${def.recovery}%`, C.SENIORITIES[def.seniority].label],
                ['Valuation', formatLong(b.tradeDate), `step-in ${formatLong(b.dates.stepIn)}, accrual from ${formatLong(b.dates.accrualStart)}`],
                ['Discount factors', `${b.discount.nodes.length} monthly nodes`, `queried from ${b.discount.source.label}, to ${formatLong(b.discount.horizon)}`],
              ]} />
            )}
          </div>
        </Panel>
      </div>

      {rows.length > 0 && (
        <div className="cols cols--wide" style={{ marginBottom: 16 }}>
          <Panel title="Survival and default" note="queried from the calibrated curve" flush>
            <div className="table-wrap">
              <table className="grid">
                <thead><tr><th>Tenor</th><th>Maturity</th><th className="num">Years</th><th className="num">Survival %</th><th className="num">Default to date %</th><th className="num">Default in period %</th><th className="num">Hazard %</th></tr></thead>
                <tbody>
                  {rows.map((r) => (
                    <tr key={r.tenor}>
                      <td>{r.tenor}</td><td>{formatLong(r.maturity)}</td>
                      <td className="num dim">{fmt(r.years, 4)}</td>
                      <td className="num">{pct(r.survival)}</td>
                      <td className="num">{pct(r.cumulative)}</td>
                      <td className="num">{pct(r.interval)}</td>
                      <td className="num">{pct(r.hazard)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            {table.error && <div className="panel__body"><EngineError error={table.error} context="Survival query" /></div>}
            <div className="panel__foot">
              Default in period is conditional on survival to the previous tenor; hazard is the constant intensity
              over the period ending at that tenor, per Act/365F year.
            </div>
          </Panel>
          <Panel title="Term structure">
            <LineChart height={190} series={[{ id: 'q', label: 'Survival %', color: '--s2', points: survival, markers: survival.slice(1) }]}
              xFormat={(v) => `${Math.round(v)}Y`} yFormat={(v) => v.toFixed(1)}
              tipX={(v) => `${fmt(v, 2)} years`} tipY={(v) => `${v.toFixed(4)}%`} />
            <LineChart height={170} series={[{ id: 'h', label: 'Hazard %', color: '--s3', points: hazard }]}
              xFormat={(v) => `${Math.round(v)}Y`} yFormat={(v) => v.toFixed(2)} yIncludeZero
              tipX={(v) => `${fmt(v, 2)} years`} tipY={(v) => `${v.toFixed(4)}%`} />
          </Panel>
        </div>
      )}
    </>
  );
}

// ---- the contract -----------------------------------------------------------------

function Contract({ built, def }) {
  const { engine } = useWorkspace();
  const { tradeDate } = built;
  const [side, setSide] = useState('ProtectionBuyer');
  const [notional, setNotional] = useState('10000000');
  const [tenor, setTenor] = useState('5Y');
  const [maturityText, setMaturityText] = useState('');
  const [coupon, setCoupon] = useState('100');
  const [spreadText, setSpreadText] = useState('');
  const [pointsText, setPointsText] = useState('');

  const coupons = C.COUPONS[def.currency];
  const couponBp = coupons.includes(coupon) ? coupon : coupons.includes('100') ? '100' : coupons[0];
  const dNotional = useDebounced(notional);
  const maturity = tenor === 'date' ? maturityText : C.standardMaturity(tradeDate, tenor);
  const maturityOk = isIso(maturity) && C.isQuarterlyTwentieth(maturity) && maturity > built.dates.stepIn;
  const notionalOk = isDecimal(dNotional) && Number(dNotional) > 0;
  const ready = !!built.survival && maturityOk && notionalOk;
  const contract = { side, notional: dNotional, coupon: couponBp, maturity };
  const sig = JSON.stringify(contract);

  const priced = useComputation(() => (ready ? priceContract(engine, built, contract) : null), [built, sig, ready]);
  const p = priced.data && priced.data.price;
  const risk = useComputation(
    () => (p ? contractRisk(engine, built, priced.data.terms, priced.data.context) : null),
    [priced.data],
  );

  // Conversion: the quoted spread defaults to the contract's par spread on the curve.
  const dSpread = useDebounced(spreadText);
  const dPoints = useDebounced(pointsText);
  const spreadBp = dSpread.trim() || (p ? fromNumber(p.par_spread_decimal * 1e4, 6) : '');
  const spreadOk = isDecimal(spreadBp) && Number(spreadBp) >= 0;
  const toUpfront = useComputation(
    () => (ready && spreadOk ? convertQuote(engine, built, contract, { kind: 'spread', bp: spreadBp }) : null),
    [built, sig, ready, spreadBp],
  );
  const u = toUpfront.data && toUpfront.data.data;
  const pointsIn = dPoints.trim() || (u ? fromNumber(u.valuation.clean_upfront_fraction * 100, 10) : '');
  const pointsOk = isDecimal(pointsIn);
  const toSpread = useComputation(
    () => (ready && pointsOk ? convertQuote(engine, built, contract, { kind: 'upfront', points: pointsIn }) : null),
    [built, sig, ready, pointsIn],
  );
  const s = toSpread.data && toSpread.data.data;

  const r = risk.data;
  const n = Number(dNotional);
  // Engine quote fields run buyer → seller whatever the side; the position view flips them for a seller.
  const sign = side === 'ProtectionBuyer' ? 1 : -1;
  const last = built.quotes[built.quotes.length - 1];

  return (
    <>
      <section className="panel" aria-label="Contract" style={{ marginBottom: 16 }}>
        <div className="panel__body">
          <div className="row" style={{ alignItems: 'flex-start', gap: 16, flexWrap: 'wrap' }}>
            <Field label="Direction" as="div"><Seg value={side} onChange={setSide} options={SIDES} label="Direction" /></Field>
            <Field label={`Notional, ${def.currency}`}>
              <input className="input--num" style={{ width: 150 }} value={notional} aria-invalid={!(isDecimal(notional) && Number(notional) > 0)}
                onChange={(e) => setNotional(e.target.value.replace(/[^\d.]/g, ''))} />
            </Field>
            <Field label="Maturity">
              <Select value={tenor} onChange={setTenor} style={{ width: 170 }}
                options={[...C.CDS_TENORS.map((t) => [t, `${t} · ${formatLong(C.standardMaturity(tradeDate, t))}`]), ['date', 'Roll date…']]} />
            </Field>
            {tenor === 'date' && (
              <Field label="Maturity (a 20 Mar/Jun/Sep/Dec)">
                <input type="date" value={maturityText} aria-invalid={maturityText !== '' && !maturityOk}
                  onChange={(e) => setMaturityText(e.target.value)} />
              </Field>
            )}
            <Field label="Running coupon, bp" as="div">
              <Seg value={couponBp} onChange={setCoupon} options={coupons} label="Running coupon" />
            </Field>
            <div className="grow" />
            <div className="dim" style={{ fontSize: 12, textAlign: 'right' }}>
              {priced.busy || risk.busy ? <><Spinner /> Calculating</> : p ? 'Calculated' : ''}
              <div>Settles {formatLong(built.dates.cashSettlement)}, T+{C.SETTLEMENT_LAG} {built.cal.label}</div>
            </div>
          </div>
          {tenor === 'date' && maturityText && !maturityOk && (
            <div className="note note--warn" style={{ marginTop: 10 }}>
              Standard contracts mature on a quarterly roll date (20 March, June, September or December) after the step-in date.
            </div>
          )}
        </div>
      </section>

      {priced.data && priced.data.error && (
        <>
          <EngineError error={priced.data.error} context="Price" />
          {maturity > last.maturity && (
            <div className="note" style={{ marginBottom: 16 }}>
              The credit curve ends at its last tenor, {last.tenor} ({formatLong(last.maturity)}), and the discount factors
              supplied with it a quarter later; neither is extrapolated. Add a longer tenor to price this maturity.
            </div>
          )}
        </>
      )}

      {p && (
        <>
          <div className="panel" style={{ marginBottom: 16 }}>
            <div className="figures">
              <Figure label="Points upfront" value={signed(p.clean_upfront_fraction * 100, 4)}
                sub={p.clean_upfront_fraction >= 0 ? 'paid by the protection buyer' : 'received by the protection buyer'} />
              <Figure label="Par spread" value={bps(p.par_spread_decimal, 2)} unit="bp" sub={`coupon ${couponBp} bp`} />
              <Figure label="Cash amount" value={money(Math.abs(p.cash_settlement_amount), 0)}
                sub={`${sign * p.cash_settlement_amount >= 0 ? 'you pay' : 'you receive'}, ${formatLong(p.cash_settlement_date)}`} />
              <Figure label="Net PV" value={money(p.net_pv, 0)} sub="your side, dirty, at trade date" />
              <Figure label="CS01" value={r && r.parallel.value !== undefined ? signed(r.parallel.value, 0) : '—'}
                sub="all spreads +1 bp, refitted" />
              <Figure label="RPV01" value={money((p.clean_risky_annuity * n) / 1e4, 0)} sub={`risky annuity ${fmt(p.clean_risky_annuity, 4)}`} />
            </div>
          </div>

          <div className="cols cols--3" style={{ marginBottom: 16 }}>
            <Panel title="Upfront and settlement" note="you pay at settlement; negative: you receive">
              <KV rows={[
                ['Clean price', fmt(p.clean_price_per100, 6), '100 × (1 − clean upfront)'],
                ['Points upfront, clean', signed(p.clean_upfront_fraction * 100, 6), 'quoted buyer → seller'],
                ['Points upfront, dirty', signed(p.dirty_upfront_fraction * 100, 6), 'clean less coupon accrued'],
                ['Principal (clean)', money(sign * p.clean_upfront_fraction * n, 2), 'notional × clean upfront'],
                ['Accrued premium', money(-sign * p.accrued_premium, 2), `${p.accrued_days} days, the seller refunds it to the buyer`],
                ['Cash amount (dirty)', money(sign * p.cash_settlement_amount, 2), `principal and accrued, ${formatLong(p.cash_settlement_date)}`],
              ]} />
            </Panel>
            <Panel title="Legs" note="present value at trade date, your side">
              <KV rows={[
                ['Protection leg', money(p.protection_leg_pv, 2)],
                ['Premium leg', money(p.premium_leg_pv, 2), `coupon ${couponBp} bp, accrual paid on default`],
                ['Net PV (dirty)', money(p.net_pv, 2)],
                ['Risky annuity, clean', fmt(p.clean_risky_annuity, 6), 'years per unit notional'],
                ['Risky annuity, dirty', fmt(p.dirty_risky_annuity, 6)],
                ['Valuation', formatLong(p.valuation_date), `step-in ${formatLong(p.step_in_date)}`],
              ]} />
            </Panel>
            <Panel title="Risk" note="signed, per +1 bp or +1 point">
              {r ? (
                <>
                  <KV rows={[
                    ['CS01, all spreads', r.parallel.value !== undefined ? signed(r.parallel.value, 2) : '—',
                      r.parallel.value !== undefined ? `+1 bp ${signed(r.parallel.up, 0)}, −1 bp ${signed(r.parallel.down, 0)}` : null],
                    ['RPV01', money((p.clean_risky_annuity * n) / 1e4, 2), 'risky annuity × notional × 1 bp'],
                    ['Recovery +1 pt, spreads held', r.recovery.value !== undefined ? signed(r.recovery.value / 100, 2) : '—', 'hazard refitted'],
                    ['Recovery +1 pt, survival held', r.recoveryHeld.value !== undefined ? signed(r.recoveryHeld.value / 100, 2) : '—'],
                    ['Discount curve +1 bp', r.rates.value !== undefined ? signed(r.rates.value, 2) : '—', 'zero shift, credit refitted'],
                  ]} />
                  <EngineError error={r.parallel.error} context="CS01" />
                  <EngineError error={r.recovery.error} context="Recovery" />
                  <EngineError error={r.recoveryHeld.error} context="Recovery" />
                  <EngineError error={r.rates.error} context="Discount risk" />
                </>
              ) : risk.error ? <EngineError error={risk.error} context="Risk" /> : <span className="dim"><Spinner /> Risk…</span>}
            </Panel>
          </div>

          <div className="cols cols--wide" style={{ marginBottom: 16 }}>
            <Panel title="Quoted spread and upfront" note="conventional quote: one flat hazard at the contract coupon">
              <div className="stack">
                <div className="form form--2">
                  <Field label="Quoted spread, bp" hint={dSpread.trim() ? 'Yours' : 'Par spread of this contract on the curve'}>
                    <input className="input--num" value={spreadText} placeholder={spreadBp}
                      aria-invalid={spreadText.trim() !== '' && !(isDecimal(spreadText) && Number(spreadText) >= 0)}
                      onChange={(e) => setSpreadText(e.target.value.trim())} />
                  </Field>
                  <Field label="Points upfront, clean" hint={dPoints.trim() ? 'Yours' : 'From the quoted spread'}>
                    <input className="input--num" value={pointsText} placeholder={pointsIn}
                      aria-invalid={pointsText.trim() !== '' && !isDecimal(pointsText)}
                      onChange={(e) => setPointsText(e.target.value.trim())} />
                  </Field>
                </div>
                <div className="cols cols--2" style={{ gap: 16 }}>
                  <div>
                    {u ? (
                      <KV rows={[
                        ['Points upfront', signed(u.valuation.clean_upfront_fraction * 100, 6), `from ${fmt(Number(spreadBp), 2)} bp`],
                        ['Clean price', fmt(u.valuation.clean_price_per100, 6)],
                        ['Cash amount, buyer pays', money(u.valuation.cash_settlement_amount, 2)],
                        ['Flat hazard', `${pct(u.hazard_per_act365f_year)}%`, `${u.iterations} iterations`],
                      ]} />
                    ) : toUpfront.data && toUpfront.data.error ? <EngineError error={toUpfront.data.error} context="Spread → upfront" />
                      : <span className="dim">{toUpfront.busy ? <Spinner /> : 'Enter a spread.'}</span>}
                  </div>
                  <div>
                    {s ? (
                      <KV rows={[
                        ['Quoted spread', `${bps(s.conventional_spread_decimal, 4)} bp`, `from ${fmt(Number(pointsIn), 4)} points`],
                        ['Clean price', fmt(s.valuation.clean_price_per100, 6)],
                        ['Flat hazard', `${pct(s.hazard_per_act365f_year)}%`],
                        ['Residual', s.native_quote_residual.toExponential(1), 'upfront fraction'],
                      ]} />
                    ) : toSpread.data && toSpread.data.error ? <EngineError error={toSpread.data.error} context="Upfront → spread" />
                      : <span className="dim">{toSpread.busy ? <Spinner /> : 'Enter points upfront.'}</span>}
                  </div>
                </div>
                <div className="dim" style={{ fontSize: 12 }}>
                  The quoted (conventional) spread converts to upfront on a single flat hazard rate, recovery {def.recovery}%,
                  as dealers quote it. The points upfront above the curve figures ({signed(p.clean_upfront_fraction * 100, 4)})
                  come from the full term structure, so the two differ unless the curve is flat.
                </div>
              </div>
            </Panel>
            <Panel title="CS01 by tenor" note="one par spread +1 bp, curve refitted" flush>
              {r && (
                <div className="table-wrap">
                  <table className="grid">
                    <thead><tr><th>Tenor</th><th>Maturity</th><th className="num">Spread bp</th><th className="num">CS01</th></tr></thead>
                    <tbody>
                      {r.buckets.map((bk, i) => (
                        <tr key={bk.tenor}>
                          <td>{bk.tenor}</td><td>{formatLong(bk.maturity)}</td>
                          <td className="num">{fmt(Number(built.quotes[i].bp), 2)}</td>
                          <td className={`num ${bk.value > 0 ? 'pos' : bk.value < 0 ? 'neg' : ''}`}>
                            {bk.value !== undefined ? signed(bk.value, 2) : <span className="neg" title={bk.error.message}>{bk.error.code}</span>}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
            </Panel>
          </div>

          <Panel title="Premium leg" note="each premium conditional on survival; amounts and PVs for your side" flush
            style={{ marginBottom: 16 }} foot={p.model_notice}>
            <div className="table-wrap" style={{ maxHeight: 420 }}>
              <table className="grid">
                <thead><tr><th>Accrual start</th><th>Accrual end</th><th>Payment</th><th className="num">Year fraction</th><th className="num">Coupon</th><th className="num">Survival %</th><th className="num">PV</th><th className="num">Accrual on default PV</th></tr></thead>
                <tbody>
                  {p.premiums.map((q) => (
                    <tr key={q.accrual_start}>
                      <td>{formatLong(q.accrual_start)}</td><td>{formatLong(q.accrual_end)}</td><td>{formatLong(q.payment_date)}</td>
                      <td className="num">{fmt(q.year_fraction, 6)}</td>
                      <td className="num">{money(q.conditional_coupon_amount, 2)}</td>
                      <td className="num">{pct(q.survival_probability)}</td>
                      <td className="num">{money(q.scheduled_pv, 2)}</td>
                      <td className="num">{money(q.accrual_on_default_pv, 2)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </Panel>
        </>
      )}
      {!p && !(priced.data && priced.data.error) && (
        <Empty>{!notionalOk ? 'Enter a notional.' : !maturityOk ? 'Choose a roll-date maturity.' : 'Pricing…'}</Empty>
      )}
    </>
  );
}

// ---- screen -------------------------------------------------------------------------

function CreditAnalysis({ def }) {
  const { engine, curves, curveDefs, tradeDate } = useWorkspace();
  const { state } = useStore();
  const choices = useMemo(() => discountChoices(curveDefs, def.currency), [curveDefs, def.currency]);
  const discountId = resolveDiscount(def.discount, choices, state.spreadCurve[def.currency]);
  const choice = choices.find((c) => c.id === discountId) || null;
  const curve = discountId ? curves[discountId] : null;
  const horizon = discountHorizon(def, tradeDate);
  const ready = !!curve && !curve.building;
  const discount = useComputation(
    () => (ready ? discountForCds(engine, curve, horizon, choice.kind) : null),
    [ready, curve && curve.key && `${curve.key.id}|${curve.key.version}`, curve && curve.error, horizon, choice && choice.kind],
  );
  const sig = JSON.stringify([def.entity, def.seniority, def.recovery, def.docClause, def.calendar, def.points, def.revision]);
  const built = useComputation(
    () => (discount.data ? buildCredit(engine, def, { tradeDate, discount: discount.data }) : null),
    [discount.data, sig, tradeDate],
  );
  const b = built.current ? built.data : null;
  const table = useComputation(() => (b && b.survival ? creditTable(engine, b) : null), [b]);
  return (
    <>
      <Calibration def={def} discount={discount.data} built={b} table={table.current ? table.data : null} choice={choice} />
      {b && b.survival && <Contract built={b} def={def} />}
    </>
  );
}

export default function Cds({ fn }) {
  const { state, dispatch } = useStore();
  const { curveDefs, tradeDate } = useWorkspace();
  const credits = Object.values(state.credit || {});
  const [editing, setEditing] = useState(() => (credits[0] ? credits[0].id : 'new'));
  const def = editing !== 'new' ? (state.credit || {})[editing] : null;
  const currency = def ? def.currency : null;
  const choices = useMemo(() => (currency ? discountChoices(curveDefs, currency) : []), [curveDefs, currency]);
  const discountId = def ? resolveDiscount(def.discount, choices, state.spreadCurve[def.currency]) : null;
  return (
    <div className="screen">
      <ScreenHead fn={fn}
        lede="Credit curves from par spreads you supply, calibrated natively on your swap curve, and standard contracts priced on them." />
      <div className="cols cols--side" style={{ marginBottom: 16 }}>
        <CreditList credits={credits} editing={editing} onSelect={setEditing} onNew={() => setEditing('new')} />
        {def ? (
          <CreditEditor def={def} choices={choices} discountId={discountId}
            onRemove={() => {
              dispatch({ type: 'removeCreditCurve', id: def.id });
              setEditing((credits.find((d) => d.id !== def.id) || {}).id || 'new');
            }} />
        ) : (
          <NewCredit existing={state.credit || {}} onCancel={credits.length ? () => setEditing(credits[0].id) : null}
            onCreate={(d) => { dispatch({ type: 'creditCurve', curve: d }); setEditing(d.id); }} />
        )}
      </div>
      {def ? <CreditAnalysis def={def} key={`${def.id}|${tradeDate}`} /> : (
        <Empty>Create a credit curve: name the reference entity, enter par spreads by tenor (or fill the example set), and the engine calibrates it.</Empty>
      )}
    </div>
  );
}
