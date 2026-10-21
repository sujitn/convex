import React, { useState } from 'react';
import { useWorkspace, useComputation } from '../state/workspace';
import { INTERPOLATIONS, sampleCurve } from '../analytics/curves';
import * as R from '../lib/requests';
import { Panel, Seg, Field, KV, EngineError, Empty, Prov, Spinner, Select } from '../ui/primitives';
import { LineChart } from '../ui/charts';
import ScreenHead from '../ui/ScreenHead';
import { fmt, ms } from '../lib/format';
import { formatLong, tenorYears } from '../lib/dates';

const COMPOUNDINGS = ['Continuous', 'Annual', 'SemiAnnual', 'Quarterly', 'Simple'];
const PALETTE = ['--s1', '--s2', '--s3', '--s4', '--s5', '--s6'];
const METHOD = {
  par: 'Piecewise par-coupon calibration',
  ois: 'Piecewise OIS calibration from par swaps',
  zero: 'Supplied zero nodes',
};
const INPUT = { par: 'Par input', ois: 'Swap rate input', zero: 'Spot input' };

// Built-in curves then yours, in a fixed order, so a curve keeps its colour; past
// six, the palette repeats with a dashed line rather than a generated hue.
function seriesStyle(defs, cid) {
  const i = Math.max(0, Object.keys(defs).indexOf(cid));
  return { color: PALETTE[i % PALETTE.length], dash: i >= PALETTE.length };
}

function QueryTool({ curve }) {
  const { engine } = useWorkspace();
  const [tenor, setTenor] = useState('7');
  const [end, setEnd] = useState('10');
  const [comp, setComp] = useState('SemiAnnual');
  const t = Number(tenor);
  const e = Number(end);
  const valid = t > 0 && e > t;
  const { data, busy, error } = useComputation(async () => {
    if (!valid || !curve.recipe) return null;
    const res = await engine.callMany([
      R.curveQuery([curve.recipe], curve.key, R.zeroQuery(t, comp)),
      R.curveQuery([curve.recipe], curve.key, R.dfQuery({ Tenor: t })),
      R.curveQuery([curve.recipe], curve.key, R.forwardQuery(t, e, comp)),
    ], { label: `${curve.key.id} point query` });
    return res;
  }, [curve.key && curve.key.id, curve.key && curve.key.version, t, e, comp, valid]);
  const v = (i) => (data && data[i].ok ? data[i].data.value : NaN);
  return (
    <Panel title="Point query" note="native curve_query">
      <div className="form form--2" style={{ marginBottom: 10 }}>
        <Field label="Tenor, years"><input className="input--num" value={tenor} onChange={(x) => setTenor(x.target.value)} /></Field>
        <Field label="Forward end, years"><input className="input--num" value={end} onChange={(x) => setEnd(x.target.value)} /></Field>
        <Field label="Rate compounding" wide as="div">
          <Seg value={comp} onChange={setComp} options={COMPOUNDINGS.map((c) => [c, c === 'SemiAnnual' ? 'Semi' : c])} label="Compounding" />
        </Field>
      </div>
      {busy && <Spinner />}
      {data && (
        <KV rows={[
          [`Zero rate at ${tenor}Y`, `${fmt(v(0) * 100, 5)}%`, `${comp}, Act/365F`],
          [`Discount factor at ${tenor}Y`, fmt(v(1), 8)],
          [`Forward ${tenor}Y → ${end}Y`, `${fmt(v(2) * 100, 5)}%`, comp],
        ]} />
      )}
      <EngineError error={error} context="Point query" />
      {data && data.find((d) => !d.ok) && <EngineError error={data.find((d) => !d.ok).error} />}
    </Panel>
  );
}

