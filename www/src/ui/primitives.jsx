import React from 'react';

export function Panel({ title, note, actions, children, flush, foot, className = '', ...rest }) {
  return (
    <section className={`panel ${className}`} {...rest}>
      {(title || actions) && (
        <header className="panel__head">
          {title && <h2 className="panel__title">{title}</h2>}
          {note && <span className="panel__note">{note}</span>}
          {actions && <div className="panel__actions">{actions}</div>}
        </header>
      )}
      <div className={flush ? 'panel__body panel__body--flush' : 'panel__body'}>{children}</div>
      {foot && <footer className="panel__foot">{foot}</footer>}
    </section>
  );
}

export function Field({ label, hint, wide, children }) {
  return (
    <label className={`field${wide ? ' field--wide' : ''}`}>
      <span>{label}</span>
      {children}
      {hint && <em className="field__hint">{hint}</em>}
    </label>
  );
}

export function Select({ value, onChange, options, ...rest }) {
  return (
    <select value={value} onChange={(e) => onChange(e.target.value)} {...rest}>
      {options.map((o) => {
        const [v, l] = Array.isArray(o) ? o : [o, o];
        return (
          <option key={v} value={v}>
            {l}
          </option>
        );
      })}
    </select>
  );
}

export function Seg({ value, onChange, options, label }) {
  return (
    <div className="seg" role="group" aria-label={label}>
      {options.map((o) => {
        const [v, l] = Array.isArray(o) ? o : [o, o];
        return (
          <button key={v} type="button" aria-pressed={value === v} onClick={() => onChange(v)}>
            {l}
          </button>
        );
      })}
    </div>
  );
}

export function Tabs({ value, onChange, options, label }) {
  return (
    <div className="tabs" role="tablist" aria-label={label}>
      {options.map((o) => {
        const [v, l] = Array.isArray(o) ? o : [o, o];
        return (
          <button key={v} type="button" role="tab" aria-selected={value === v} onClick={() => onChange(v)}>
            {l}
          </button>
        );
      })}
    </div>
  );
}

const PROV_TEXT = { live: 'Live', snapshot: 'Snapshot', manual: 'Edited', derived: 'Derived', error: 'Failed' };

/** Provenance: a shape-coded mark plus a word, never colour alone. */
export function Prov({ status, children, title }) {
  return (
    <span className={`prov prov--${status}`} title={title}>
      <i className="prov__mark" aria-hidden="true" />
      {children || PROV_TEXT[status] || status}
    </span>
  );
}

export function Figure({ label, value, unit, sub, title }) {
  return (
    <div className="figure" title={title}>
      <div className="figure__label">{label}</div>
      <div className="figure__value">
        {value}
        {unit && <small>{unit}</small>}
      </div>
      {sub && <div className="figure__sub">{sub}</div>}
    </div>
  );
}

/** Key/value rows: [[label, value, source?], ...]. */
export function KV({ rows }) {
  return (
    <dl className="kv">
      {rows.filter(Boolean).map(([label, value, src]) => (
        <div key={label}>
          <dt>
            {label}
            {src && <span className="kv__src">{src}</span>}
          </dt>
          <dd>{value}</dd>
        </div>
      ))}
    </dl>
  );
}

/** A native library error, shown verbatim with its code. */
export function EngineError({ error, context }) {
  if (!error) return null;
  return (
    <div className="note note--error" role="alert">
      <b>{error.code}</b> {context ? `${context}: ` : ''}
      {error.message}
    </div>
  );
}

export function Empty({ children }) {
  return <div className="empty">{children}</div>;
}

export function Spinner({ label = 'Calculating' }) {
  return <span className="spinner" role="status" aria-label={label} />;
}

export function Icon({ name }) {
  const paths = {
    sun: 'M12 4V2M12 22v-2M4 12H2M22 12h-2M5.6 5.6 4.2 4.2M19.8 19.8l-1.4-1.4M5.6 18.4l-1.4 1.4M19.8 4.2l-1.4 1.4M12 17a5 5 0 1 0 0-10 5 5 0 0 0 0 10Z',
    moon: 'M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5Z',
    code: 'm8 7-5 5 5 5M16 7l5 5-5 5',
    refresh: 'M20 11a8 8 0 0 0-14.6-4.5L4 8M4 4v4h4M4 13a8 8 0 0 0 14.6 4.5L20 16M20 20v-4h-4',
    copy: 'M9 9h10v10H9zM5 15V5h10',
    close: 'M6 6l12 12M18 6 6 18',
    download: 'M12 4v11m0 0-4-4m4 4 4-4M5 19h14',
    plus: 'M12 5v14M5 12h14',
    trash: 'M5 7h14M10 7V4h4v3M7 7l1 13h8l1-13',
  };
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round"
      strokeLinejoin="round" aria-hidden="true">
      <path d={paths[name]} />
    </svg>
  );
}
