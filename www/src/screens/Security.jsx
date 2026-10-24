import React, { useEffect, useMemo, useState } from 'react';
import { useWorkspace, useComputation } from '../state/workspace';
import { useStore } from '../state/store';
import * as R from '../lib/requests';
import { bondTerms } from '../lib/terms';
import { CALENDAR_RULES } from '../lib/calendars';
import { isIso, formatLong } from '../lib/dates';
import { isDecimal } from '../lib/decimal';
import { Panel, Field, Select, EngineError, Icon, Tabs, Prov } from '../ui/primitives';
import SecurityBand, { dayCountLabel } from '../ui/SecurityBand';
import ScreenHead from '../ui/ScreenHead';
import { JsonView } from '../ui/Inspector';
import { fmt } from '../lib/format';
import BondImport from './BondImport';
import { FrnTermsForm, FrnContract } from './Frn';

const DAY_COUNTS = ['ActActIcma', 'Thirty360US', 'Thirty360E', 'Act365Fixed', 'Act360', 'ActActIsda'];
const FREQS = ['SemiAnnual', 'Annual', 'Quarterly', 'Monthly'];
const CURRENCIES = ['USD', 'EUR', 'CAD', 'GBP', 'JPY'];

function SecurityList({ universe, selected, onSelect }) {
  const [q, setQ] = useState('');
  const [group, setGroup] = useState('coupon');
  const rows = universe.filter((s) => {
    if (group === 'coupon' && !(s.source === 'auction' && s.kind === 'fixed')) return false;
    if (group === 'bills' && s.kind !== 'bill') return false;
    if (group === 'other' && s.source === 'auction') return false;
    const t = q.trim().toUpperCase();
    return !t || [s.cusip, s.isin, s.ticker, s.name].filter(Boolean).some((v) => v.toUpperCase().includes(t));
  });
  return (
    <Panel title="Securities" flush>
      <div style={{ padding: '10px 12px 0' }}>
        <input className="input" placeholder="Filter by CUSIP, ISIN or name" value={q} onChange={(e) => setQ(e.target.value)} aria-label="Filter securities" />
      </div>
      <div style={{ padding: '0 6px' }}>
        <Tabs value={group} onChange={setGroup} options={[['coupon', 'Treasuries'], ['bills', 'Bills'], ['other', 'Yours and examples']]} label="Group" />
      </div>
      <div className="table-wrap" style={{ maxHeight: 560 }}>
        <table className="grid">
          <tbody>
            {rows.map((s) => (
              <tr key={s.id} className={selected && s.id === selected.id ? 'is-active' : ''} style={{ cursor: 'pointer' }}
                onClick={() => onSelect(s)}>
                <td>
                  <button type="button" className="row-button" aria-pressed={!!selected && s.id === selected.id}
                    onClick={(e) => { e.stopPropagation(); onSelect(s); }}>
                    <div>{s.ticker || s.name}</div>
                    <div className="muted" style={{ fontSize: 11.5 }}>
                      {s.cusip || s.isin || (s.imported ? 'Imported' : s.source === 'custom' ? 'Custom' : 'Example')}
                    </div>
                  </button>
                </td>
                <td className="num dim">{isIso(s.maturity) ? formatLong(s.maturity) : ''}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </Panel>
  );
}

function RowsEditor({ title, rows, columns, onChange, empty }) {
  const update = (i, key, value) => onChange(rows.map((r, j) => (j === i ? { ...r, [key]: value } : r)));
  return (
    <div className="field field--wide">
      <span>{title}</span>
      <table className="grid">
        <thead><tr>{columns.map((c) => <th key={c.key}>{c.label}</th>)}<th /></tr></thead>
        <tbody>
          {rows.map((r, i) => (
            <tr key={i}>
              {columns.map((c) => (
                <td key={c.key} className="edit">
                  <input value={r[c.key]} style={{ textAlign: 'left' }} aria-label={`${c.label} ${i + 1}`}
                    aria-invalid={c.check && r[c.key] !== '' && !c.check(r[c.key])} onChange={(e) => update(i, c.key, e.target.value)} />
                </td>
              ))}
              <td style={{ width: 34 }}>
                <button type="button" className="icon-btn row-action" style={{ width: 24, height: 24 }} aria-label="Remove row"
                  onClick={() => onChange(rows.filter((_, j) => j !== i))}><Icon name="close" /></button>
              </td>
            </tr>
          ))}
          {rows.length === 0 && <tr><td colSpan={columns.length + 1} className="muted">{empty}</td></tr>}
        </tbody>
      </table>
      <div>
        <button type="button" className="btn btn--sm btn--ghost" onClick={() => onChange([...rows, Object.fromEntries(columns.map((c) => [c.key, '']))])}>
          <Icon name="plus" /> Add row
        </button>
      </div>
    </div>
  );
}

// isDecimal accepts thousands separators ("1,000"); Number does not.
const positive = (text) => Number(String(text).replace(/,/g, '')) > 0;

function TermsEditor({ sec, onApply }) {
  const [draft, setDraft] = useState(sec);
  useEffect(() => setDraft(sec), [sec]);
  const set = (k) => (v) => setDraft((d) => ({ ...d, [k]: v }));
  const text = (k) => ({ value: draft[k] ?? '', onChange: (e) => set(k)(e.target.value) });
  const dirty = JSON.stringify(draft) !== JSON.stringify(sec);
  const optionalDate = (d) => !d || isIso(d);
  const rowsValid = (rows, valueKey) => (rows || []).every((r) => isIso(r.date) && isDecimal(r[valueKey] || ''));
  const valid = isDecimal(draft.coupon || '') && isIso(draft.issueDate) && isIso(draft.maturity) && draft.maturity > draft.issueDate
    && optionalDate(draft.datedDate) && optionalDate(draft.firstCouponDate)
    && isDecimal(draft.face || '') && positive(draft.face) && isDecimal(draft.redemption || '') && positive(draft.redemption)
    && rowsValid(draft.calls, 'price') && rowsValid(draft.steps, 'coupon') && rowsValid(draft.sinking, 'amount');
  return (
    <Panel title="Terms" note={sec.source === 'auction' ? 'Treasury terms are read-only; copy to edit' : 'editable'}
      actions={sec.source !== 'auction' && (
        <button type="button" className="btn btn--primary btn--sm" disabled={!dirty || !valid} onClick={() => onApply(draft)}>Apply terms</button>
      )}>
      <fieldset disabled={sec.source === 'auction'} style={{ border: 0, padding: 0, margin: 0 }}>
        <div className="form">
          <Field label="Name" wide><input {...text('name')} /></Field>
          <Field label="Coupon %"><input className="input--num" {...text('coupon')} aria-invalid={!isDecimal(draft.coupon || '')} /></Field>
          <Field label="Currency"><Select value={draft.currency} onChange={set('currency')} options={CURRENCIES} /></Field>
          <Field label="Issue date"><input type="date" {...text('issueDate')} /></Field>
          <Field label="Dated date"><input type="date" {...text('datedDate')} /></Field>
          <Field label="First coupon"><input type="date" value={draft.firstCouponDate || ''} onChange={(e) => set('firstCouponDate')(e.target.value || null)} /></Field>
          <Field label="Maturity"><input type="date" {...text('maturity')} /></Field>
          <Field label="Frequency"><Select value={draft.frequency} onChange={set('frequency')} options={FREQS} /></Field>
          <Field label="Day count"><Select value={draft.dayCount} onChange={set('dayCount')} options={DAY_COUNTS.map((d) => [d, dayCountLabel(d)])} /></Field>
          <Field label="Calendar"><Select value={draft.calendar} onChange={set('calendar')} options={Object.entries(CALENDAR_RULES).map(([k, v]) => [k, v.label])} /></Field>
          <Field label="Settlement days"><input className="input--num" value={draft.settlementDays} onChange={(e) => set('settlementDays')(Number(e.target.value.replace(/\D/g, '')) || 0)} /></Field>
          <Field label="Redemption % of face"><input className="input--num" {...text('redemption')} aria-invalid={!isDecimal(draft.redemption || '')} /></Field>
          <Field label="Face (quote unit)"><input className="input--num" {...text('face')} aria-invalid={!isDecimal(draft.face || '')} /></Field>
        </div>
        {sec.source !== 'auction' && (
          <div className="form" style={{ marginTop: 14 }}>
            <RowsEditor title="Call schedule (issuer call at price, per 100)" rows={draft.calls || []}
              columns={[{ key: 'date', label: 'Date', check: isIso }, { key: 'price', label: 'Price', check: isDecimal }]}
              onChange={set('calls')} empty="No calls: a bullet bond." />
            <RowsEditor title="Coupon steps (new coupon % from date)" rows={draft.steps || []}
              columns={[{ key: 'date', label: 'From', check: isIso }, { key: 'coupon', label: 'Coupon %', check: isDecimal }]}
              onChange={set('steps')} empty="Fixed coupon throughout." />
            <RowsEditor title="Sinking fund (face retired on date)" rows={draft.sinking || []}
              columns={[{ key: 'date', label: 'Date', check: isIso }, { key: 'amount', label: 'Face amount', check: isDecimal }]}
              onChange={set('sinking')} empty="Bullet principal at maturity." />
          </div>
        )}
      </fieldset>
    </Panel>
  );
}

export default function Security({ fn }) {
  const { engine, universe, selected: sec, settlement, tradeDate } = useWorkspace();
  const { dispatch } = useStore();
  const [view, setView] = useState('schedule');
  const [importing, setImporting] = useState(false);

  const built = useComputation(
    () => (sec && sec.kind === 'fixed' ? engine.call(R.bondCashflows(sec, settlement), { label: `${sec.ticker || sec.name} schedule` }) : null),
    [sec, settlement],
  );
  const terms = useMemo(() => {
    try {
      return sec && sec.kind === 'fixed' ? bondTerms(sec) : null;
    } catch (e) {
      return { error: e.message };
    }
  }, [sec]);

  const apply = (draft) => {
    const custom = { ...draft, source: draft.source === 'auction' ? 'custom' : draft.source };
    dispatch({ type: 'custom', security: custom, select: true });
  };
  const copy = () => {
    const id = `CUSTOM-${Date.now().toString(36).toUpperCase()}`;
    dispatch({ type: 'custom', select: true,
      security: { ...sec, id, source: 'custom', cusip: undefined, isin: undefined, auction: undefined, imported: undefined,
        issuer: 'Custom security', name: `${sec.name} (copy)`, ticker: `${sec.ticker} copy` } });
  };

  const cash = built.data && built.data.ok ? built.data.data : null;
  return (
    <div className="screen">
      <ScreenHead fn={fn} lede="The full contract each calculation uses. Treasuries come from auction results; examples, copies and bonds you import are editable.">
        <button type="button" className="btn" onClick={() => setImporting((v) => !v)} aria-expanded={importing}>
          <Icon name="upload" /> Import from Excel
        </button>
      </ScreenHead>
      {importing && <div style={{ marginBottom: 16 }}><BondImport onClose={() => setImporting(false)} /></div>}
      <div className="cols cols--side">
        <SecurityList universe={universe} selected={sec} onSelect={(s) => dispatch({ type: 'select', id: s.id })} />
        <div className="stack">
          <SecurityBand sec={sec} settlement={settlement} />
          {sec && (
            <div className="row">
              <button type="button" className="btn btn--sm" onClick={() => dispatch({ type: 'screen', screen: sec.kind === 'bill' ? 'BILL' : sec.kind === 'floating' ? 'FRN' : 'YAS' })}>Analyse</button>
              {(sec.kind === 'fixed' || sec.kind === 'floating') && <button type="button" className="btn btn--sm" onClick={copy}>Copy as custom security</button>}
              {sec.source === 'custom' && (
                <button type="button" className="btn btn--sm btn--ghost" onClick={() => { dispatch({ type: 'removeCustom', id: sec.id }); dispatch({ type: 'select', id: null }); }}>
                  <Icon name="trash" /> Delete
                </button>
              )}
              {sec.auction && (
                <span className="dim" style={{ fontSize: 12 }}>
                  Auctioned {formatLong(sec.auction.date)} at {sec.auction.price}{sec.auction.highYield ? `, high yield ${sec.auction.highYield}%` : ''}
                  {sec.auction.reopening ? ' (reopening)' : ''}
                </span>
              )}
            </div>
          )}
          {sec && sec.kind === 'fixed' && <TermsEditor sec={sec} onApply={apply} />}
          {sec && sec.kind === 'floating' && <FrnTermsForm sec={sec} tradeDate={tradeDate} onApply={(d) => apply({ ...sec, ...d })} />}
          {sec && sec.kind === 'floating' && <FrnContract sec={sec} settlement={settlement} />}
          {sec && sec.kind === 'fixed' && (
            <Panel title="Contract" flush actions={<Tabs value={view} onChange={setView} options={[['schedule', 'Generated schedule'], ['terms', 'BondTerms JSON']]} label="View" />}>
              {view === 'terms' ? (terms && !terms.error ? <JsonView value={terms} /> : <div className="panel__body"><EngineError error={{ code: 'invalid_input', message: terms && terms.error }} /></div>) : (
                <>
                  {built.error && <div className="panel__body"><EngineError error={built.error} context="Construction" /></div>}
                  {built.data && !built.data.ok && <div className="panel__body"><EngineError error={built.data.error} context="Construction" /></div>}
                  {cash && (
                    <div className="table-wrap" style={{ maxHeight: 420 }}>
                      <table className="grid">
                        <thead><tr><th>Accrual start</th><th>Accrual end</th><th>Payment</th><th className="num">Coupon</th><th className="num">Accrued at settle</th></tr></thead>
                        <tbody>
                          {cash.coupons.map((c) => (
                            <tr key={c.accrual_start}>
                              <td>{formatLong(c.accrual_start)}</td><td>{formatLong(c.accrual_end)}</td><td>{formatLong(c.payment_date)}</td>
                              <td className="num">{fmt(Number(c.amount), 6)}</td>
                              <td className="num dim">{Number(c.accrued) ? fmt(Number(c.accrued), 6) : ''}</td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    </div>
                  )}
                  {cash && <div className="panel__foot">Cash per {sec.face} face from settlement {formatLong(settlement)} · outstanding principal {cash.outstanding_principal} · <Prov status="derived">engine schedule</Prov></div>}
                </>
              )}
            </Panel>
          )}
        </div>
      </div>
    </div>
  );
}
