import React, { useRef, useState } from 'react';
import { useWorkspace } from '../state/workspace';
import { useStore } from '../state/store';
import { exportData, readImport } from '../lib/userdata';
import { CURVE_METHODS, customCurveDef, CALENDAR_FOR } from '../analytics/curves';
import { OIS_INDICES, MAX_OIS_YEARS, EXAMPLE_OIS, examplePoints } from '../lib/ois';
import { tenorYears, formatLong, isIso } from '../lib/dates';
import { fmt } from '../lib/format';
import { Panel, Field, Seg, Select, Prov, Spinner, Icon, KV, EngineError } from '../ui/primitives';
import { LineChart } from '../ui/charts';
import PointsEditor from '../ui/PointsEditor';
import BondImport from './BondImport';

// Tenors offered in the copyable curve template, by curve type.
const TEMPLATE_TENORS = {
  ois: ['1W', '1M', '3M', '6M', '1Y', '2Y', '3Y', '5Y', '7Y', '10Y', '12Y', '15Y'],
  par: ['1M', '3M', '6M', '1Y', '2Y', '3Y', '5Y', '7Y', '10Y', '20Y', '30Y'],
  zero: ['3M', '6M', '1Y', '2Y', '3Y', '5Y', '7Y', '10Y', '15Y', '20Y', '30Y', '40Y', '50Y'],
};

const CURRENCIES = ['USD', 'EUR', 'GBP', 'CAD'];
const DAY_COUNTS = [['Act360', 'Act/360'], ['Act365Fixed', 'Act/365F'], ['ActActIsda', 'Act/Act ISDA']];
const PAR_DAY_COUNTS = [['ActActIcma', 'Act/Act ICMA'], ['Act365Fixed', 'Act/365F'], ['Thirty360US', '30/360 US'], ['Thirty360E', '30E/360']];

function slug(text) {
  return String(text).trim().toUpperCase().replace(/[^A-Z0-9]+/g, '.').replace(/^\.+|\.+$/g, '') || 'CURVE';
}

