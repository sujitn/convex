import React, { useState } from 'react';
import { Icon } from './primitives';
import { isDecimal } from '../lib/decimal';
import { tenorYears } from '../lib/dates';
import { normaliseTenor, parseTenorTable, formatTenorTable } from '../lib/paste';

const byTenor = (a, b) => tenorYears(a.tenor) - tenorYears(b.tenor);

/** Paste a tenor/rate table to replace the points or merge into them. */
export function PasteBox({ points, onApply, onClose }) {
  const [text, setText] = useState('');
  const parsed = text.trim() ? parseTenorTable(text) : null;
  const apply = (mode) => {
    if (!parsed || !parsed.points.length) return;
    if (mode === 'replace') onApply(parsed.points);
    else {
      const map = new Map(points.map((p) => [p.tenor, p]));
      for (const p of parsed.points) map.set(p.tenor, { ...(map.get(p.tenor) || {}), ...p });
      onApply([...map.values()].sort(byTenor));
    }
    setText('');
    if (onClose) onClose();
  };
  return (
    <div className="paste">
      <textarea value={text} onChange={(e) => setText(e.target.value)} spellCheck={false} aria-label="Pasted tenors and rates"
        placeholder={'One point per line, tenor then rate in %. From Excel, a terminal or a web table:\n1M\t3.90\n1Y\t3.60\n10 yr, 3.62'} />
      <div className="row" style={{ gap: 8, alignItems: 'center' }}>
        <span className="dim" style={{ fontSize: 12 }}>
          {parsed ? `${parsed.points.length} point${parsed.points.length === 1 ? '' : 's'} read` : 'Paste or type a table'}
          {parsed && parsed.errors.length > 0 && <span className="neg">, {parsed.errors.length} skipped</span>}
        </span>
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

/** Editable tenor/rate grid: edit a rate, remove a tenor, add a tenor, paste a table, copy it out. */
export default function PointsEditor({ points, onChange, valueLabel = 'Rate %', maxHeight = 360 }) {
  const [draft, setDraft] = useState({});
  const [tenor, setTenor] = useState('');
  const [value, setValue] = useState('');
  const [pasting, setPasting] = useState(false);
  const rows = [...points].sort(byTenor);
  const commit = (t, text) => {
    setDraft((d) => ({ ...d, [t]: undefined }));
    if (!isDecimal(text)) return;
    onChange(points.map((p) => (p.tenor === t ? { ...p, value: text.trim() } : p)));
  };
  const newTenor = normaliseTenor(tenor);
  const canAdd = newTenor && isDecimal(value);
  const add = () => {
    if (!canAdd) return;
    const rest = points.filter((p) => p.tenor !== newTenor);
    onChange([...rest, { tenor: newTenor, value: value.trim() }].sort(byTenor));
    setTenor('');
    setValue('');
  };
  const copy = () => navigator.clipboard && navigator.clipboard.writeText(formatTenorTable(rows)).catch(() => {});
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
                    aria-invalid={draft[p.tenor] !== undefined && !isDecimal(draft[p.tenor])}
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
            {!rows.length && <tr><td colSpan={3} className="dim">No points yet. Add a tenor below or paste a table.</td></tr>}
          </tbody>
          <tfoot>
            <tr className="points__add">
              <td className="edit"><input value={tenor} placeholder="e.g. 20Y" aria-label="New tenor"
                aria-invalid={tenor !== '' && !newTenor} onChange={(e) => setTenor(e.target.value)}
                onKeyDown={(e) => e.key === 'Enter' && add()} /></td>
              <td className="edit"><input value={value} placeholder="rate" aria-label="New rate"
                aria-invalid={value !== '' && !isDecimal(value)} onChange={(e) => setValue(e.target.value)}
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
        <button type="button" className="btn btn--ghost btn--sm" onClick={() => setPasting((v) => !v)} aria-expanded={pasting}>Paste table</button>
        <button type="button" className="btn btn--ghost btn--sm" onClick={copy} disabled={!rows.length}>Copy</button>
      </div>
      {pasting && <PasteBox points={points} onApply={onChange} onClose={() => setPasting(false)} />}
    </div>
  );
}
