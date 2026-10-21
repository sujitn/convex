import React, { useState } from 'react';
import { useWorkspace, useComputation } from '../state/workspace';
import { useStore } from '../state/store';
import * as R from '../lib/requests';
import { SCENARIOS, repriceUnderShifts, keyRateVariants } from '../analytics/scenarios';
import { Panel, Field, EngineError, Empty, Spinner, KV } from '../ui/primitives';
import SecurityBand from '../ui/SecurityBand';
import ScreenHead from '../ui/ScreenHead';
import { BarChart } from '../ui/charts';
import { fmt, signed, money } from '../lib/format';

function useDebounced(value, delay = 400) {
  const [v, setV] = React.useState(value);
  React.useEffect(() => {
    const t = setTimeout(() => setV(value), delay);
    return () => clearTimeout(t);
  }, [value, delay]);
  return v;
}

export default function Scenarios({ fn }) {
  // Scenarios shift the government curve's par quotes and recalibrate it.
  const { engine, selected: sec, settlement, govCurve: curve, datasets } = useWorkspace();
  const { state } = useStore();
  const face = Number(state.quote.face || '1000000');
  const [zText, setZText] = useState(null);
  const def = curve && curve.def;
  const parCurve = def && def.method === 'par';
  const z = zText ?? (sec && sec.source === 'auction' ? '0' : '125');
  const dz = useDebounced(z);
  const zOk = z.trim() !== '' && Number.isFinite(Number(z));
  const zDecimal = zOk ? Number(dz) / 10000 : NaN;
  const bullet = sec ? { ...sec, calls: [] } : null;
  const ready = sec && sec.kind === 'fixed' && curve && curve.recipe && !curve.building && parCurve && Number.isFinite(zDecimal);
  const label = sec ? sec.ticker || sec.name : '';

  const base0 = useComputation(async () => {
    if (!ready) return null;
    const role = curve.spreadRole;
    return engine.call(R.bondPrice(bullet, settlement, R.spreadMethod(curve.key, role, zDecimal), [curve.recipe]),
      { label: `${label} base price` });
  }, [ready, sec, settlement, curve && curve.key && curve.key.id, curve && curve.key && curve.key.version, zDecimal]);

  const scen0 = useComputation(
    () => (ready ? repriceUnderShifts(engine, { sec: bullet, settlement, curve, dataset: datasets[def.dataset],
      zDecimal, variants: SCENARIOS, label }) : null),
    [ready, sec, settlement, curve && curve.key && curve.key.id, curve && curve.key && curve.key.version, zDecimal],
  );
  const krd0 = useComputation(
    () => (ready ? repriceUnderShifts(engine, { sec: bullet, settlement, curve, dataset: datasets[def.dataset],
      zDecimal, variants: keyRateVariants(datasets[def.dataset]), label: `${label} key rates` }) : null),
    [ready, sec, settlement, curve && curve.key && curve.key.id, curve && curve.key && curve.key.version, zDecimal],
  );

  // Results for another security, curve or spread are not shown while the next run is under way.
  const base = { ...base0, data: base0.current ? base0.data : null };
  const scen = { ...scen0, data: scen0.current ? scen0.data : null };
  const krd = { ...krd0, data: krd0.current ? krd0.data : null };
  const pending = !zOk ? (
    <div className="note note--error" role="alert">Enter the Z-spread as a number of basis points, for example 125.</div>
  ) : !curve || curve.building ? (
    <Empty><Spinner /> Building {def ? def.label : 'the government curve'}…</Empty>
  ) : curve.error ? (
    <EngineError error={curve.error} context={def ? def.label : 'Government curve'} />
  ) : scen0.error || krd0.error ? (
    <EngineError error={scen0.error || krd0.error} context="Scenarios" />
  ) : <Empty><Spinner /></Empty>;

  const p0 = base.data && base.data.ok ? base.data.data.dirty_per100 : NaN;
  const scale = sec ? face / Number(sec.face || 100) : 1;
  const krRows = krd.data ? krd.data.map((r) => ({ ...r, krd: -(r.dirty - p0) / (p0 * 1e-4) })) : [];
  const krSum = krRows.reduce((a, r) => a + (Number.isFinite(r.krd) ? r.krd : 0), 0);
  const maxAbs = scen.data ? Math.max(...scen.data.map((r) => Math.abs(r.dirty - p0) || 0)) : 1;

  return (
    <div className="screen">
      <ScreenHead fn={fn} lede="Each scenario shifts the par quotes, recalibrates the curve in the engine, and reprices the bond at an unchanged Z-spread." />
      <SecurityBand sec={sec} settlement={settlement} />
      {!sec || sec.kind !== 'fixed' ? <Empty>Scenarios apply to coupon bonds.</Empty> : !parCurve ? (
        <div className="note">Scenarios recalibrate a par curve; {def ? def.label : 'this currency'} is built from published spot rates. Choose a USD or CAD security.</div>
      ) : (
        <div className="stack">
          <Panel>
            <div className="row" style={{ alignItems: 'flex-end' }}>
              <Field label={`Z-spread held constant, bp over ${def.label}`}>
                <input className="input input--num" style={{ width: 160 }} value={z} onChange={(e) => setZText(e.target.value)} />
              </Field>
              <div className="grow" />
              {(scen.busy || krd.busy) && <span className="dim"><Spinner /> Recalibrating {SCENARIOS.length + krRows.length || ''} curves</span>}
            </div>
            {sec.calls && sec.calls.length > 0 && <div className="note note--warn" style={{ marginTop: 10 }}>Calls are removed for these straight scenarios; see OAS for option-adjusted risk.</div>}
            {base.data && !base.data.ok && <EngineError error={base.data.error} context="Base price" />}
          </Panel>

          <div className="cols cols--wide">
            <Panel title="Curve scenarios" note={`P&L for ${money(face, 0)} face`} flush>
              {scen.data ? (
                <table className="grid">
                  <thead><tr><th>Scenario</th><th className="num">Clean</th><th className="num">Δ price</th><th className="num">P&amp;L</th><th style={{ width: '30%' }} /></tr></thead>
                  <tbody>
                    {scen.data.map((r) => {
                      const d = r.dirty - p0;
                      const w = Number.isFinite(d) ? (Math.abs(d) / maxAbs) * 50 : 0;
                      return (
                        <tr key={r.id}>
                          <td>{r.label}</td>
                          <td className="num">{fmt(r.clean, 4)}</td>
                          <td className={`num ${d > 0 ? 'pos' : d < 0 ? 'neg' : ''}`}>{signed(d, 4)}</td>
                          <td className={`num ${d > 0 ? 'pos' : d < 0 ? 'neg' : ''}`}>{signed(d * scale, 0)}</td>
                          <td>
                            <div style={{ position: 'relative', height: 10 }} aria-hidden="true">
                              <div style={{ position: 'absolute', left: '50%', top: -4, bottom: -4, width: 1, background: 'var(--rule-strong)' }} />
                              <div style={{ position: 'absolute', height: 10, borderRadius: 2, top: 0,
                                left: d >= 0 ? '50%' : `${50 - w}%`, width: `${w}%`, background: d >= 0 ? 'var(--pos)' : 'var(--neg)' }} />
                            </div>
                            {r.error && <span className="neg">{r.error.message}</span>}
                          </td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              ) : pending}
            </Panel>
            <Panel title="Key-rate durations" note="+1 bp on one par quote at a time">
              {krRows.length ? (
                <>
                  <BarChart height={230}
                    keys={[{ key: 'krd', label: 'Key-rate duration', color: '--s2' }]}
                    bars={krRows.map((r, i) => ({ x: i, label: r.tenor, parts: [{ key: 'krd', value: r.krd }] }))}
                    xTicks={krRows.map((_, i) => i)} xFormat={(v) => (krRows[v] ? krRows[v].tenor : '')}
                    yFormat={(v) => fmt(v, 2)} tipX={(v, b) => `${b.label} par quote`} />
                  <KV rows={[
                    ['Sum of key-rate durations', fmt(krSum, 4)],
                    ['Largest bucket', krRows.reduce((a, r) => (r.krd > a.krd ? r : a), krRows[0]).tenor],
                    ['DV01 by bucket sum', fmt((krSum * p0 * 1e-4) * scale, 2), `for ${money(face, 0)} face`],
                  ]} />
                </>
              ) : pending}
            </Panel>
          </div>
          {krRows.length > 0 && (
            <Panel title="Key-rate table" flush>
              <div className="table-wrap">
                <table className="grid">
                  <thead><tr><th>Par tenor</th>{krRows.map((r) => <th key={r.id} className="num">{r.tenor}</th>)}</tr></thead>
                  <tbody>
                    <tr><td>Duration</td>{krRows.map((r) => <td key={r.id} className="num">{fmt(r.krd, 3)}</td>)}</tr>
                    <tr><td>DV01 for face</td>{krRows.map((r) => <td key={r.id} className="num">{fmt(-(r.dirty - p0) * scale, 0)}</td>)}</tr>
                  </tbody>
                </table>
              </div>
            </Panel>
          )}
        </div>
      )}
    </div>
  );
}
