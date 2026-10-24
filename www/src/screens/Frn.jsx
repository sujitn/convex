import React, { useEffect, useMemo, useState } from 'react';
import { useWorkspace, useComputation } from '../state/workspace';
import { useStore } from '../state/store';
import { analyseFrn } from '../analytics/frn';
import { customCurveDef } from '../analytics/curves';
import { FRN_DEFAULTS, FRN_INDICES, SOFR, frnSecurity, frnConstruct, floatingTerms, frnLabel, lastCouponOnOrBefore } from '../lib/frn';
import { examplePoints } from '../lib/ois';
import { isDecimal, parsePrice, formatThirtySeconds } from '../lib/decimal';
import { isIso, formatLong, formatShort, daysBetween } from '../lib/dates';
import { parseDate } from '../lib/bondtable';
import { fmt, pct, signed, signedBp, money } from '../lib/format';
import { Panel, Field, Seg, Select, Figure, KV, EngineError, Empty, Spinner, Prov, Icon, Tabs } from '../ui/primitives';
import SecurityBand, { dayCountLabel, frequencyLabel } from '../ui/SecurityBand';
import ScreenHead from '../ui/ScreenHead';
import { JsonView } from '../ui/Inspector';
import { LineChart } from '../ui/charts';

const LEDE = 'SOFR floating-rate notes, compounded in arrears: price and discount margin from each other, the current '
  + 'coupon’s fixings, the projected coupons and risk. Every figure is computed by the library.';

const FREQS = ['Quarterly', 'Monthly', 'SemiAnnual', 'Annual'];
const DAY_COUNTS = ['Act360', 'Act365Fixed'];
const ADJUSTMENTS = [['ModifiedFollowing', 'Modified Following'], ['Following', 'Following'], ['Unadjusted', 'Unadjusted']];
const EXAMPLE_CURVE_ID = 'USD.SOFR.EXAMPLE';

function useDebounced(value, delay = 350) {
  const [v, setV] = useState(value);
  useEffect(() => {
    const t = setTimeout(() => setV(value), delay);
    return () => clearTimeout(t);
  }, [value, delay]);
  return v;
}

function quoteValid(q) {
  if (!q || !String(q.text).trim()) return false;
  if (q.kind === 'price') {
    try {
      return parsePrice(q.text) > 0;
    } catch {
      return false;
    }
  }
  return isDecimal(q.text) && Math.abs(Number(q.text)) < 5000;
}

// A pasted or imported price is the opening quote; otherwise the note opens
// at a discount margin equal to its quoted margin (there is no market price).
function defaultQuote(sec) {
  if (typeof sec.price === 'string' && sec.price.trim()) return { kind: 'price', text: sec.price };
  return { kind: 'dm', text: String(sec.spreadBp ?? '0'), opening: true };
}

const epochDay = (iso) => Date.parse(`${iso}T00:00:00Z`) / 86400000;
const isoOfDay = (v) => new Date(Math.round(v) * 86400000).toISOString().slice(0, 10);

const ORIGIN_TEXT = { fixed: 'Fixed', partial: 'Partly fixed', projected: 'Projected', unknown: '—' };

function RateOrigin({ row }) {
  const title = `${row.published} published, ${row.projected} projected of ${row.observations.length} daily observations`;
  return (
    <span className={`rate-origin rate-origin--${row.origin}`} title={title}>
      <i aria-hidden="true" />
      {ORIGIN_TEXT[row.origin]}
      {row.origin === 'partial' ? ` ${row.published}/${row.observations.length}` : ''}
    </span>
  );
}

const OBS_TEXT = { published: 'Published', awaiting: 'Awaiting publication', projected: 'Projected' };

/** Calls typed as "2027-08-16@100; 03/01/2028 @ 100". */
function parseCallsText(text) {
  const out = [];
  for (const part of String(text || '').split(/[;|\n]/).map((p) => p.trim()).filter(Boolean)) {
    const m = /^(.+?)\s*@\s*([\d.]+)$/.exec(part);
    const date = m && parseDate(m[1]);
    if (!m || !date || !isDecimal(m[2])) return null;
    out.push({ date, price: m[2] });
  }
  return out.sort((a, b) => (a.date < b.date ? -1 : 1));
}

const callsText = (calls) => (calls || []).filter((c) => c.date && c.price).map((c) => `${c.date}@${c.price}`).join('; ');

