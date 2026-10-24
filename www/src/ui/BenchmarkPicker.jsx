import React, { useState } from 'react';
import { useWorkspace } from '../state/workspace';
import { Seg, Field } from './primitives';
import { formatLong } from '../lib/dates';

/**
 * The G-spread benchmark set: the on-the-run Treasuries or bullet bonds you
 * pick in the security's currency (Treasuries, examples, bonds you imported),
 * and the engine's selection rule: interpolate between the two bracketing
 * maturities, or use the one benchmark maturing the same day (the library
 * takes exactly one security then, so only same-day bonds are offered and
 * picking one replaces the last). Never the security itself, callable or
 * sinking-fund bonds; the on-the-run Treasuries are a USD set.
 */
export default function BenchmarkPicker({ sec }) {
  const { universe, benchmarks, benchmarkChoice: choice, setBenchmarkChoice } = useWorkspace();
  const [q, setQ] = useState('');
  const exact = choice.selection === 'ExactMaturity';
  const usd = sec.currency === 'USD';
  const mode = usd ? choice.mode : 'custom';
  const candidates = universe.filter((s) => s.id !== sec.id && s.kind === 'fixed' && s.currency === sec.currency
    && !(s.calls || []).length && !(s.sinking || []).length && (!exact || s.maturity === sec.maturity));
  const ids = new Set(choice.ids);
  const toggle = (id) => setBenchmarkChoice({
    ids: ids.has(id) ? choice.ids.filter((x) => x !== id) : exact ? [id] : [...choice.ids, id],
  });
  const addOtr = () => setBenchmarkChoice({
    ids: [...new Set([...choice.ids, ...benchmarks.map((b) => b.sec.id)
      .filter((id) => candidates.some((s) => s.id === id))])],
  });
  const t = q.trim().toUpperCase();
  const shown = candidates
    .filter((s) => !t || [s.cusip, s.isin, s.ticker, s.name].filter(Boolean).some((v) => v.toUpperCase().includes(t)))
    .sort((a, b) => (ids.has(b.id) - ids.has(a.id)) || (a.maturity < b.maturity ? -1 : 1));
  const sameDay = candidates.filter((s) => s.maturity === sec.maturity);
  return (
    <div className="stack" style={{ gap: 10 }}>
      <div className="row" style={{ gap: 16, alignItems: 'flex-end', flexWrap: 'wrap' }}>
        <Field label="Benchmarks" as="div">
          <Seg value={mode} onChange={(mode) => setBenchmarkChoice({ mode })} label="Benchmark set"
            options={usd ? [['otr', 'On-the-run Treasuries'], ['custom', 'Your selection']] : [['custom', 'Your selection']]} />
        </Field>
        <Field label="Benchmark yield" as="div">
          <Seg value={choice.selection} onChange={(selection) => setBenchmarkChoice({ selection })} label="Selection rule"
            options={[['LinearMaturity', 'Interpolated by maturity'], ['ExactMaturity', 'Same maturity']]} />
        </Field>
        <div className="dim" style={{ fontSize: 12, maxWidth: 420 }}>
          {exact
            ? `Needs one benchmark maturing ${formatLong(sec.maturity)}.`
            : 'Interpolates the two benchmarks bracketing the maturity; the maturity must lie between the shortest and longest.'}
        </div>
      </div>
      {exact && sameDay.length > 0 && (
        <div className="row" style={{ gap: 8, alignItems: 'center' }}>
          <span className="dim" style={{ fontSize: 12 }}>Same-maturity bonds:</span>
          {sameDay.map((s) => (
            <button key={s.id} type="button" className="btn btn--sm"
              onClick={() => setBenchmarkChoice({ mode: 'custom', ids: exact ? [s.id] : ids.has(s.id) ? choice.ids : [...choice.ids, s.id] })}>
              {ids.has(s.id) && mode === 'custom' ? '✓ ' : ''}{s.ticker || s.name}
            </button>
          ))}
        </div>
      )}
      {mode === 'custom' && (
        <div className="stack" style={{ gap: 6 }}>
          <div className="row" style={{ gap: 8, alignItems: 'center' }}>
            <input className="input" style={{ maxWidth: 280 }} placeholder={`Filter ${sec.currency} bonds by CUSIP or name`} value={q}
              onChange={(e) => setQ(e.target.value)} aria-label="Filter benchmark candidates" />
            <span className="dim" style={{ fontSize: 12 }}>{choice.ids.filter((id) => candidates.some((s) => s.id === id)).length} chosen</span>
            <div className="grow" />
            {usd && (
              <button type="button" className="btn btn--ghost btn--sm" onClick={addOtr}>
                Add the on-the-runs
              </button>
            )}
            <button type="button" className="btn btn--ghost btn--sm" onClick={() => setBenchmarkChoice({ ids: [] })} disabled={!choice.ids.length}>Clear</button>
          </div>
          <div className="table-wrap" style={{ maxHeight: 220 }}>
            <table className="grid">
              <tbody>
                {shown.map((s) => (
                  <tr key={s.id} className={ids.has(s.id) ? 'is-active' : ''}>
                    <td style={{ width: 30 }}>
                      <input type="checkbox" checked={ids.has(s.id)} onChange={() => toggle(s.id)} aria-label={`Use ${s.ticker || s.name} as a benchmark`} />
                    </td>
                    <td>{s.ticker || s.name}</td>
                    <td className="dim">{s.cusip || s.isin || (s.imported ? 'Imported' : 'Example')}</td>
                    <td className="num">{s.coupon}</td>
                    <td>{formatLong(s.maturity)}</td>
                  </tr>
                ))}
                {!shown.length && <tr><td colSpan={5} className="dim">No {sec.currency} bullet bonds{exact ? ` maturing ${formatLong(sec.maturity)}` : ''} match. Import some on SEC → Import from Excel.</td></tr>}
              </tbody>
            </table>
          </div>
        </div>
      )}
    </div>
  );
}
