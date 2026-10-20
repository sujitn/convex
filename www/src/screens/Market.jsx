import React, { useMemo, useState } from 'react';
import { useWorkspace } from '../state/workspace';
import { useStore } from '../state/store';
import { SOURCES } from '../market/feeds';
import { SNAPSHOT } from '../market/snapshot';
import { Panel, Prov, Tabs, Icon, Spinner, Field } from '../ui/primitives';
import { LineChart } from '../ui/charts';
import ScreenHead from '../ui/ScreenHead';
import { tenorYears, formatLong } from '../lib/dates';
import { isDecimal } from '../lib/decimal';
import { fmt } from '../lib/format';
import { treasuryFromAuction } from '../analytics/securities';

const GROUPS = [
  ['USD', 'United States'],
  ['EUR', 'Euro area'],
  ['CAD', 'Canada'],
  ['OTHER', 'UK and Japan'],
];

function SourceLine({ ds, error }) {
  if (!ds) return null;
  return (
    <div className="source-line">
      <Prov status={error ? 'error' : ds.status} title={error || undefined} />
      <span>As of <b className="dim">{formatLong(ds.asOf)}</b></span>
      {ds.source && <a href={ds.source.url} target="_blank" rel="noreferrer">{ds.source.name}</a>}
      {ds.status === 'snapshot' && <span>bundled {formatLong(SNAPSHOT.capturedAt.slice(0, 10))}</span>}
      {ds.status === 'manual' && <span>edited in this browser</span>}
      {error && <span className="neg">Live load failed: {error}</span>}
    </div>
  );
}

function LoadButtons({ id }) {
  const { loadOne, resetDataset, loading, datasets } = useWorkspace();
  const ds = datasets[id];
  return (
    <>
      {ds && ds.status === 'manual' && SNAPSHOT.datasets[id] && (
        <button type="button" className="btn btn--ghost btn--sm" onClick={() => resetDataset(id)}>Discard edits</button>
      )}
      <button type="button" className="btn btn--sm" onClick={() => loadOne(id)} disabled={loading[id]}>
        {loading[id] ? <Spinner label="Loading" /> : <Icon name="refresh" />} Load latest
      </button>
    </>
  );
}