/** Terms form for a new FRN or for editing one. */
export function FrnTermsForm({ sec, tradeDate, onApply, onCancel, readOnly = false }) {
  const start = sec || { ...FRN_DEFAULTS, spreadBp: '', issueDate: '', maturity: '' };
  const initial = { ...start, callsText: callsText(start.calls) };
  const [draft, setDraft] = useState(initial);
  useEffect(() => setDraft(initial), [sec]); // eslint-disable-line react-hooks/exhaustive-deps
  const dirty = JSON.stringify(draft) !== JSON.stringify(initial);
  const set = (k) => (v) => setDraft((d) => ({ ...d, [k]: v }));
  const text = (k) => ({ value: draft[k] ?? '', onChange: (e) => set(k)(e.target.value) });
  const days = (k) => ({ value: draft[k] ?? '', onChange: (e) => /^\d{0,2}$/.test(e.target.value) && set(k)(e.target.value === '' ? '' : Number(e.target.value)) });
  const calls = parseCallsText(draft.callsText);
  const intOk = (v) => Number.isInteger(v) && v >= 0;
  const issue = draft.issueDate || (isIso(draft.maturity) ? lastCouponOnOrBefore(draft.maturity, draft.frequency, tradeDate) : '');
  const valid = isDecimal(draft.spreadBp || '') && isIso(draft.maturity) && isIso(issue) && draft.maturity > issue
    && intOk(draft.lookbackDays) && intOk(draft.lockoutDays) && intOk(draft.paymentDelayDays) && intOk(draft.settlementDays)
    && calls !== null;
  const apply = () => {
    const { callsText: _, ...rest } = draft;
    onApply({ ...rest, issueDate: issue, datedDate: draft.datedDate && draft.datedDate >= issue ? draft.datedDate : issue,
      accrualAdjustment: draft.paymentAdjustment, calls: calls || [] });
  };
  return (
    <Panel title={sec ? 'Floating terms' : 'New SOFR floating-rate note'}
      note={readOnly ? 'read-only' : 'compounded SOFR in arrears, margin outside the compounding'}
      actions={!readOnly && (
        <>
          {onCancel && <button type="button" className="btn btn--ghost btn--sm" onClick={onCancel}>Cancel</button>}
          <button type="button" className="btn btn--primary btn--sm" disabled={!valid || (sec && !dirty)} onClick={apply}>{sec ? 'Apply terms' : 'Create FRN'}</button>
        </>
      )}>
      <fieldset disabled={readOnly} style={{ border: 0, padding: 0, margin: 0 }}>
        <div className="form">
          <Field label="Name" wide><input {...text('name')} placeholder={`SOFR + ${draft.spreadBp || 'margin'} FRN`} /></Field>
          <Field label="Index"><Select value={draft.index || SOFR} onChange={set('index')}
            options={Object.entries(FRN_INDICES).map(([k, v]) => [k, `${v.label}, compounded in arrears`])} /></Field>
          <Field label="Margin over index, bp"><input className="input--num" {...text('spreadBp')}
            aria-invalid={draft.spreadBp !== '' && !isDecimal(draft.spreadBp || '')} /></Field>
          <Field label="Issue date" hint={draft.issueDate ? null : 'blank: last coupon date (regular)'}>
            <input type="date" {...text('issueDate')} /></Field>
          <Field label="Maturity"><input type="date" {...text('maturity')} /></Field>
          <Field label="Reset and pay"><Select value={draft.frequency} onChange={set('frequency')} options={FREQS.map((f) => [f, frequencyLabel(f)])} /></Field>
          <Field label="Day count"><Select value={draft.dayCount} onChange={set('dayCount')} options={DAY_COUNTS.map((d) => [d, dayCountLabel(d)])} /></Field>
          <Field label="Lookback, business days"><input className="input--num" {...days('lookbackDays')} /></Field>
          <Field label="Observation" as="div">
            <Seg value={draft.observationShift === false ? 'lag' : 'shift'} label="Observation convention"
              onChange={(v) => set('observationShift')(v === 'shift')} options={[['shift', 'Shift'], ['lag', 'Lag']]} />
          </Field>
          <Field label="Lockout, business days"><input className="input--num" {...days('lockoutDays')} /></Field>
          <Field label="Payment delay, business days"><input className="input--num" {...days('paymentDelayDays')} /></Field>
          <Field label="Date adjustment"><Select value={draft.paymentAdjustment || 'ModifiedFollowing'} onChange={set('paymentAdjustment')} options={ADJUSTMENTS} /></Field>
          <Field label="Settlement days"><input className="input--num" {...days('settlementDays')} /></Field>
          <Field label="Calls (date@price; …)" wide hint="optional; issuer calls at a price per 100, priced as workouts">
            <input {...text('callsText')} placeholder="2028-08-15@100" aria-invalid={calls === null} />
          </Field>
        </div>
      </fieldset>
      <div className="dim" style={{ fontSize: 12, marginTop: 10 }}>
        Shift weights each day by the observation period (ARRC observation shift); Lag looks back on the observation calendar but
        weights by interest-period days. Index floors and caps are not offered: the library has no overnight coupon option yet.
      </div>
    </Panel>
  );
}

