import React, { useMemo, useState } from 'react';
import { useWorkspace } from '../state/workspace';
import { useStore } from '../state/store';
import { parseBondTable, bondTemplate, BOND_COLUMNS } from '../lib/bondtable';
import { dayCountLabel } from '../ui/SecurityBand';
import { formatLong } from '../lib/dates';
import { Panel, Seg, Prov, Icon } from '../ui/primitives';

const FREQ = { Annual: 'Annual', SemiAnnual: 'Semi', Quarterly: 'Qtrly', Monthly: 'Monthly' };

function copy(text) {
  if (navigator.clipboard) navigator.clipboard.writeText(text).catch(() => {});
}

/** Paste bond reference data from Excel or a CSV; preview every row, then add them as your securities. */
export default function BondImport({ onClose }) {
  const { tradeDate, benchmarks, universe } = useWorkspace();
  const { dispatch } = useStore();
  const [text, setText] = useState('');
  const [order, setOrder] = useState('auto');
  const [done, setDone] = useState(null);
  const parsed = useMemo(
    () => (text.trim() ? parseBondTable(text, { tradeDate, dayFirst: order === 'auto' ? undefined : order === 'dmy' }) : null),
    [text, tradeDate, order],
  );
  // A row whose generated id is already in the workspace gets a fresh one, so adding
  // never replaces a security you have; the preview says which rows were renamed.
  const rows = useMemo(() => {
    if (!parsed) return [];
    const have = new Set(universe.map((s) => s.id));
    const reserved = new Set([...have, ...parsed.rows.map((r) => r.sec.id)]);
    return parsed.rows.map((r) => {
      if (!have.has(r.sec.id)) return r;
      let n = 2;
      while (reserved.has(`${r.sec.id}-${n}`)) n++;
      const id = `${r.sec.id}-${n}`;
      reserved.add(id);
      return { ...r, sec: { ...r.sec, id }, renamedFrom: r.sec.id };
    });
  }, [parsed, universe]);
  const mine = universe.filter((s) => s.imported);
  // Real securities make the best sample: the current on-the-run Treasuries.
  const sample = bondTemplate(benchmarks.map((b) => b.sec));
  const add = () => {
    const custom = Object.fromEntries(rows.map((r) => [r.sec.id, r.sec]));
    dispatch({ type: 'importData', data: { custom } });
    setDone({ count: rows.length, first: rows[0].sec.id });
    setText('');
  };
  const removeAll = () => {
    mine.forEach((s) => dispatch({ type: 'removeCustom', id: s.id }));
    setDone(null);
  };
  const slashDates = rows.length > 0 && /(?<!\d)\d{1,2}[/.-]\d{1,2}[/.-]\d{2,4}(?!\d)/.test(text);

  return (
    <Panel title="Bonds from a spreadsheet" note="paste rows from Excel, a CSV file or a terminal export"
      actions={(
        <>
          <button type="button" className="btn btn--ghost btn--sm" onClick={() => copy(sample)} title="Header and the current on-the-run Treasuries, tab-separated">
            <Icon name="copy" /> Copy template
          </button>
          <button type="button" className="btn btn--ghost btn--sm" onClick={() => setText(sample)}>Fill with current Treasuries</button>
          {onClose && <button type="button" className="icon-btn" aria-label="Close bond import" onClick={onClose}><Icon name="close" /></button>}
        </>
      )}>
      <div className="stack">
        <div className="dim" style={{ fontSize: 12.5 }}>
          The first line names the columns, in any order. Required: <b>Coupon</b> and <b>Maturity</b>. Optional:{' '}
          {BOND_COLUMNS.filter((c) => !c.required).map((c) => c.label).join(', ')}. Calls are written
          as <code>date@price</code> separated by semicolons. Missing conventions (currency, frequency, day count,
          settlement days, issue date) get the currency's market default, listed per row below; redemption defaults
          to 100, and a missing identifier, name, price or calls stay blank.
        </div>
        <div className="paste">
          <textarea value={text} onChange={(e) => { setText(e.target.value); setDone(null); }} spellCheck={false}
            aria-label="Pasted bond rows" style={{ minHeight: 140 }}
            placeholder={'CUSIP\tName\tCoupon\tMaturity\tPrice\n91282CRF0\tUS Treasury Note\t4.625\t08/15/2036\t96-16+'} />
        </div>
        {slashDates && (
          <div className="row" style={{ gap: 10, alignItems: 'center' }}>
            <span className="dim" style={{ fontSize: 12 }}>Dates like 03/04/2030 or 03-04-2030 read as</span>
            <Seg value={order} onChange={setOrder} label="Date order"
              options={[['auto', `Auto (${parsed.dayFirst ? 'day first' : 'month first'})`], ['mdy', 'Month first'], ['dmy', 'Day first']]} />
          </div>
        )}
        {parsed && parsed.unknown.length > 0 && (
          <div className="dim" style={{ fontSize: 12 }}>Columns ignored: {parsed.unknown.join(', ')}.</div>
        )}
        {parsed && parsed.errors.length > 0 && (
          <div className="note note--error">
            {parsed.errors.length} line{parsed.errors.length === 1 ? '' : 's'} skipped:
            <ul className="paste__errors" style={{ color: 'inherit' }}>
              {parsed.errors.slice(0, 8).map((e) => <li key={e.line}>Line {e.line}: {e.reason}</li>)}
            </ul>
          </div>
        )}
        {parsed && rows.length > 0 && (
          <div className="table-wrap" style={{ maxHeight: 320 }}>
            <table className="grid">
              <thead>
                <tr><th>Line</th><th>Security</th><th>Identifier</th><th className="num">Coupon</th><th>Maturity</th>
                  <th>Coupons</th><th>Day count</th><th className="num">Price</th><th>Defaults applied</th></tr>
              </thead>
              <tbody>
                {rows.map(({ line, sec, assumed, warnings, renamedFrom }) => (
                  <tr key={line}>
                    <td className="dim">{line}</td>
                    <td>{sec.name}{sec.calls.length > 0 && <span className="dim"> · {sec.calls.length} calls</span>}</td>
                    <td>{sec.cusip || sec.isin || <span className="dim">—</span>}</td>
                    <td className="num">{sec.kind === 'floating' ? `SOFR + ${sec.spreadBp} bp` : sec.coupon}</td>
                    <td>{formatLong(sec.maturity)}</td>
                    <td>{FREQ[sec.frequency]}</td>
                    <td>{dayCountLabel(sec.dayCount)}</td>
                    <td className="num">{sec.price || ''}</td>
                    <td className="dim" style={{ fontSize: 11.5, whiteSpace: 'normal', minWidth: 200 }}>
                      {assumed.join(', ')}
                      {renamedFrom && <div className="neg">{renamedFrom} is already in your securities; added as {sec.id}</div>}
                      {warnings.map((w) => <div key={w} className="neg">{w}</div>)}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
        <div className="row" style={{ gap: 8, alignItems: 'center' }}>
          {done && (
            <>
              <Prov status="manual">Added {done.count} bond{done.count === 1 ? '' : 's'}</Prov>
              <button type="button" className="btn btn--sm" onClick={() => { dispatch({ type: 'select', id: done.first }); dispatch({ type: 'screen', screen: 'YAS' }); }}>
                Analyse the first in YAS
              </button>
            </>
          )}
          {mine.length > 0 && (
            <span className="dim" style={{ fontSize: 12 }}>
              {mine.length} imported bond{mine.length === 1 ? '' : 's'} in this browser.{' '}
              <button type="button" className="btn btn--ghost btn--sm" onClick={removeAll}>Remove all imported</button>
            </span>
          )}
          <div className="grow" />
          <button type="button" className="btn btn--primary" disabled={!rows.length} onClick={add}>
            {rows.length ? `Add ${rows.length} bond${rows.length === 1 ? '' : 's'}` : 'Add bonds'}
          </button>
        </div>
      </div>
    </Panel>
  );
}
