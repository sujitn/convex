import React, { useState } from 'react';
import { Icon } from './primitives';
import { normaliseTenor, normaliseRate, tenorKey, parseTenorTable, formatTenorTable } from '../lib/paste';

const byTenor = (a, b) => tenorKey(a.tenor) - tenorKey(b.tenor);

function copy(text) {
  if (navigator.clipboard) navigator.clipboard.writeText(text).catch(() => {});
}

/** Points with `point` added, replacing any with the same maturity (12M and 1Y are one point). */
function upsert(points, point) {
  const key = tenorKey(point.tenor);
  return [...points.filter((p) => tenorKey(p.tenor) !== key), point].sort(byTenor);
}

/** Paste a tenor/rate table to replace the points or merge into them. */
export function PasteBox({ points, onApply, onClose, templateTenors, valueLabel = 'Rate %' }) {
  const [text, setText] = useState('');
  const parsed = text.trim() ? parseTenorTable(text) : null;
  const apply = (mode) => {
    if (!parsed || !parsed.points.length) return;
    if (mode === 'replace') onApply(parsed.points);
    else onApply(parsed.points.reduce((acc, p) => upsert(acc, { ...(acc.find((q) => tenorKey(q.tenor) === tenorKey(p.tenor)) || {}), ...p }), points));
    setText('');
    if (onClose) onClose();
  };
  // A two-column sheet to fill in Excel: the current points, or the usual tenors.
  const template = () => {
    const rows = points.length ? points : (templateTenors || []).map((tenor) => ({ tenor, value: '' }));
    copy(`Tenor\t${valueLabel}\n${formatTenorTable([...rows].sort(byTenor))}`);
  };
  return (
    <div className="paste">
      <textarea value={text} onChange={(e) => setText(e.target.value)} spellCheck={false} aria-label="Pasted tenors and rates"
        placeholder={`Copy two columns from Excel (tenor, ${valueLabel}) and paste here. Also reads CSV or typed text:\n1Y\t3.60\n5Y\t3.42\n10 yr, 3.62`} />
      <div className="row" style={{ gap: 8, alignItems: 'center' }}>
        <span className="dim" style={{ fontSize: 12 }}>
          {parsed ? `${parsed.points.length} point${parsed.points.length === 1 ? '' : 's'} read` : 'Paste or type a table'}
          {parsed && parsed.errors.length > 0 && <span className="neg">, {parsed.errors.length} skipped</span>}
        </span>
        <button type="button" className="btn btn--ghost btn--sm" onClick={template}
          title="A tenor and rate sheet to fill in Excel, then paste back">
          <Icon name="copy" /> Copy template
        </button>
        <div className="grow" />
        {onClose && <button type="button" className="btn btn--ghost btn--sm" onClick={onClose}>Cancel</button>}
        <button type="button" className="btn btn--sm" disabled={!parsed || !parsed.points.length} onClick={() => apply('merge')}>Merge</button>
        <button type="button" className="btn btn--primary btn--sm" disabled={!parsed || !parsed.points.length} onClick={() => apply('replace')}>Replace all</button>
      </div>
      {parsed && parsed.errors.length > 0 && (
        <ul className="paste__errors">
          {parsed.errors.slice(0, 6).map((e) => <li key={e.line}>Line {e.line}, “{e.text.trim()}”: {e.reason}</li>)}
        </ul>
      )}
    </div>
  );
}

/**
 * Editable tenor/rate grid: edit a rate, remove a tenor, add a tenor, paste a
 * table, copy it out. `pasteOpen` starts with the paste box showing (an empty
 * curve); `templateTenors` seeds the copyable Excel template.
 */
export default function PointsEditor({ points, onChange, valueLabel = 'Rate %', maxHeight = 360, pasteOpen = false, templateTenors }) {
  const [draft, setDraft] = useState({});
  const [tenor, setTenor] = useState('');
  const [value, setValue] = useState('');
  const [pasting, setPasting] = useState(pasteOpen);
  const rows = [...points].sort(byTenor);
  const commit = (t, text) => {
    setDraft((d) => ({ ...d, [t]: undefined }));
    const v = normaliseRate(text);
    if (v === null) return;
    onChange(points.map((p) => (p.tenor === t ? { ...p, value: v } : p)));
  };
  const newTenor = normaliseTenor(tenor);
  const newValue = normaliseRate(value);
  const canAdd = newTenor && newValue !== null;
  const add = () => {
    if (!canAdd) return;
    onChange(upsert(points, { tenor: newTenor, value: newValue }));
    setTenor('');
    setValue('');
  };
  return (
    <div className="points">
      <div className="table-wrap" style={{ maxHeight }}>
        <table className="grid">
          <thead><tr><th>Tenor</th><th className="num">{valueLabel}</th><th /></tr></thead>
          <tbody>
            {rows.map((p) => (
              <tr key={p.tenor}>
                <td>{p.tenor}</td>
                <td className="edit">
                  <input value={draft[p.tenor] ?? p.value} aria-label={`${p.tenor} rate`}
                    aria-invalid={draft[p.tenor] !== undefined && normaliseRate(draft[p.tenor]) === null}
                    onChange={(e) => setDraft((d) => ({ ...d, [p.tenor]: e.target.value }))}
                    onBlur={(e) => draft[p.tenor] !== undefined && commit(p.tenor, e.target.value)}
                    onKeyDown={(e) => e.key === 'Enter' && e.currentTarget.blur()} />
                </td>
                <td style={{ width: 34 }}>
                  <button type="button" className="icon-btn row-action" style={{ width: 24, height: 24 }}
                    aria-label={`Remove ${p.tenor}`} onClick={() => onChange(points.filter((x) => x.tenor !== p.tenor))}><Icon name="close" /></button>
                </td>
              </tr>
            ))}
            {!rows.length && <tr><td colSpan={3} className="dim">No points yet. Paste a table or add a tenor below.</td></tr>}
          </tbody>
          <tfoot>
            <tr className="points__add">
              <td className="edit"><input value={tenor} placeholder="e.g. 20Y" aria-label="New tenor"
                aria-invalid={tenor !== '' && !newTenor} onChange={(e) => setTenor(e.target.value)}
                onKeyDown={(e) => e.key === 'Enter' && add()} /></td>
              <td className="edit"><input value={value} placeholder="rate" aria-label="New rate"
                aria-invalid={value !== '' && newValue === null} onChange={(e) => setValue(e.target.value)}
                onKeyDown={(e) => e.key === 'Enter' && add()} /></td>
              <td style={{ width: 34 }}>
                <button type="button" className="icon-btn" style={{ width: 24, height: 24 }} aria-label="Add tenor"
                  disabled={!canAdd} onClick={add}><Icon name="plus" /></button>
              </td>
            </tr>
          </tfoot>
        </table>
      </div>
      <div className="points__bar">
        <button type="button" className="btn btn--ghost btn--sm" onClick={() => setPasting((v) => !v)} aria-expanded={pasting}>Paste from Excel</button>
        <button type="button" className="btn btn--ghost btn--sm" onClick={() => copy(formatTenorTable(rows))} disabled={!rows.length}>Copy</button>
      </div>
      {pasting && (
        <PasteBox points={points} onApply={onChange} onClose={points.length ? () => setPasting(false) : null}
          templateTenors={templateTenors} valueLabel={valueLabel} />
      )}
    </div>
  );
}