/** The engine-generated schedule and the full terms, for SEC. */
export function FrnContract({ sec, settlement }) {
  const { engine } = useWorkspace();
  const [view, setView] = useState('schedule');
  const built = useComputation(
    () => engine.call(frnConstruct({ ...sec, calls: [] }, settlement), { label: `${sec.ticker || sec.name} schedule` }),
    [sec, settlement],
  );
  const terms = useMemo(() => {
    try {
      return floatingTerms(sec);
    } catch (e) {
      return { error: e.message };
    }
  }, [sec]);
  const res = built.current ? built.data : null;
  return (
    <Panel title="Contract" flush actions={<Tabs value={view} onChange={setView} options={[['schedule', 'Generated schedule'], ['terms', 'BondTerms JSON']]} label="View" />}>
      {view === 'terms' ? (terms && !terms.error ? <JsonView value={terms} />
        : <div className="panel__body"><EngineError error={{ code: 'invalid_input', message: terms && terms.error }} /></div>) : (
        <>
          {built.error && <div className="panel__body"><EngineError error={built.error} context="Construction" /></div>}
          {res && !res.ok && <div className="panel__body"><EngineError error={res.error} context="Construction" /></div>}
          {res && res.ok && (
            <div className="table-wrap" style={{ maxHeight: 420 }}>
              <table className="grid">
                <thead><tr><th>Accrual start</th><th>Accrual end</th><th>Payment</th><th className="num">Days</th><th>Unadjusted</th></tr></thead>
                <tbody>
                  {res.data.periods.map((p) => (
                    <tr key={p.accrual_start} className={p.accrual_start < settlement && settlement < p.accrual_end ? 'is-active' : ''}>
                      <td>{formatLong(p.accrual_start)}</td><td>{formatLong(p.accrual_end)}</td><td>{formatLong(p.payment_date)}</td>
                      <td className="num">{daysBetween(p.accrual_start, p.accrual_end)}</td>
                      <td className="dim">{formatShort(p.unadjusted_start)} – {formatShort(p.unadjusted_end)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
          {res && res.ok && (
            <div className="panel__foot">
              Coupon rates are set by daily SOFR observations; open FRN for the fixings, projections and price. <Prov status="derived">engine schedule</Prov>
            </div>
          )}
        </>
      )}
    </Panel>
  );
}

function NoCurve({ onExample, onMarket, tradeDate }) {
  return (
    <Panel title="A SOFR curve is needed" style={{ marginBottom: 16 }}>
      <div className="stack" style={{ gap: 10 }}>
        <div className="dim" style={{ fontSize: 13 }}>
          Unpublished SOFR observations are projected, and every coupon discounted, on a SOFR OIS curve you supply. There is no
          free source of swap rates, so the terminal takes them from you: <b>MKT → Your data → New curve → OIS swap rates → SOFR</b>,
          then paste tenor and par swap rate in percent (or use “Fill with example rates”). The engine calibrates a discount
          curve and declares its zero-basis overnight projection; FRN uses both.
        </div>
        <div className="row">
          <button type="button" className="btn btn--primary btn--sm" onClick={onExample}>Build a SOFR curve from example rates</button>
          <button type="button" className="btn btn--sm" onClick={onMarket}>Open MKT</button>
          <span className="dim" style={{ fontSize: 12 }}>
            Example rates are hypothetical levels, not market data; the curve is dated {formatLong(tradeDate)} and marked “Example rates”.
          </span>
        </div>
      </div>
    </Panel>
  );
}

function Chooser({ frns, onSelect, onNew }) {
  return (
    <Panel title="Floating-rate notes" style={{ marginBottom: 16 }}>
      <div className="row">
        {frns.map((s) => (
          <button key={s.id} type="button" className="btn" onClick={() => onSelect(s.id)}>{s.ticker || s.name}</button>
        ))}
        <button type="button" className="btn btn--ghost" onClick={onNew}><Icon name="plus" /> New FRN</button>
      </div>
    </Panel>
  );
}

export default function Frn({ fn }) {
  const { engine, selected, universe, settlement, tradeDate, curves, curveDefs, datasets, saveCurve, loadOne, loading } = useWorkspace();
  const { state, dispatch } = useStore();
  const [creating, setCreating] = useState(false);
  const [editing, setEditing] = useState(false);
  const [quotes, setQuotes] = useState({});
  const [curvePick, setCurvePick] = useState(null);
  const frns = universe.filter((s) => s.kind === 'floating');
  const sec = selected && selected.kind === 'floating' ? selected : null;

  // Discount curves in USD; an OIS SOFR curve carries its own declared
  // overnight projection, any other discount curve is used at zero basis.
  const choices = Object.values(curveDefs).filter((d) => d.currency === 'USD' && d.role === 'discount');
  const usdPick = state.spreadCurve.USD && choices.find((d) => d.id === state.spreadCurve.USD);
  const curveDef = choices.find((d) => d.id === curvePick)
    || choices.find((d) => d.method === 'ois' && d.index === SOFR && d.id === (usdPick && usdPick.id))
    || choices.find((d) => d.method === 'ois' && d.index === SOFR) || usdPick || choices[0] || null;
  const curve = curveDef ? curves[curveDef.id] : null;
  const history = datasets['sofr-history'];
  const histSig = history ? `${history.asOf}|${history.status}|${history.fetchedAt || ''}|${history.revision || ''}|${history.points.length}` : '';

  const face = state.quote.face || '1000000';
  const quote = (sec && quotes[sec.id]) || (sec && defaultQuote(sec));
  const dq = useDebounced(quote && `${quote.kind}|${quote.text}`);
  const dFace = useDebounced(face);
  const ready = !!(sec && curve && curve.recipe && !curve.building && history && quoteValid(quote)
    && isDecimal(dFace) && Number(dFace) > 0);
  const { data: stale, error, busy, current } = useComputation(
    () => (ready ? analyseFrn(engine, { sec, tradeDate, settlement, curve, history, quote, face: dFace }) : null),
    [ready, sec, settlement, tradeDate, dq, dFace, curve && curve.key && curve.key.id, curve && curve.key && curve.key.version, histSig],
  );
  const data = current ? stale : null;
  const r = data && !data.fatal ? data : null;

  const setQuote = (q) => setQuotes((all) => ({ ...all, [sec.id]: { ...quote, ...q, opening: false } }));
  const switchKind = (kind) => {
    if (!r) return setQuote({ kind, text: '' });
    return setQuote({ kind, text: kind === 'price' ? r.clean.toFixed(6) : (r.dm * 10000).toFixed(3) });
  };

  const buildExample = () => {
    const existing = curveDefs[EXAMPLE_CURVE_ID];
    if (!existing) {
      saveCurve({ ...customCurveDef({ id: EXAMPLE_CURVE_ID, label: 'SOFR OIS', currency: 'USD', method: 'ois',
        asOf: tradeDate, points: examplePoints(SOFR), index: SOFR }), example: true });
    }
    setCurvePick(EXAMPLE_CURVE_ID);
  };

  const create = (fields) => {
    const id = `FRN-${Date.now().toString(36).toUpperCase()}`;
    const short = (fields.name || '').trim().split(/\s+/)[0] || 'FRN';
    const security = frnSecurity({ ...fields, id, short, source: 'custom', issuer: fields.issuer || 'Your floating-rate note' });
    dispatch({ type: 'custom', security, select: true });
    setCreating(false);
  };
  const applyEdit = (fields) => {
    dispatch({ type: 'custom', security: { ...sec, ...fields, kind: 'floating' }, select: true });
    setEditing(false);
  };

  const head = (
    <ScreenHead fn={fn} lede={LEDE}>
      {choices.length > 0 && (
        <label className="field" style={{ width: 230 }}>
          <span>SOFR curve</span>
          <Select value={curveDef ? curveDef.id : ''} onChange={setCurvePick} aria-label="SOFR curve"
            options={choices.map((d) => [d.id, `${d.label}${d.example ? ' (example)' : d.custom ? ' (yours)' : ''}`])} />
        </label>
      )}
      <button type="button" className="btn" onClick={() => setCreating((v) => !v)} aria-expanded={creating}><Icon name="plus" /> New FRN</button>
    </ScreenHead>
  );
  const form = creating && (
    <div style={{ marginBottom: 16 }}>
      <FrnTermsForm tradeDate={tradeDate} onApply={create} onCancel={() => setCreating(false)} />
    </div>
  );

  if (!sec) {
    return (
      <div className="screen">
        {head}
        {form}
        <div className="note" style={{ marginBottom: 12 }}>
          {selected ? `${selected.ticker || selected.name} is not a floating-rate note.` : 'No security selected.'} FRN analyses SOFR
          floaters. Choose one, create one, or import rows with an Index column on SEC.
        </div>
        <Chooser frns={frns} onSelect={(id) => dispatch({ type: 'select', id })} onNew={() => setCreating(true)} />
      </div>
    );
  }

  const scale = Number(face) / Number(sec.face || 100);
  const fatal = (data && data.fatal) || error;
  const curveStatus = curveDef && curveDef.example ? 'derived' : curveDef && curveDef.custom ? 'manual' : 'live';
  const zeroBasis = curveDef && !(curveDef.method === 'ois' && curveDef.index === (sec.index || SOFR));
  const cur = r && r.current;
  const cutoff = r && r.market.cutoffs[0];

  return (
    <div className="screen">
      {head}
      {form}
      <SecurityBand sec={sec} settlement={settlement} />
      <div className="row" style={{ marginBottom: 12 }}>
        <button type="button" className="btn btn--sm" onClick={() => setEditing((v) => !v)} aria-expanded={editing}>
          {editing ? 'Close terms' : 'Edit terms'}
        </button>
        {frns.filter((s) => s.id !== sec.id).map((s) => (
          <button key={s.id} type="button" className="btn btn--ghost btn--sm" onClick={() => dispatch({ type: 'select', id: s.id })}>{s.ticker || s.name}</button>
        ))}
      </div>
      {editing && (
        <div style={{ marginBottom: 16 }}>
          <FrnTermsForm sec={sec} tradeDate={tradeDate} onApply={applyEdit} onCancel={() => setEditing(false)} readOnly={sec.source === 'auction'} />
        </div>
      )}

      {!curveDef && <NoCurve tradeDate={tradeDate} onExample={buildExample} onMarket={() => dispatch({ type: 'screen', screen: 'MKT' })} />}
      {!history && <div className="note note--warn" style={{ marginBottom: 16 }}>SOFR history is not loaded. Load it under MKT → NY Fed.</div>}

      {curveDef && (
        <section className="panel" aria-label="Quote" style={{ marginBottom: 16 }}>
          <div className="panel__body">
            <div className="row" style={{ alignItems: 'flex-start', gap: 16 }}>
              <Field label="Quote as" as="div">
                <Seg value={quote.kind} onChange={switchKind} options={[['price', 'Price'], ['dm', 'Discount margin bp']]} label="Quote type" />
              </Field>
              <Field label={quote.kind === 'price' ? 'Clean price (decimal or 32nds)' : 'Discount margin, bp'}
                hint={quote.opening ? 'Opening quote: DM at the quoted margin (no market price). Type to replace.' : null}>
                <input className="input input--quote" style={{ width: 200 }} value={quote.text}
                  aria-invalid={quote.text !== '' && !quoteValid(quote)} onChange={(e) => setQuote({ text: e.target.value })} />
              </Field>
              <Field label="Face amount">
                <input className="input input--num" style={{ width: 150 }} value={face}
                  aria-invalid={!(isDecimal(face) && Number(face) > 0)}
                  onChange={(e) => dispatch({ type: 'quote', quote: { face: e.target.value.replace(/[^\d.]/g, '') } })} />
              </Field>
              <div className="grow" />
              <div className="dim" style={{ fontSize: 12, textAlign: 'right' }}>
                {busy || (curve && curve.building) ? <><Spinner /> Calculating</> : r ? 'Calculated' : ''}
                <div>
                  <Prov status={curveStatus}>{curveDef.label}{curveDef.example ? ' · example rates' : ''} · {formatLong(curveDef.asOf)}</Prov>
                </div>
                {history && (
                  <div><Prov status={history.status}>SOFR fixings to {formatLong(history.asOf)}</Prov></div>
                )}
              </div>
            </div>
          </div>
        </section>
      )}

      {curve && curve.error && <EngineError error={curve.error} context="SOFR curve" />}
      {curveDef && curveDef.asOf !== tradeDate && (
        <div className="note note--warn" style={{ marginBottom: 16 }}>
          The SOFR curve is dated {formatLong(curveDef.asOf)} and the trade date is {formatLong(tradeDate)}. Fixings are taken
          to the trade date and the rest projected from the curve, so the two should agree: set the curve date under MKT → Your data.
        </div>
      )}
      {fatal && <EngineError error={fatal} context="FRN" />}
      {data && data.missing && (
        <div className="note note--warn" style={{ marginBottom: 16 }}>
          {data.missing.count} {FRN_INDICES[data.missing.index] ? FRN_INDICES[data.missing.index].label : data.missing.index} fixing
          {data.missing.count === 1 ? ' was' : 's were'} due by the end of {formatLong(tradeDate)} but {data.missing.count === 1 ? 'is' : 'are'} not
          in the loaded history ({formatLong(data.missing.first)}{data.missing.last !== data.missing.first ? ` to ${formatLong(data.missing.last)}` : ''}).
          Published history is never projected. {history ? `The history runs ${formatLong(history.points[0].date)} to ${formatLong(history.asOf)}.` : ''}{' '}
          <button type="button" className="btn btn--sm" disabled={!!loading['sofr-history']} onClick={() => loadOne('sofr-history')}>
            {loading['sofr-history'] ? 'Loading…' : 'Reload SOFR history'}
          </button>
        </div>
      )}
      {r && r.callsIgnored && (
        <div className="note note--warn" style={{ marginBottom: 16 }}>
          This note is callable. Figures below are to maturity with the calls removed; the workout table gives the discount
          margin to each legal redemption date.
        </div>
      )}

      {r && (
        <>
          <div className="panel" style={{ marginBottom: 16 }}>
            <div className="figures">
              <Figure label="Clean price" value={fmt(r.clean, 6)} sub={`Dirty ${fmt(r.dirty, 6)}`} />
              <Figure label="Discount margin" value={signedBp(r.dm, 2)} unit="bp" sub={`quoted margin ${sec.spreadBp} bp`} />
              <Figure label="Current coupon index" value={cur && cur.rate != null ? pct(cur.rate, 4) : '—'} unit="%"
                sub={cur ? `compounded SOFR, ${cur.published} of ${cur.observations.length} fixed` : ''} />
              <Figure label="Spread duration" value={r.dmRisk ? fmt(r.dmRisk.duration, 3) : '—'} sub="from DM ± 1 bp" />
              <Figure label={`DM01 for ${money(face, 0)}`} value={r.dmRisk ? signed(r.dmRisk.dm01, 2) : '—'} sub="price change, +1 bp DM" />
              <Figure label={`IR01 for ${money(face, 0)}`} value={r.curveRisk ? signed(r.curveRisk.ir01, 2) : '—'} sub="+1 bp SOFR curve, same DM" />
            </div>
          </div>

          <div className="cols cols--3" style={{ marginBottom: 16 }}>
            <Panel title="Price">
              <KV rows={[
                ['Clean price', fmt(r.clean, 6), quote.kind === 'price' ? 'your quote' : `at DM ${fmt(Number(quote.text), 3)} bp`],
                ['In 32nds', formatThirtySeconds(r.clean)],
                ['Accrued interest', fmt(r.accrued, 6), cutoff
                  ? `${daysBetween(cutoff.period_start, settlement)} days; SOFR compounded to ${formatLong(cutoff.weight_end)} (shifted)`
                  : cur ? `${Math.max(0, daysBetween(cur.accrualStart, settlement))} days since ${formatLong(cur.accrualStart)}` : null],
                ['Dirty price', fmt(r.dirty, 6)],
                ['Invoice for face', money(r.dirty * Number(face) / 100, 2), `${money(face, 0)} ${sec.currency} face, dirty × face / 100`],
                ['Discount margin', `${signedBp(r.dm, 3)} bp`, `over ${r.market.discountKey.id}, Act/365F continuous`],
                ['DM inverse', `${r.inverse.iterations} iterations`, `price residual ${r.inverse.residual.toExponential(1)}`],
              ]} />
            </Panel>
            <Panel title="Current coupon" note={cur ? `${cur.published} fixed · ${cur.projected} projected` : ''}>
              {cur ? (
                <KV rows={[
                  ['Interest period', `${formatShort(cur.accrualStart)} – ${formatShort(cur.accrualEnd)}`, `${daysBetween(cur.accrualStart, cur.accrualEnd)} days, pays ${formatLong(cur.paymentDate)}`],
                  ['Observation window', cur.observations.length ? `${formatShort(cur.observations[0].date)} – ${formatShort(cur.observations[cur.observations.length - 1].date)}` : '—',
                    `${sec.lookbackDays} business days ${sec.observationShift === false ? 'lookback (lag)' : 'shifted'}`],
                  ['Published fixings', `${cur.published}`, 'NY Fed SOFR, final'],
                  ['Projected observations', `${cur.projected}`, `from ${r.market.projectionKey.id}`],
                  ['Compounded SOFR, period', `${pct(cur.rate, 5)} %`, 'known fixings with the projected rest'],
                  ['Margin', `${sec.spreadBp} bp`, 'simple, outside the compounding'],
                  ['Coupon per 100', fmt(Number(cur.amount), 6), `${money(Number(cur.amount) * scale, 2)} for face`],
                ]} />
              ) : <Empty>No coupon owed.</Empty>}
            </Panel>
            <Panel title="Risk" note="engine re-pricing">
              <KV rows={[
                ['DM01 per 100', r.dmRisk ? signed(r.dmRisk.dm01Per100, 6) : '—', 'dirty, DM ± 1 bp, coupons held'],
                ['Spread duration', r.dmRisk ? fmt(r.dmRisk.duration, 4) : '—'],
                ['IR01 per 100', r.curveRisk ? signed(r.curveRisk.ir01Per100, 6) : '—', 'SOFR zero curve ± 1 bp, same DM'],
                ['Rate duration', r.curveRisk ? fmt(r.curveRisk.duration, 4) : '—', 'coupons re-projected on the shifted curve'],
                ['Current coupon, curve −1 / +1 bp', r.curveRisk ? `${fmt(Number(r.curveRisk.nextCoupon.down), 5)} / ${fmt(Number(r.curveRisk.nextCoupon.up), 5)}` : '—', 'per 100'],
              ]} />
              <EngineError error={r.errors.dmRisk} context="DM risk" />
              <EngineError error={r.errors.curveRisk} context="Curve risk" />
            </Panel>
          </div>

          {cur && cur.observations.length > 0 && <Fixings cur={cur} cutoff={cutoff} tradeDate={tradeDate} />}

          <Panel title="Projected cash flows" flush style={{ marginBottom: 16 }}
            note={`per ${money(face, 0)} ${sec.currency} face; coupons from engine projection`}
            foot={`Fixed: every observation published. Partly fixed: published to date, the rest projected. Projected: forecast on ${r.market.projectionKey.id}. Discounted at the DM over ${r.market.discountKey.id}.`}>
            <div className="table-wrap" style={{ maxHeight: 460 }}>
              <table className="grid">
                <thead>
                  <tr><th>Payment</th><th>Interest period</th><th className="num">Days</th><th className="num">SOFR comp. %</th>
                    <th className="num">Margin bp</th><th>Rate</th><th className="num">Coupon</th><th className="num">Principal</th><th className="num">Total</th></tr>
                </thead>
                <tbody>
                  {r.coupons.map((c) => {
                    const principal = r.principal.filter((p) => p.date === c.paymentDate).reduce((a, p) => a + Number(p.amount), 0);
                    return (
                      <tr key={c.accrualStart} className={c === cur ? 'is-active' : ''}>
                        <td>{formatLong(c.paymentDate)}</td>
                        <td className="dim">{formatShort(c.accrualStart)} – {formatShort(c.accrualEnd)}</td>
                        <td className="num">{daysBetween(c.accrualStart, c.accrualEnd)}</td>
                        <td className="num">{c.rate != null ? pct(c.rate, 4) : '—'}</td>
                        <td className="num">{sec.spreadBp}</td>
                        <td><RateOrigin row={c} /></td>
                        <td className="num">{money(Number(c.amount) * scale)}</td>
                        <td className="num">{principal ? money(principal * scale) : ''}</td>
                        <td className="num">{money((Number(c.amount) + principal) * scale)}</td>
                      </tr>
                    );
                  })}
                  {r.principal.filter((p) => !r.coupons.some((c) => c.paymentDate === p.date)).map((p) => (
                    <tr key={`p${p.date}`}>
                      <td>{formatLong(p.date)}</td><td className="dim">principal</td><td /><td /><td /><td /><td />
                      <td className="num">{money(Number(p.amount) * scale)}</td>
                      <td className="num">{money(Number(p.amount) * scale)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </Panel>

          <div className="cols cols--wide" style={{ marginBottom: 16 }}>
            <Panel title="Price and discount margin" note="engine prices at shifted DM, coupons held">
              {r.ladder ? (
                <LineChart height={260}
                  series={[{ id: 'pdm', label: 'Clean price', color: '--s2',
                    points: r.ladder.map((l) => ({ x: l.dm * 10000, y: l.clean })), markers: [{ x: r.dm * 10000, y: r.clean }] }]}
                  xFormat={(v) => `${v.toFixed(0)}`} yFormat={(v) => v.toFixed(2)} xLabel="DM, bp"
                  tipX={(v) => `DM ${v.toFixed(2)} bp`} tipY={(v) => fmt(v, 4)} refs={[{ x: r.dm * 10000, label: 'Quote' }]} />
              ) : <EngineError error={r.errors.ladder} context="Ladder" />}
            </Panel>
            <Panel title="DM ladder" flush>
              {r.ladder && (
                <div className="table-wrap">
                  <table className="grid ladder">
                    <thead><tr><th className="num">Shift</th><th className="num">DM bp</th><th className="num">Clean</th><th className="num">P&amp;L for face</th></tr></thead>
                    <tbody>
                      {r.ladder.map((l) => (
                        <tr key={l.bp} className={l.bp === 0 ? 'is-active' : ''}>
                          <td className="num">{l.bp === 0 ? 'quote' : `${l.bp > 0 ? '+' : ''}${l.bp} bp`}</td>
                          <td className="num">{fmt(l.dm * 10000, 2)}</td>
                          <td className="num">{fmt(l.clean, 4)}</td>
                          <td className={`num ${l.change > 0 ? 'pos' : l.change < 0 ? 'neg' : ''}`}>{signed(l.change * scale, 0)}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
            </Panel>
          </div>

          {r.workouts && (
            <Panel title="Workouts" flush style={{ marginBottom: 16 }}
              note="discount margin to each legal redemption, same clean price"
              foot="Each path keeps the projected coupons to its redemption; a call inside a period pays the coupon accrued to it, compounded to the call date less the lookback. No path is chosen as worst: the engine gives no stochastic exercise view for floaters.">
              <div className="table-wrap">
                <table className="grid">
                  <thead><tr><th>Path</th><th>Date</th><th className="num" title="Price plus any partial coupon the exercise pays; a scheduled coupon on the same date is paid as usual">Redemption cash per 100</th><th className="num">DM bp</th><th className="num">Residual</th><th /></tr></thead>
                  <tbody>
                    {r.workouts.map((w) => (
                      <tr key={`${w.kind}${w.provision ?? ''}${w.date || ''}`}>
                        <td>{w.kind === 'maturity' ? 'Maturity' : `${w.right === 'put' ? 'Put' : 'Call'} at ${fmt(Number(w.price), 3)}`}</td>
                        <td>{formatLong(w.date || sec.maturity)}</td>
                        <td className="num">{w.exercise ? fmt(Number(w.exercise.settlement_cash), 4) : fmt(Number(sec.redemption || 100), 4)}</td>
                        <td className="num">{w.error ? '—' : signedBp(w.dm, 2)}</td>
                        <td className="num dim">{w.error ? '' : w.residual.toExponential(1)}</td>
                        <td>{w.error && <span className="neg" title={w.error.message}>{w.error.code}: {w.error.message}</span>}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </Panel>
          )}

          <Panel title="Inputs and conventions" style={{ marginBottom: 16 }}>
            <KV rows={[
              ['SOFR fixings', history ? <Prov status={history.status}>{history.points.length} NY Fed fixings, {formatShort(history.points[0].date)} – {formatShort(history.asOf)}</Prov> : '—',
                'each taken as published and known at 08:00 New York on the next business day'],
              ['Information cutoff', r.market.knownAt, `fixings published by then are used; later observations are projected (${r.market.fixings} supplied, ${r.market.publications} publication dates)`],
              ['Projection', <Prov status={curveStatus}>{r.market.projectionKey.id}</Prov>,
                zeroBasis ? `zero-basis overnight view of ${curveDef.label}, assumed (not an OIS calibration for SOFR)`
                  : `zero-basis overnight view of ${curveDef.label}, as its OIS calibration declares`],
              ['Discounting', <Prov status={curveStatus}>{r.market.discountKey.id}</Prov>,
                `${curveDef.example ? 'example rates, not market data' : 'your curve'}, collateral role`],
              ['Discount margin', 'Act/365F continuous', 'added to the collateral discount rate; projected coupons held fixed'],
              ['Accrued, observation shift', cutoff ? `weights to ${formatLong(cutoff.weight_end)}` : 'not needed',
                'settlement less the lookback on the observation calendar: an app convention, stated to the engine'],
              ['Coupon', `${frnLabel(sec)}, ${dayCountLabel(sec.dayCount)}`, `lookback ${sec.lookbackDays}, lockout ${sec.lockoutDays || 0}, payment delay ${sec.paymentDelayDays || 0} business days; unrounded`],
            ]} />
          </Panel>
        </>
      )}
      {!r && !fatal && curveDef && !(curve && curve.error) && (
        <Empty>{!curve || curve.building ? 'Building the SOFR curve…' : busy ? <Spinner /> : !(isDecimal(dFace) && Number(dFace) > 0) ? 'Enter a positive face amount.' : 'Enter a quote to analyse this note.'}</Empty>
      )}
    </div>
  );
}

/** Daily SOFR in the current coupon: published fixings and projected days. */
function Fixings({ cur, cutoff, tradeDate }) {
  const obs = cur.observations;
  const pts = obs.map((o) => ({ x: epochDay(o.date), y: Number(o.rate) * 100, o }));
  const known = pts.filter((p) => p.o.origin === 'published');
  const lastKnown = known[known.length - 1];
  const rest = pts.filter((p) => p.o.origin !== 'published');
  const series = [
    { id: 'pub', label: 'Published', color: '--s1', points: known },
    { id: 'proj', label: 'Projected', color: '--s2', dash: true, points: lastKnown ? [lastKnown, ...rest] : rest },
  ].filter((s) => s.points.length);
  // One reference line: the trade date. The accrued cutoff sits a day or two
  // before it and is named in the panel note instead of crowding the label.
  const refs = [{ x: epochDay(tradeDate), label: 'Trade date' }];
  return (
    <div className="cols cols--wide" style={{ marginBottom: 16 }}>
      <Panel title="Current coupon: daily SOFR"
        note={`published fixings, then projection${cutoff ? `; accrued compounds to ${formatLong(cutoff.weight_end)}` : ''}`}>
        <LineChart height={240} series={series} refs={refs}
          xFormat={(v) => formatShort(isoOfDay(v)).slice(0, 5)} yFormat={(v) => v.toFixed(2)}
          tipX={(v, p) => formatLong(p && p.o ? p.o.date : isoOfDay(v))} tipY={(v) => `${v.toFixed(4)}%`} yLabel="%" />
      </Panel>
      <Panel title="Observations" flush note={`${obs.length} business days`}>
        <div className="table-wrap" style={{ maxHeight: 286 }}>
          <table className="grid">
            <thead><tr><th>Observed</th><th className="num">SOFR %</th><th className="num">Days</th><th>Origin</th></tr></thead>
            <tbody>
              {obs.map((o) => (
                <tr key={o.date}>
                  <td>{formatShort(o.date)}</td>
                  <td className="num">{pct(o.rate, o.origin === 'published' ? 2 : 4)}</td>
                  <td className="num dim">{daysBetween(o.weightStart, o.weightEnd)}</td>
                  <td>
                    <span className={`rate-origin rate-origin--${o.origin === 'published' ? 'fixed' : 'projected'}`}
                      title={o.publishedAt ? `published ${o.publishedAt}` : undefined}>
                      <i aria-hidden="true" />{OBS_TEXT[o.origin]}
                    </span>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </Panel>
    </div>
  );
}