export default function Curves({ fn }) {
  const { engine, curves, curveOptions, setInterpolation, datasets, curveDefs } = useWorkspace();
  const [pick, setId] = useState('USD.UST');
  const [compare, setCompare] = useState(false);
  const id = curveDefs[pick] ? pick : 'USD.UST';
  const curve = curves[id];
  const def = curveDefs[id];

  const sample = useComputation(
    () => (curve && curve.recipe && !curve.building ? sampleCurve(engine, curve) : null),
    [id, curve && curve.key && curve.key.id, curve && curve.key && curve.key.version, curve && curve.interpolation, curve && curve.building],
  );
  const others = useComputation(async () => {
    if (!compare) return null;
    const out = {};
    for (const [cid, c] of Object.entries(curves)) {
      if (c && c.recipe && cid !== id) out[cid] = await sampleCurve(engine, c, { step: 0.5 });
    }
    return out;
  }, [compare, id, Object.values(curves).map((c) => c && c.key && `${c.key.id}@${c.key.version}`).join('|')]);

  const ds = def.custom ? def : datasets[def.dataset];
  const rows = sample.data ? sample.data.rows : [];
  const inputPoints = ds ? ds.points.map((p) => ({ x: tenorYears(p.tenor), y: Number(p.value) })) : [];
  const series = rows.length ? [
    { id: 'zero', label: 'Zero (cont.)', color: '--s1', points: rows.map((r) => ({ x: r.t, y: r.zero * 100 })) },
    { id: 'fwd', label: '3M forward', color: '--s2', points: rows.map((r) => ({ x: r.t, y: r.fwd * 100 })), dash: true },
    { id: 'in', label: INPUT[def.method], color: '--s3', points: inputPoints.sort((a, b) => a.x - b.x),
      markers: inputPoints },
  ] : [];
  const compareSeries = compare && others.data && rows.length ? [
    { id, label: def.label, ...seriesStyle(curveDefs, id), points: rows.filter((_, i) => i % 2 === 1).map((r) => ({ x: r.t, y: r.zero * 100 })) },
    ...Object.entries(others.data).filter(([cid]) => curveDefs[cid]).map(([cid, s]) => ({
      id: cid, label: curveDefs[cid].label, ...seriesStyle(curveDefs, cid), points: s.rows.map((r) => ({ x: r.t, y: r.zero * 100 })),
    })),
  ] : null;

  return (
    <div className="screen">
      <ScreenHead fn={fn} dates={false}
        lede="Curves are built by the engine from the market data screen: par yields are calibrated from par-coupon bonds, swap rates from par OIS swaps, published spot curves are used as nodes. Add your own under MKT, Your data.">
        {Object.keys(curveDefs).length <= 5 ? (
          <Seg value={id} onChange={setId} options={Object.values(curveDefs).map((d) => [d.id, d.label])} label="Curve" />
        ) : (
          <Select className="input" style={{ width: 260 }} value={id} onChange={setId} aria-label="Curve"
            options={Object.values(curveDefs).map((d) => [d.id, d.custom ? `${d.label} (yours)` : d.label])} />
        )}
      </ScreenHead>
      {!curve || curve.building ? <Empty><Spinner /> Building {def.label}…</Empty> : curve.error ? (
        <EngineError error={curve.error} context={def.label} />
      ) : (
        <div className="stack">
          <div className="cols cols--wide">
            <Panel title={`${def.label} zero and forward curve`} note="sampled natively every 3 months"
              actions={(
                <>
                  <label className="check"><input type="checkbox" checked={compare} onChange={(e) => setCompare(e.target.checked)} /> Compare</label>
                  {def.method === 'zero' ? (
                    <Select value={curveOptions[id] || curve.interpolation} onChange={(v) => setInterpolation(id, v)}
                      options={INTERPOLATIONS.map((i) => [i, i === 'CubicSpline' ? 'Cubic spline' : 'Linear zero'])} aria-label="Interpolation" />
                  ) : <span className="panel__note" title="The native calibration fits linear continuous zero rates">Linear zero (calibration)</span>}
                </>
              )}>
              {sample.busy && !rows.length ? <Empty><Spinner /></Empty> : (
                <LineChart height={320} series={compareSeries || series}
                  xFormat={(v) => `${Math.round(v)}Y`} yFormat={(v) => v.toFixed(2)}
                  tipX={(v) => `${fmt(v, 2)} years`} tipY={(v) => `${v.toFixed(4)}%`} />
              )}
              <EngineError error={sample.error || (sample.data && sample.data.error)} context="Sampling" />
            </Panel>
            <div className="stack">
              <Panel title="Construction">
                <KV rows={[
                  ['Curve', curve.key.id, `version ${curve.key.version}`],
                  ['Reference date', formatLong(curve.tradeDate)],
                  ['Method', METHOD[curve.method] || curve.method],
                  ['Role', def.role === 'discount' ? `Discount (${def.currency} collateral)` : `Government (${def.benchmarkId})`],
                  ['Representation', 'Zero rates, continuous, Act/365F time'],
                  ['Interpolation', curve.interpolation],
                  curve.repricing && ['Helpers', `${curve.repricing.length} ${curve.method === 'ois' ? 'spot-starting par OIS swaps' : 'par bonds'}`],
                  curve.publications && ['Publications supplied', curve.publications.toLocaleString(), `daily ${def.index} observation dates`],
                  curve.evaluations && ['Price evaluations', curve.evaluations],
                  ['Engine time', ms(curve.ms)],
                  ['Market data', <Prov key="p" status={curve.dataset.status}>{def.custom ? 'Yours, ' : ''}{formatLong(curve.dataset.asOf)}</Prov>],
                ]} />
                {def.note && <div className="note" style={{ marginTop: 10 }}>{def.note}</div>}
              </Panel>
              <QueryTool curve={curve} />
            </div>
          </div>

          {curve.repricing && (
            <Panel title="Calibration repricing" note="every helper repriced by the engine on the fitted curve" flush>
              <div className="table-wrap">
                <table className="grid">
                  <thead><tr><th>Helper</th><th>Pillar</th><th className="num">{curve.method === 'ois' ? 'Swap rate %' : 'Par yield %'}</th><th className="num">Model %</th><th className="num">Residual</th><th className="num">Zero at pillar %</th></tr></thead>
                  <tbody>
                    {curve.repricing.map((r, i) => {
                      const node = curve.nodes[i + 1];
                      return (
                        <tr key={r.helper_id}>
                          <td>{r.helper_id.slice(curve.key.id.length + 1)}</td><td>{formatLong(r.pillar)}</td>
                          <td className="num">{fmt(r.input.value * 100, 4)}</td>
                          <td className="num">{fmt(r.model.value * 100, 6)}</td>
                          <td className="num dim">{r.residual.toExponential(2)}</td>
                          <td className="num">{node ? fmt(node[1] * 100, 4) : ''}</td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              </div>
            </Panel>
          )}

          {rows.length > 0 && (
            <Panel title="Curve table" note="annual points" flush>
              <div className="table-wrap" style={{ maxHeight: 360 }}>
                <table className="grid">
                  <thead><tr><th>Tenor</th><th className="num">Zero % (cont.)</th><th className="num">3M fwd % (cont.)</th><th className="num">Discount factor</th></tr></thead>
                  <tbody>
                    {rows.filter((r) => Math.abs(r.t - Math.round(r.t)) < 1e-9 || r.t === 0.25 || r.t === 0.5).map((r) => (
                      <tr key={r.t}>
                        <td>{r.t < 1 ? `${Math.round(r.t * 12)}M` : `${r.t}Y`}</td>
                        <td className="num">{fmt(r.zero * 100, 4)}</td>
                        <td className="num">{fmt(r.fwd * 100, 4)}</td>
                        <td className="num">{fmt(r.df, 6)}</td>
                      </tr>
                    ))}
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
