import React, { useEffect, useMemo, useState } from 'react';
import { Icon, Tabs } from './primitives';
import { ms } from '../lib/format';

const HIGHLIGHT_LIMIT = 150000;

function highlight(text) {
  if (text.length > HIGHLIGHT_LIMIT) return null;
  const out = [];
  const re = /("(?:\\.|[^"\\])*")(\s*:)?|(-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)|\b(true|false|null)\b/g;
  let last = 0;
  let m;
  let i = 0;
  while ((m = re.exec(text))) {
    if (m.index > last) out.push(text.slice(last, m.index));
    if (m[1]) out.push(<span key={i++} className={m[2] ? 'k' : 's'}>{m[1]}</span>, m[2] || '');
    else if (m[3]) out.push(<span key={i++} className="n">{m[3]}</span>);
    else out.push(<span key={i++} className="b">{m[4]}</span>);
    last = re.lastIndex;
  }
  out.push(text.slice(last));
  return out;
}

/** Pretty JSON where arrays of scalars (holiday tables, grids) stay compact. */
export function stringify(value, indent = '') {
  if (value === null || typeof value !== 'object') return JSON.stringify(value);
  const next = `${indent}  `;
  if (Array.isArray(value)) {
    if (!value.length) return '[]';
    if (value.every((v) => v === null || typeof v !== 'object')) {
      const items = value.map((v) => JSON.stringify(v));
      const rows = [];
      for (let i = 0; i < items.length; i += 8) rows.push(items.slice(i, i + 8).join(', '));
      return rows.length === 1 ? `[${rows[0]}]` : `[\n${rows.map((r) => next + r).join(',\n')}\n${indent}]`;
    }
    return `[\n${value.map((v) => next + stringify(v, next)).join(',\n')}\n${indent}]`;
  }
  const keys = Object.keys(value);
  if (!keys.length) return '{}';
  return `{\n${keys.map((k) => `${next}${JSON.stringify(k)}: ${stringify(value[k], next)}`).join(',\n')}\n${indent}}`;
}

export function JsonView({ value, text }) {
  const pretty = useMemo(() => {
    if (text !== undefined) {
      try {
        return stringify(JSON.parse(text));
      } catch {
        return text;
      }
    }
    return stringify(value);
  }, [value, text]);
  const parts = useMemo(() => highlight(pretty || ''), [pretty]);
  return <pre className="json">{parts || pretty}</pre>;
}

function copy(text) {
  if (navigator.clipboard) navigator.clipboard.writeText(text).catch(() => {});
}

function download(name, text) {
  const blob = new Blob([text], { type: 'application/json' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = name;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

export default function Inspector({ engine, onClose, onOpenInConsole }) {
  const [log, setLog] = useState(engine.getLog());
  const [selected, setSelected] = useState(null);
  const [tab, setTab] = useState('request');
  useEffect(() => engine.onLog(setLog), [engine]);
  const entry = log.find((e) => e.id === selected) || log[0];
  const requestText = entry ? JSON.stringify(entry.request, null, 2) : '';
  const responseText = entry && entry.response ? entry.response : '';
  return (
    <aside className="inspector" aria-label="Library requests">
      <div className="inspector__head">
        <h2>Library calls</h2>
        <span className="muted">{log.length}</span>
        <div className="grow" />
        <button type="button" className="btn btn--ghost btn--sm" onClick={() => engine.clearLog()}>Clear</button>
        <button type="button" className="icon-btn" onClick={onClose} aria-label="Close inspector"><Icon name="close" /></button>
      </div>
      <div className="inspector__list" role="listbox" aria-label="Recent calls">
        {log.length === 0 && <div className="empty">Every engine call appears here with its exact request and response.</div>}
        {log.map((e) => (
          <div key={e.id} className="log-row" role="option" aria-selected={entry && e.id === entry.id}
            tabIndex={0} onClick={() => setSelected(e.id)} onKeyDown={(k) => k.key === 'Enter' && setSelected(e.id)}>
            <i className={`log-row__dot${e.ok ? '' : ' log-row__dot--err'}`} aria-label={e.ok ? 'ok' : 'error'} />
            <div style={{ minWidth: 0 }}>
              <div className="log-row__op">{e.op}{e.count > 1 ? ` ×${e.count}` : ''}</div>
              <div className="log-row__label">{e.ok ? e.label : `${e.error && e.error.code}: ${e.error && e.error.message}`}</div>
            </div>
            <span className="log-row__ms">{ms(e.ms)}</span>
          </div>
        ))}
      </div>
      <div className="inspector__detail">
        {entry ? (
          <>
            <div className="row" style={{ borderBottom: '1px solid var(--rule)', paddingRight: 8 }}>
              <Tabs value={tab} onChange={setTab} label="Payload"
                options={[['request', 'Request'], ['response', entry.count > 1 ? 'Response (first)' : 'Response']]} />
              <div className="grow" />
              <button type="button" className="icon-btn" title="Copy" aria-label="Copy JSON"
                onClick={() => copy(tab === 'request' ? requestText : responseText)}><Icon name="copy" /></button>
              <button type="button" className="icon-btn" title="Download" aria-label="Download JSON"
                onClick={() => download(`${entry.op}-${tab}.json`, tab === 'request' ? requestText : responseText)}>
                <Icon name="download" />
              </button>
              {onOpenInConsole && (
                <button type="button" className="btn btn--sm" onClick={() => onOpenInConsole(entry.request)}>Edit in API</button>
              )}
            </div>
            <JsonView key={`${entry.id}-${tab}`} {...(tab === 'request' ? { value: entry.request } : { text: responseText })} />
          </>
        ) : (
          <div />
        )}
      </div>
    </aside>
  );
}
