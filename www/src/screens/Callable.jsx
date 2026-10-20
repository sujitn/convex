import React, { useEffect, useState } from 'react';
import { useWorkspace, useComputation } from '../state/workspace';
import { useStore } from '../state/store';
import { analyseCallable } from '../analytics/callable';
import { Panel, Field, Figure, KV, EngineError, Empty, Spinner, Prov } from '../ui/primitives';
import SecurityBand from '../ui/SecurityBand';
import ScreenHead from '../ui/ScreenHead';
import { LineChart } from '../ui/charts';
import { WorkoutTable } from './Yas';
import { fmt, pct, signedBp } from '../lib/format';
import { formatLong } from '../lib/dates';
import { parsePrice } from '../lib/decimal';

function useDebounced(value, delay = 500) {
  const [v, setV] = useState(value);
  useEffect(() => {
    const t = setTimeout(() => setV(value), delay);
    return () => clearTimeout(t);
  }, [value, delay]);
  return v;
}

function num(text) {
  try {
    return parsePrice(text);
  } catch {
    return NaN;
  }
}

export default function Callable({ fn }) {
  const { engine, selected: sec, universe, settlement, tradeDate, curve } = useWorkspace();
  const { dispatch } = useStore();
  const [price, setPrice] = useState('97.00');
  const [mr, setMr] = useState('3.0');
  const [vol, setVol] = useState('1.00');
  const inputs = useDebounced(`${price}|${mr}|${vol}`);
  const callable = sec && (sec.calls || []).length > 0;
  const clean = num(price);
  const meanReversion = Number(mr) / 100;
  const volatility = Number(vol) / 100;
  const valid = Number.isFinite(clean) && clean > 0 && meanReversion >= 0 && volatility > 0;
  const ready = callable && curve && curve.recipe && !curve.building && valid;

  const { data, busy, error } = useComputation(
    () => (ready ? analyseCallable(engine, { sec, tradeDate, settlement, curve, cleanPrice: clean, meanReversion, volatility }) : null),
    [ready, sec, settlement, tradeDate, curve && curve.key && curve.key.version, inputs],
  );

  if (!callable) {
    const options = universe.filter((s) => (s.calls || []).length > 0);
    return (
      <div className="screen">
        <ScreenHead fn={fn} lede="Option-adjusted spread on a Hull-White one-factor tree, with option value, effective risk and every workout path." />
        <SecurityBand sec={sec} settlement={settlement} />
        <div className="note">
          {sec ? `${sec.ticker || sec.name} has no call provisions.` : 'No security selected.'} OAS applies to bonds with an
          embedded call. Choose one below, or add calls to an example on SEC.
        </div>
        <div className="row" style={{ marginTop: 12 }}>
          {options.map((s) => (
            <button key={s.id} type="button" className="btn" onClick={() => dispatch({ type: 'select', id: s.id })}>{s.ticker || s.name}</button>
          ))}
        </div>
      </div>
    );
  }

  const r = data && !data.fatal ? data : null;
  return (
    <div className="screen">
      <ScreenHead fn={fn} lede="Option-adjusted spread on a Hull-White one-factor tree, with option value, effective risk and every workout path." />
      <SecurityBand sec={sec} settlement={settlement} />
      <Panel style={{ marginBottom: 16 }}>
        <div className="row" style={{ alignItems: 'flex-end', gap: 16 }}>
          <Field label="Clean price"><input className="input input--quote" style={{ width: 170 }} value={price} onChange={(e) => setPrice(e.target.value)} aria-invalid={!Number.isFinite(clean)} /></Field>
          <Field label="Short-rate volatility, % (normal)"><input className="input input--num" style={{ width: 120 }} value={vol} onChange={(e) => setVol(e.target.value)} /></Field>
          <Field label="Mean reversion, %"><input className="input input--num" style={{ width: 120 }} value={mr} onChange={(e) => setMr(e.target.value)} /></Field>
          <div className="grow" />
          <div className="dim" style={{ fontSize: 12, maxWidth: 360 }}>
            {busy ? <><Spinner /> Solving on the tree…</> : (
              <>Model parameters are your assumptions, sent to the engine as an explicitly assumed model, not a calibration.</>
            )}
          </div>
        </div>
      </Panel>
      {(data && data.fatal) && <EngineError error={data.fatal} context="OAS" />}
      {error && <EngineError error={error} />}
      {r && (
        <div className="stack">
          <div className="panel">
            <div className="figures">
              <Figure label="OAS" value={signedBp(r.oas, 1)} unit="bp" sub={`over ${curve.def.label} zero curve`} />
              <Figure label="Option value" value={r.optionValue !== undefined ? fmt(r.optionValue, 4) : '—'} sub="straight − callable, per 100" />
              <Figure label="Effective duration" value={r.risk ? fmt(r.risk.duration, 3) : '—'}
                sub={r.risk ? `curve ± ${r.risk.bump * 10000} bp at fixed OAS` : 'did not converge'} />
              <Figure label="Effective convexity" value={r.risk ? fmt(r.risk.convexity, 2) : '—'} />
              <Figure label="Yield to worst" value={r.workout ? pct(r.workout.worst.decimalYield, 4) : '—'} unit="%"
                sub={r.workout ? (r.workout.worst.kind === 'maturity' ? 'to maturity' : `to call ${formatLong(r.workout.worst.date)}`) : ''} />
            </div>
          </div>
          <div className="cols cols--3">
            <Panel title="Valuation">
              <KV rows={[
                ['Clean price', fmt(clean, 4)],
                ['Accrued', fmt(r.accrued, 6)],
                ['Dirty price', fmt(r.dirty, 6)],
                ['Model dirty at OAS', fmt(r.oasPrice.dirty_price, 6), `${r.iterations} solver iterations`],
                ['Straight bond at OAS', r.straightDirty !== undefined ? fmt(r.straightDirty, 6) : '—', 'calls removed, same constant spread'],
                ['Option value', r.optionValue !== undefined ? fmt(r.optionValue, 6) : '—'],
              ]} />
              <EngineError error={r.errors.straight} />
            </Panel>
            <Panel title="Effective risk">
              {r.risk ? (
                <KV rows={[
                  ['Effective duration', fmt(r.risk.duration, 4)],
                  ['Effective convexity', fmt(r.risk.convexity, 3)],
                  ['PV01 per 100', r.risk.pv01 !== undefined ? fmt(r.risk.pv01, 6) : '—'],
                  ['Model', 'Hull-White 1F', `a = ${mr}%, σ = ${vol}%`],
                  ['Discount curve', r.discountKey.id],
                ]} />
              ) : <EngineError error={r.errors.risk} context="Effective risk" />}
            </Panel>
            <Panel title="OAS by volatility" note="same price, other σ">
              <LineChart height={170} legend={false}
                series={[{ id: 'v', label: 'OAS', color: '--s5', points: r.volLadder.filter((v) => Number.isFinite(v.oas)).map((v) => ({ x: v.vol * 100, y: v.oas * 10000 })),
                  markers: [{ x: volatility * 100, y: r.oas * 10000 }] }]}
                xFormat={(v) => `${v.toFixed(2)}%`} yFormat={(v) => v.toFixed(0)} tipX={(v) => `σ ${v.toFixed(2)}%`} tipY={(v) => `${v.toFixed(1)} bp`} />
            </Panel>
          </div>
          <Panel title="Workouts" note="dated-cash yield to each legal redemption path" flush>
            {r.workout ? <WorkoutTable rows={r.workout} /> : <div className="panel__body"><EngineError error={r.errors.workout} /></div>}
          </Panel>
          <div className="cols cols--2">
            <Panel title="Exercise schedule" flush>
              <table className="grid">
                <thead><tr><th>Right</th><th>Exercise</th><th>Notice</th><th>Settlement</th><th className="num">Tree nodes</th></tr></thead>
                <tbody>
                  {r.exercises.map((x) => (
                    <tr key={x.dates.exercise}><td>{x.right}</td><td>{formatLong(x.dates.exercise)}</td><td>{formatLong(x.dates.notice)}</td>
                      <td>{formatLong(x.dates.settlement)}</td><td className="num dim">{x.exercise_nodes}</td></tr>
                  ))}
                </tbody>
              </table>
            </Panel>
            <Panel title="Tree convergence" note="successive refinements at the solved OAS" flush>
              <table className="grid">
                <thead><tr><th className="num">Steps</th><th className="num">Dirty price</th><th className="num">Change</th><th className="num">Nodes</th></tr></thead>
                <tbody>
                  {(r.grids || []).map((g) => (
                    <tr key={g.desired_steps}><td className="num">{g.desired_steps}</td><td className="num">{fmt(g.dirty_price, 6)}</td>
                      <td className="num dim">{g.successive_difference !== null ? g.successive_difference.toExponential(2) : ''}</td>
                      <td className="num dim">{g.tree.nodes.toLocaleString()}</td></tr>
                  ))}
                </tbody>
              </table>
            </Panel>
          </div>
          <div className="source-line"><Prov status="derived">Assumed model</Prov> {r.model.reason}</div>
        </div>
      )}
      {!r && !(data && data.fatal) && <Empty>{busy ? <Spinner /> : 'Enter a price to solve for OAS.'}</Empty>}
    </div>
  );
}