function CurveDataset({ id, title }) {
  const { datasets, errors, editDataset, curves } = useWorkspace();
  const ds = datasets[id];
  const [draft, setDraft] = useState({});
  if (!ds) return null;
  const pts = [...ds.points].sort((a, b) => tenorYears(a.tenor) - tenorYears(b.tenor));
  const curve = Object.values(curves).find((c) => c && c.def && c.def.dataset === id);
  const commit = (tenor, text) => {
    setDraft((d) => ({ ...d, [tenor]: undefined }));
    if (!isDecimal(text)) return;
    editDataset(id, ds.points.map((p) => (p.tenor === tenor ? { ...p, value: text.trim() } : p)));
  };
  const remove = (tenor) => editDataset(id, ds.points.filter((p) => p.tenor !== tenor));
  return (
    <Panel title={title || ds.label} actions={<LoadButtons id={id} />}
      foot={<SourceLine ds={ds} error={errors[id]} />} flush>
      <div className="cols cols--2" style={{ gap: 0 }}>
        <div className="table-wrap" style={{ maxHeight: 360, borderRight: '1px solid var(--rule)' }}>
          <table className="grid">
            <thead>
              <tr><th>Tenor</th><th className="num">{ds.kind === 'zero' ? 'Spot %' : 'Yield %'}</th><th /></tr>
            </thead>
            <tbody>
              {pts.map((p) => (
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
                      aria-label={`Remove ${p.tenor}`} onClick={() => remove(p.tenor)}><Icon name="close" /></button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <div style={{ padding: '10px 12px' }}>
          <LineChart height={240} legend={false}
            series={[{ id, label: ds.currency, color: '--s1',
              points: pts.map((p) => ({ x: tenorYears(p.tenor), y: Number(p.value) })),
              markers: pts.map((p) => ({ x: tenorYears(p.tenor), y: Number(p.value) })) }]}
            xFormat={(v) => `${Math.round(v)}Y`} yFormat={(v) => `${v.toFixed(2)}`}
            tipX={(v) => `${fmt(v, 2)} years`} tipY={(v) => `${v.toFixed(4)}%`} />
          <div className="dim" style={{ fontSize: 12 }}>
            {!curve ? 'Reference rates; not used to build a curve.'
              : ds.kind === 'zero' ? 'Continuously compounded spot rates, used directly as curve nodes.'
                : 'Par yields; the engine calibrates a zero curve from one par bond per tenor.'}
            {curve && curve.error && <div className="neg">Curve failed: {curve.error.message}</div>}
            {curve && curve.repricing && (
              <div>Calibrated {curve.repricing.length} helpers, max residual {Math.max(...curve.repricing.map((r) => Math.abs(r.residual))).toExponential(1)}.</div>
            )}
          </div>
        </div>
      </div>
    </Panel>
  );
}

function RatesDataset({ ids }) {
  const { datasets, errors } = useWorkspace();
  return ids.map((id) => {
    const ds = datasets[id];
    if (!ds) return null;
    return (
      <Panel key={id} title={ds.label} actions={<LoadButtons id={id} />} foot={<SourceLine ds={ds} error={errors[id]} />} flush>
        <table className="grid">
          <thead><tr><th>Rate</th><th>Name</th><th className="num">%</th><th>Date</th><th className="num">Volume</th></tr></thead>
          <tbody>
            {ds.points.map((p) => (
              <tr key={p.id}>
                <td><b>{p.id}</b></td><td className="dim">{p.label}</td><td className="num">{p.value}</td>
                <td>{formatLong(p.date)}</td>
                <td className="num dim">{p.extra && p.extra.volumeBn ? `${p.extra.volumeBn}bn` : ''}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </Panel>
    );
  });
}

function SofrHistory() {
  const { datasets, errors } = useWorkspace();
  const ds = datasets['sofr-history'];
  if (!ds) return null;
  const t0 = Date.parse(ds.points[0].date);
  const pts = ds.points.map((p) => ({ x: (Date.parse(p.date) - t0) / 86400000, y: Number(p.value), date: p.date }));
  return (
    <Panel title="SOFR fixings" actions={<LoadButtons id="sofr-history" />} foot={<SourceLine ds={ds} error={errors['sofr-history']} />}>
      <LineChart height={200} legend={false}
        series={[{ id: 'sofr', label: 'SOFR', color: '--s2', points: pts }]}
        xFormat={(v) => formatLong(new Date(t0 + v * 86400000).toISOString().slice(0, 10)).replace(/ \d{4}$/, '')}
        yFormat={(v) => v.toFixed(2)} tipX={(v, p) => formatLong(p.date)} tipY={(v) => `${v.toFixed(2)}%`} />
    </Panel>
  );
}

function Auctions() {
  const { datasets, errors } = useWorkspace();
  const { dispatch } = useStore();
  const ds = datasets['ust-auctions'];
  const [filter, setFilter] = useState('Coupon');
  if (!ds) return null;
  const rows = ds.securities.filter((s) => (filter === 'Bills' ? s.type === 'Bill' : s.type !== 'Bill'));
  const open = (a) => {
    const sec = treasuryFromAuction(a);
    dispatch({ type: 'select', id: sec.id });
    dispatch({ type: 'screen', screen: a.type === 'Bill' ? 'BILL' : 'YAS' });
  };
  return (
    <Panel title="Treasury auction results" note="actual securities and their auction prices"
      actions={<><Tabs value={filter} onChange={setFilter} options={['Coupon', 'Bills']} label="Security type" /><LoadButtons id="ust-auctions" /></>}
      foot={<SourceLine ds={ds} error={errors['ust-auctions']} />} flush>
      <div className="table-wrap" style={{ maxHeight: 420 }}>
        <table className="grid">
          <thead>
            <tr><th>Auction</th><th>Type</th><th>Term</th><th>CUSIP</th><th className="num">Coupon</th><th>Maturity</th>
              <th className="num">{filter === 'Bills' ? 'Discount %' : 'High yield %'}</th><th className="num">Price</th><th /></tr>
          </thead>
          <tbody>
            {rows.map((a) => (
              <tr key={`${a.cusip}${a.auctionDate}`}>
                <td>{formatLong(a.auctionDate)}</td><td>{a.type}</td><td className="dim">{a.term}{a.reopening ? ' · reopening' : ''}</td>
                <td>{a.cusip}</td><td className="num">{a.coupon ? Number(a.coupon).toFixed(3) : '—'}</td><td>{formatLong(a.maturity)}</td>
                <td className="num">{filter === 'Bills' ? a.highDiscountRate : a.highYield}</td>
                <td className="num">{a.pricePer100}</td>
                <td><button type="button" className="btn btn--sm" onClick={() => open(a)}>{a.type === 'Bill' ? 'BILL' : 'YAS'}</button></td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </Panel>
  );
}

function ProxySettings() {
  const { state, dispatch } = useStore();
  const proxySources = SOURCES.filter((s) => s.corsRequired);
  const { datasets, errors } = useWorkspace();
  return (
    <div className="stack">
      <div className="note">
        These publishers don't allow browser access from other sites, so they need a CORS proxy you trust.
        Leave the template empty to skip them. The template must contain <code>{'{url}'}</code>, for example
        <code> https://your-proxy.example/?url={'{url}'}</code>.
      </div>
      <Field label="Proxy URL template">
        <input value={state.proxy} placeholder="https://…?url={url}" onChange={(e) => dispatch({ type: 'proxy', proxy: e.target.value })} />
      </Field>
      {proxySources.map((s) => (
        datasets[s.id] ? <RatesOrCurve key={s.id} id={s.id} /> : (
          <Panel key={s.id} title={s.label} actions={state.proxy.includes('{url}') ? <LoadButtons id={s.id} /> : null}
            foot={errors[s.id] ? <span className="neg">{errors[s.id]}</span> : <a href={s.homepage || s.url} target="_blank" rel="noreferrer">{s.homepage || s.url}</a>}>
            <div className="dim">{s.description}</div>
          </Panel>
        )
      ))}
    </div>
  );
}

function RatesOrCurve({ id }) {
  const { datasets } = useWorkspace();
  const ds = datasets[id];
  return ds.kind === 'rates' ? <RatesDataset ids={[id]} /> : <CurveDataset id={id} />;
}

export default function Market({ fn }) {
  const { loadAll, loading } = useWorkspace();
  const [group, setGroup] = useState('USD');
  const busy = useMemo(() => Object.values(loading).some(Boolean), [loading]);
  return (
    <div className="screen">
      <ScreenHead fn={fn} dates={false}
        lede="Free public sources, loaded straight from the publishers. Every value can be edited; edits stay in this browser and are marked as such.">
        <button type="button" className="btn btn--primary" onClick={() => loadAll()} disabled={busy}>
          {busy ? <Spinner label="Loading" /> : <Icon name="refresh" />} Load all latest
        </button>
      </ScreenHead>
      <Tabs value={group} onChange={setGroup} options={GROUPS} label="Market" />
      {group === 'USD' && (
        <div className="stack">
          <div className="cols cols--wide">
            <CurveDataset id="ust-par" title="Treasury par yield curve" />
            <div className="stack"><RatesDataset ids={['nyfed-rates']} /></div>
          </div>
          <div className="cols cols--2">
            <CurveDataset id="ust-bills" title="Treasury bill rates (coupon-equivalent)" />
            <SofrHistory />
          </div>
          <Auctions />
        </div>
      )}
      {group === 'EUR' && (
        <div className="stack">
          <CurveDataset id="ecb-aaa" title="Euro area AAA government spot curve (ECB)" />
          <CurveDataset id="ecb-all" title="Euro area all-issuer government spot curve (ECB)" />
          <RatesDataset ids={['estr']} />
        </div>
      )}
      {group === 'CAD' && (
        <div className="stack">
          <CurveDataset id="boc-benchmarks" title="Government of Canada benchmark bond yields" />
          <RatesDataset ids={['corra']} />
        </div>
      )}
      {group === 'OTHER' && <ProxySettings />}
    </div>
  );
}
