import React, { useEffect, useState } from 'react';
import { Mark } from './Rosette';
import { bytes } from '../lib/format';

export default function Boot({ engine }) {
  const [status, setStatus] = useState(engine.getStatus());
  useEffect(() => engine.onStatus(setStatus), [engine]);
  const known = status.total > 0;
  const share = known ? Math.min(1, status.received / status.total) : 0;
  return (
    <main className="boot">
      <div className="boot__card">
        <Mark className="boot__mark" style={{ width: 44, height: 44, color: 'var(--accent)' }} />
        <h1 className="boot__title">Convex Terminal</h1>
        <p className="boot__sub">Loading the fixed-income analytics engine (Rust compiled to WebAssembly).</p>
        <div className="boot__bar" role="progressbar" aria-valuemin={0} aria-valuemax={100}
          aria-valuenow={known ? Math.round(share * 100) : undefined} aria-label="Engine download">
          <div className={`boot__fill${known ? '' : ' boot__fill--indeterminate'}`}
            style={known ? { width: `${share * 100}%` } : undefined} />
        </div>
        <div className="boot__meta">
          <span>{status.received ? bytes(status.received) : 'Connecting'}</span>
          <span>{known ? `of ${bytes(status.total)}` : ''}</span>
        </div>
      </div>
    </main>
  );
}
