import React, { useEffect, useMemo, useState } from 'react';
import { useWorkspace } from '../state/workspace';
import { useStore } from '../state/store';
import * as R from '../lib/requests';
import { cashConvention } from '../lib/cash';
import { CURVE_DEFS } from '../analytics/curves';
import { Panel, Tabs, EngineError, Spinner, Prov } from '../ui/primitives';
import ScreenHead from '../ui/ScreenHead';
import { JsonView, stringify } from '../ui/Inspector';
import { ms } from '../lib/format';

function templates({ sec, settlement, curve }) {
  const out = [
    { id: 'capabilities', label: 'Capabilities report', build: () => R.request(R.inputs(), { kind: 'capabilities' }) },
    { id: 'roundtrip', label: 'Round-trip inputs', build: () => R.request(R.inputs({ calendars: ['US.SIFMA'] }), { kind: 'roundtrip' }) },
  ];
  if (curve && curve.recipe) {
    out.push(
      { id: 'curves', label: `Build ${curve.key.id}`, build: () => R.curvesBuild([curve.recipe]) },
      { id: 'zero', label: `${curve.key.id} 10Y zero rate`, build: () => R.curveQuery([curve.recipe], curve.key, R.zeroQuery(10, 'SemiAnnual')) },
      { id: 'fwd', label: `${curve.key.id} 5Y→10Y forward`, build: () => R.curveQuery([curve.recipe], curve.key, R.forwardQuery(5, 10, 'Annual')) },
    );
  }
  if (sec && sec.kind === 'fixed') {
    const bullet = { ...sec, calls: [] };
    out.push(
      { id: 'construct', label: `Construct ${sec.ticker}`, build: () => R.bondConstruct(sec, settlement) },
      { id: 'cashflows', label: `${sec.ticker} cash flows`, build: () => R.bondCashflows(bullet, settlement) },
      { id: 'icma', label: `${sec.ticker} price at 5% (ICMA)`, build: () => R.icmaPrice(bullet, settlement, 0.05, 'SemiAnnual', '1000000') },
      { id: 'yinv', label: `${sec.ticker} yield from 99.5 (dated cash)`, build: () => R.bondYieldInverse(bullet, settlement, { kind: 'clean', value: 99.5 }, cashConvention(sec)) },
    );
    if (curve && curve.recipe) {
      const role = R.governmentRole(CURVE_DEFS[curve.id].benchmarkId);
      out.push({ id: 'zinv', label: `${sec.ticker} Z-spread from 99.5`, build: () => R.bondSpreadInverse(bullet, settlement, { kind: 'clean', value: 99.5 }, curve.recipe, role) });
    }
  }
  return out;
}