function NewCurve({ onCreate, onCancel, existing, tradeDate }) {
  const [method, setMethod] = useState('ois');
  const [index, setIndex] = useState('USD.SOFR');
  const [currency, setCurrency] = useState('USD');
  const [role, setRole] = useState('discount');
  const [name, setName] = useState('');
  const [asOf, setAsOf] = useState(tradeDate);
  const [points, setPoints] = useState([]);
  const [example, setExample] = useState(false);
  const ccy = method === 'ois' ? OIS_INDICES[index].currency : currency;
  const label = name.trim() || (method === 'ois' ? `${OIS_INDICES[index].label} OIS` : method === 'par' ? `${ccy} par curve` : `${ccy} zero curve`);
  let id = `${ccy}.${slug(label)}`;
  for (let n = 2; existing[id]; n++) id = `${ccy}.${slug(label)}.${n}`;
  const tooLong = method === 'ois' && points.some((p) => tenorYears(p.tenor) > MAX_OIS_YEARS);
  const ok = points.length >= 2 && /^\d{4}-\d{2}-\d{2}$/.test(asOf) && !tooLong;
  const create = () => {
    const def = customCurveDef({ id, label, currency: ccy, method, asOf, points, index, role });
    onCreate(example ? { ...def, example: true } : def);
  };
  return (
    <Panel title="New curve" actions={<button type="button" className="btn btn--ghost btn--sm" onClick={onCancel}>Cancel</button>}>
      <div className="stack">
        <Field label="What you are supplying">
          <Seg value={method} onChange={setMethod} label="Curve type" options={Object.entries(CURVE_METHODS)} />
        </Field>
        <div className="form form--2">
          {method === 'ois' ? (
            <Field label="Overnight index" hint="Spot-starting par swaps, annual fixed vs compounded overnight">
              <Select value={index} onChange={setIndex} options={Object.entries(OIS_INDICES).map(([k, v]) => [k, `${v.label} (${v.currency})`])} />
            </Field>
          ) : (
            <Field label="Currency"><Select value={currency} onChange={setCurrency} options={CURRENCIES} /></Field>
          )}
          {method === 'zero' && (
            <Field label="Use as" hint="Swap or collateral curves price spreads in the collateral role">
              <Select value={role} onChange={setRole} options={[['discount', 'Swap / collateral discount curve'], ['government', 'Government curve']]} />
            </Field>
          )}
          <Field label="Name"><input value={name} placeholder={label} onChange={(e) => setName(e.target.value)} /></Field>
          <Field label="Curve date"><input type="date" value={asOf} onChange={(e) => setAsOf(e.target.value)} /></Field>
        </div>
        <div className="dim" style={{ fontSize: 12.5 }}>
          {method === 'ois' && <>Enter par swap rates in percent. The engine calibrates a discount curve from one spot-starting OIS per tenor, up to {MAX_OIS_YEARS} years (one request carries one publication record per overnight date). Supply longer points as a zero curve.</>}
          {method === 'par' && <>Enter par yields in percent. The engine calibrates from one par bond per tenor, semi-annual, settling T+1 on the {CALENDAR_FOR[ccy] || 'US.SIFMA'} calendar.</>}
          {method === 'zero' && <>Enter continuously compounded zero rates in percent, Act/365F time. They are used directly as curve nodes, any horizon.</>}
        </div>
        <PointsEditor key={method} points={points} onChange={(p) => { setPoints(p); setExample(false); }} maxHeight={260}
          pasteOpen templateTenors={TEMPLATE_TENORS[method]} />
        {method === 'ois' && EXAMPLE_OIS[index] && (
          <div className="row" style={{ gap: 10, alignItems: 'center' }}>
            <button type="button" className="btn btn--sm" onClick={() => { setPoints(examplePoints(index)); setExample(true); }}>
              Fill with example rates
            </button>
            <span className="dim" style={{ fontSize: 12 }}>Hypothetical levels to try the route, not market data. The curve is marked as an example.</span>
          </div>
        )}
        {tooLong && <div className="note note--warn">OIS tenors beyond {MAX_OIS_YEARS} years can't be calibrated in one request. Remove them, or create a zero curve for the long end.</div>}
        <div className="row">
          <div className="grow" />
          <button type="button" className="btn btn--primary" disabled={!ok} onClick={create}>Create and build</button>
        </div>
      </div>
    </Panel>
  );
}

function Conventions({ def, update }) {
  if (def.method === 'ois') {
    return (
      <div className="form form--3">
        <Field label="Spot lag, business days"><input className="input--num" value={def.spotLag}
          onChange={(e) => /^\d{0,1}$/.test(e.target.value) && e.target.value !== '' && update({ spotLag: Number(e.target.value) })} /></Field>
        <Field label="Payment delay, business days"><input className="input--num" value={def.paymentDelay}
          onChange={(e) => /^\d{0,1}$/.test(e.target.value) && e.target.value !== '' && update({ paymentDelay: Number(e.target.value) })} /></Field>
        <Field label="Day count, both legs"><Select value={def.dayCount} onChange={(v) => update({ dayCount: v })} options={DAY_COUNTS} /></Field>
      </div>
    );
  }
  if (def.method === 'par') {
    return (
      <div className="form form--3">
        <Field label="Coupon frequency"><Select value={def.frequency} onChange={(v) => update({ frequency: v })} options={[['SemiAnnual', 'Semi-annual'], ['Annual', 'Annual'], ['Quarterly', 'Quarterly']]} /></Field>
        <Field label="Day count"><Select value={def.dayCount} onChange={(v) => update({ dayCount: v })} options={PAR_DAY_COUNTS} /></Field>
        <Field label="Settlement, business days"><input className="input--num" value={def.settlementDays}
          onChange={(e) => /^\d$/.test(e.target.value) && update({ settlementDays: Number(e.target.value) })} /></Field>
      </div>
    );
  }
  return (
    <div className="form form--3">
      <Field label="Use as"><Select value={def.role} onChange={(v) => update({ role: v })} options={[['discount', 'Swap / collateral'], ['government', 'Government']]} /></Field>
      <Field label="Interpolation"><Select value={def.interpolation} onChange={(v) => update({ interpolation: v })} options={[['CubicSpline', 'Cubic spline'], ['Linear', 'Linear zero']]} /></Field>
    </div>
  );
}

function CurveEditor({ def, curve, onRemove }) {
  const { saveCurve, curveChoices, curveId, selected } = useWorkspace();
  const { state, dispatch } = useStore();
  const update = (patch) => saveCurve({ ...def, ...patch, revision: (def.revision || 1) + 1, example: patch.points ? false : def.example });
  const used = state.spreadCurve[def.currency] === def.id;
  const maxResidual = curve && curve.repricing ? Math.max(...curve.repricing.map((r) => Math.abs(r.residual))) : null;
  const input = def.points.map((p) => ({ x: tenorYears(p.tenor), y: Number(p.value) })).sort((a, b) => a.x - b.x);
  const fitted = curve && curve.nodes ? curve.nodes.filter(([t]) => t > 0).map(([t, v]) => ({ x: t, y: v * 100 })) : [];
  return (
    <Panel title={def.label}
      note={`${CURVE_METHODS[def.method]}${def.method === 'ois' ? `, ${OIS_INDICES[def.index].label}` : ''}, ${def.currency}`}
      actions={(
        <>
          {def.example && <Prov status="derived">Example rates</Prov>}
          <button type="button" className="btn btn--sm" disabled={used}
            onClick={() => dispatch({ type: 'spreadCurve', currency: def.currency, id: def.id })}>
            {used ? `Used for ${def.currency} spreads` : `Use for ${def.currency} spreads`}
          </button>
          <button type="button" className="icon-btn" aria-label={`Delete ${def.label}`} title="Delete curve" onClick={onRemove}><Icon name="trash" /></button>
        </>
      )}>
      <div className="stack">
        <div className="form form--3">
          <Field label="Name"><input defaultValue={def.label} key={def.id} onBlur={(e) => e.target.value.trim() && e.target.value !== def.label && update({ label: e.target.value.trim() })} /></Field>
          <Field label="Curve date"><input type="date" value={def.asOf} onChange={(e) => /^\d{4}-\d{2}-\d{2}$/.test(e.target.value) && update({ asOf: e.target.value })} /></Field>
          <Field label="Curve id" hint="Key sent to the engine"><input value={def.id} readOnly /></Field>
        </div>
        <Conventions def={def} update={update} />
        <div className="cols cols--2" style={{ gap: 16 }}>
          <PointsEditor points={def.points} onChange={(points) => update({ points })} templateTenors={TEMPLATE_TENORS[def.method]}
            valueLabel={def.method === 'ois' ? 'Swap rate %' : def.method === 'par' ? 'Par yield %' : 'Zero %'} />
          <div>
            <LineChart height={220}
              series={[
                { id: 'in', label: def.method === 'ois' ? 'Swap rates' : def.method === 'par' ? 'Par yields' : 'Zero input', color: '--s3', points: input, markers: input },
                ...(fitted.length && def.method !== 'zero' ? [{ id: 'z', label: 'Fitted zero (cont.)', color: '--s1', points: fitted }] : []),
              ]}
              xFormat={(v) => `${Math.round(v)}Y`} yFormat={(v) => v.toFixed(2)}
              tipX={(v) => `${fmt(v, 2)} years`} tipY={(v) => `${v.toFixed(4)}%`} />
            {(!curve || curve.building) && <div className="dim"><Spinner /> Building…</div>}
            {curve && !curve.building && curve.error && <EngineError error={curve.error} context="Build" />}
            {curve && !curve.building && curve.recipe && (
              <KV rows={[
                ['Status', <Prov key="s" status="live">Built</Prov>, curve.ms ? `${Math.round(curve.ms)} ms in the engine` : null],
                curve.repricing && ['Helpers repriced', `${curve.repricing.length}`, `max residual ${maxResidual.toExponential(1)}`],
                curve.publications && ['Publications supplied', curve.publications.toLocaleString()],
                ['Role', def.role === 'discount' ? 'Discount, collateral spreads' : 'Government'],
              ]} />
            )}
          </div>
        </div>
        {selected && selected.currency === def.currency && curveId === def.id && (
          <div className="dim" style={{ fontSize: 12.5 }}>YAS, CFL and OAS for {selected.ticker || selected.name} now price against this curve. Choose another under the spread curve selector.</div>
        )}
        {curveChoices.length === 1 && def.currency !== 'USD' && def.currency !== 'EUR' && def.currency !== 'CAD' && (
          <div className="dim" style={{ fontSize: 12.5 }}>This is the only {def.currency} curve, so {def.currency} securities use it.</div>
        )}
      </div>
    </Panel>
  );
}

function Transfer() {
  const { state, dispatch } = useStore();
  const file = useRef(null);
  const [message, setMessage] = useState(null);
  const counts = {
    curves: Object.keys(state.customCurves).length,
    credit: Object.keys(state.credit || {}).length,
    securities: Object.keys(state.custom).length,
    prices: Object.keys(state.benchPrices).length,
    datasets: Object.values(state.datasets).filter((d) => d && d.status === 'manual').length,
  };
  const download = () => {
    const blob = new Blob([JSON.stringify(exportData(state), null, 2)], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `convex-terminal-data-${new Date().toISOString().slice(0, 10)}.json`;
    a.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  };
  const upload = (e) => {
    const f = e.target.files && e.target.files[0];
    if (!f) return;
    f.text().then((text) => {
      try {
        const data = readImport(JSON.parse(text), { customCurves: state.customCurves });
        dispatch({ type: 'importData', data });
        const skipped = data.rejected.length;
        setMessage({ ok: true, text: `Imported ${Object.keys(data.customCurves).length} curves, ${Object.keys(data.credit).length} credit curves, ${Object.keys(data.custom).length} securities, `
          + `${Object.keys(data.benchPrices).length} benchmark prices and ${Object.keys(data.datasets).length} edited datasets.`
          + (skipped ? ` Skipped ${skipped} malformed record${skipped === 1 ? '' : 's'}: `
            + `${data.rejected.slice(0, 3).map((r) => `${r.section} ${r.id} (${r.reason})`).join('; ')}${skipped > 3 ? '…' : ''}.` : '') });
      } catch (err) {
        setMessage({ ok: false, text: err.message });
      }
      e.target.value = '';
    });
  };
  return (
    <Panel title="Save and share your data" note="one JSON file">
      <div className="stack">
        <div className="dim" style={{ fontSize: 12.5 }}>
          In this browser: {counts.curves} curves, {counts.credit} credit curves, {counts.securities} securities, {counts.prices} benchmark prices, {counts.datasets} edited datasets.
          Download them to keep a copy or move them to another browser; importing merges into what is here.
        </div>
        <div className="row" style={{ gap: 8 }}>
          <button type="button" className="btn" onClick={download}><Icon name="download" /> Download your data</button>
          <button type="button" className="btn" onClick={() => file.current && file.current.click()}><Icon name="upload" /> Import a data file</button>
          <input ref={file} type="file" accept="application/json,.json" hidden onChange={upload} />
        </div>
        {message && <div className={message.ok ? 'note' : 'note note--error'}>{message.text}</div>}
      </div>
    </Panel>
  );
}

const SUPPLY = [
  ['Swap and other curves', 'Below: paste a tenor and rate column from Excel as OIS swap rates (SOFR, €STR, SONIA, CORRA) calibrated by the engine, par yields, or zero rates. Use one for spreads on YAS, CFL and OAS.'],
  ['Bonds', 'Below or on SEC: paste bond reference data (CUSIP or ISIN, coupon, maturity, optional dates, conventions, price, calls) from a spreadsheet.'],
  ['Market data', 'Curve values on the other tabs can be edited; add tenors or paste a whole table into any curve.'],
  ['Benchmark prices', 'On YAS, type actual prices for the on-the-run Treasuries; G-spread then uses yours instead of curve-implied ones.'],
  ['Securities', 'On SEC, copy any bond as a custom security and change its terms, calls, steps or sinking schedule.'],
  ['Model inputs', 'Volatility and mean reversion on OAS; trade date and face on every screen.'],
];

export default function YourData() {
  const { curveDefs, curves, saveCurve, removeCurve, tradeDate } = useWorkspace();
  const mine = Object.values(curveDefs).filter((d) => d.custom);
  const [editing, setEditing] = useState(() => (mine[0] ? mine[0].id : 'new'));
  const current = editing === 'new' ? null : curveDefs[editing];
  return (
    <div className="stack">
      <Panel title="What you can supply" flush>
        <table className="grid">
          <tbody>
            {SUPPLY.map(([k, v]) => <tr key={k}><td style={{ width: 200 }}><b>{k}</b></td><td className="dim">{v}</td></tr>)}
          </tbody>
        </table>
      </Panel>
      <div className="cols cols--side">
        <Panel title="Your curves" flush
          actions={<button type="button" className="btn btn--sm" onClick={() => setEditing('new')}><Icon name="plus" /> New</button>}>
          {mine.length === 0 && <div className="empty">No curves yet.</div>}
          <div role="listbox" aria-label="Your curves">
            {mine.map((d) => {
              const c = curves[d.id];
              const status = !c || c.building ? 'snapshot' : c.error ? 'error' : 'live';
              return (
                <div key={d.id} role="option" tabIndex={0} aria-selected={editing === d.id}
                  className={`list-row${editing === d.id ? ' is-active' : ''}`}
                  onClick={() => setEditing(d.id)} onKeyDown={(e) => e.key === 'Enter' && setEditing(d.id)}>
                  <div>
                    <div><b>{d.label}</b></div>
                    <div className="dim" style={{ fontSize: 11.5 }}>{CURVE_METHODS[d.method]}, {d.points.length} points, {isIso(d.asOf) ? formatLong(d.asOf) : d.asOf}</div>
                  </div>
                  <Prov status={status}>{!c || c.building ? 'Building' : c.error ? 'Failed' : 'Built'}</Prov>
                </div>
              );
            })}
          </div>
        </Panel>
        {editing === 'new' || !current ? (
          <NewCurve existing={curveDefs} tradeDate={tradeDate}
            onCancel={() => setEditing(mine[0] ? mine[0].id : 'new')}
            onCreate={(def) => { saveCurve(def); setEditing(def.id); }} />
        ) : (
          <CurveEditor def={current} curve={curves[current.id]}
            onRemove={() => { removeCurve(current.id); setEditing(mine.find((d) => d.id !== current.id)?.id || 'new'); }} />
        )}
      </div>
      <BondImport />
      <Transfer />
    </div>
  );
}