function SchemaBrowser({ engine }) {
  const [schema, setSchema] = useState(null);
  const [q, setQ] = useState('BondTerms');
  const [error, setError] = useState(null);
  useEffect(() => {
    engine.schema().then((s) => setSchema(JSON.parse(s)), (e) => setError(e.message));
  }, [engine]);
  const defs = schema ? schema.$defs || schema.definitions || {} : {};
  const names = useMemo(() => Object.keys(defs).filter((n) => n.toLowerCase().includes(q.toLowerCase())).sort(), [defs, q]);
  const [pick, setPick] = useState('BondTerms');
  if (error) return <div className="note note--error">{error}</div>;
  if (!schema) return <div className="empty"><Spinner /> Loading the generated schema…</div>;
  return (
    <div className="cols" style={{ gridTemplateColumns: 'minmax(200px, 260px) minmax(0, 1fr)', gap: 0 }}>
      <div style={{ borderRight: '1px solid var(--rule)' }}>
        <div style={{ padding: 10 }}>
          <input className="input" value={q} onChange={(e) => setQ(e.target.value)} placeholder="Search definitions" aria-label="Search schema definitions" />
          <div className="muted" style={{ fontSize: 11.5, marginTop: 6 }}>{Object.keys(defs).length} definitions</div>
        </div>
        <div className="table-wrap" style={{ maxHeight: 520 }}>
          <table className="grid">
            <tbody>
              {names.map((n) => (
                <tr key={n} className={n === pick ? 'is-active' : ''} style={{ cursor: 'pointer' }} onClick={() => setPick(n)}><td>{n}</td></tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
      <div style={{ maxHeight: 600, overflow: 'auto' }}>
        {defs[pick] && <JsonView value={defs[pick]} />}
      </div>
    </div>
  );
}

export default function ApiConsole({ fn }) {
  const { engine, selected: sec, settlement, curve } = useWorkspace();
  const { state } = useStore();
  const list = templates({ sec, settlement, curve });
  const [tab, setTab] = useState('console');
  const [text, setText] = useState(() => stringify(state.consoleRequest || list[0].build()));
  const [result, setResult] = useState(null);
  const [busy, setBusy] = useState(false);
  const [parseError, setParseError] = useState(null);

  useEffect(() => {
    if (state.consoleRequest) setText(stringify(state.consoleRequest));
  }, [state.consoleRequest]);

  const run = async () => {
    let req;
    try {
      req = JSON.parse(text);
      setParseError(null);
    } catch (e) {
      setParseError(e.message);
      return;
    }
    setBusy(true);
    const res = await engine.call(req, { label: 'API console' });
    setBusy(false);
    setResult(res);
  };

  return (
    <div className="screen">
      <ScreenHead fn={fn} dates={false}
        lede="The same library() call every screen makes. Start from a template built from the current security and curve, edit it, run it.">
        <Tabs value={tab} onChange={setTab} options={[['console', 'Console'], ['schema', 'Schema']]} label="View" />
      </ScreenHead>
      {tab === 'schema' ? (
        <Panel title="Generated request schema" note="library_schema()" flush><SchemaBrowser engine={engine} /></Panel>
      ) : (
        <div className="cols cols--2">
          <Panel title="Request" flush
            actions={(
              <>
                <select className="input" style={{ width: 230, height: 26 }} aria-label="Template" value=""
                  onChange={(e) => {
                    const t = list.find((x) => x.id === e.target.value);
                    if (t) setText(stringify(t.build()));
                  }}>
                  <option value="">Load a template…</option>
                  {list.map((t) => <option key={t.id} value={t.id}>{t.label}</option>)}
                </select>
                <button type="button" className="btn btn--primary btn--sm" onClick={run} disabled={busy}>
                  {busy ? <Spinner /> : null} Run
                </button>
              </>
            )}>
            <textarea className="json" style={{ width: '100%', minHeight: 560, border: 0, background: 'var(--field)', resize: 'vertical', outline: 'none' }}
              spellCheck={false} value={text} onChange={(e) => setText(e.target.value)} aria-label="Request JSON"
              onKeyDown={(e) => { if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') run(); }} />
            {parseError && <div className="panel__foot neg">JSON: {parseError}</div>}
            <div className="panel__foot">Ctrl + Enter runs the request. {(text.length / 1024).toFixed(1)} KB.</div>
          </Panel>
          <Panel title="Response" flush
            actions={result && (
              <span className="row" style={{ gap: 10 }}>
                <Prov status={result.ok ? 'live' : 'error'}>{result.ok ? 'ok' : result.error.code}</Prov>
                <span className="dim tabular">{ms(result.ms)}</span>
              </span>
            )}>
            {!result && <div className="empty">Run a request to see the native envelope.</div>}
            {result && !result.ok && <div className="panel__body"><EngineError error={result.error} /></div>}
            {result && (
              <div style={{ maxHeight: 600, overflow: 'auto' }}>
                <JsonView value={result.ok ? { data: result.data, market_calibration: result.calibration } : result.error} />
              </div>
            )}
          </Panel>
        </div>
      )}
    </div>
  );
}
