import React, { useEffect, useRef, useState } from 'react';
import { useWorkspace, useComputation } from '../state/workspace';
import { useStore } from '../state/store';
import { computeYas } from '../analytics/yas';
import * as R from '../lib/requests';
import { workoutPaths, workoutRows } from '../lib/callable';
import { cashConvention } from '../lib/cash';
import { Panel, Seg, Field, Figure, KV, EngineError, Empty, Prov, Spinner } from '../ui/primitives';
import SecurityBand, { dayCountLabel } from '../ui/SecurityBand';
import { LineChart } from '../ui/charts';
import ScreenHead from '../ui/ScreenHead';
import CurvePicker from '../ui/CurvePicker';
import { fmt, pct, bp, signedBp, signed, money } from '../lib/format';
import { formatThirtySeconds, parsePrice } from '../lib/decimal';
import { formatLong } from '../lib/dates';

const KINDS = [
  ['price', 'Price'],
  ['yield', 'Yield %'],
  ['zspread', 'Z-spread bp'],
  ['gspread', 'G-spread bp'],
];

// Actual Treasuries open on a price: the curve-implied clean price, rounded to
// the nearest 1/256 the way a screen quotes it, so Z and G read as results.
// The examples open on a spread, since they have no market of their own.
function defaultQuote(sec) {
  return sec.source === 'auction' ? { kind: 'price', text: '' } : { kind: 'zspread', text: '125' };
}

/** Debounce typed text so every keystroke doesn't fire a dozen engine calls. */
function useDebounced(value, delay = 350) {
  const [v, setV] = useState(value);
  useEffect(() => {
    const t = setTimeout(() => setV(value), delay);
    return () => clearTimeout(t);
  }, [value, delay]);
  return v;
}

function quoteValid(q) {
  if (!q.text.trim()) return false;
  if (q.kind === 'price') {
    try {
      return parsePrice(q.text) > 0;
    } catch {
      return false;
    }
  }
  return Number.isFinite(Number(q.text));
}

export default function Yas({ fn }) {
  const { engine, selected: sec, curve, govCurve, benchmarks, benchPrices, tradeDate, settlement } = useWorkspace();
  const { state, dispatch } = useStore();
  const [quotes, setQuotes] = useState({});
  const quote = (sec && quotes[sec.id]) || (sec && defaultQuote(sec));
  const seeding = useRef(null);
  const face = state.quote.face || '1000000';
  const dq = useDebounced(quote && `${quote.kind}|${quote.text}`);
  const dFace = useDebounced(face);
  const usd = sec && sec.currency === 'USD';

  const benchKey = JSON.stringify(benchPrices);
  const ready = sec && sec.kind !== 'bill' && curve && !curve.building && quote && quoteValid(quote)
    && !(usd && govCurve && govCurve.building);
  const { data, error, busy } = useComputation(
    () => (ready
      ? computeYas(engine, { sec, tradeDate, settlement, quote, face: dFace, curve, benchmarks: usd ? benchmarks : [],
        benchmarkCurve: govCurve, benchPrices })
      : null),
    [sec && sec.id, sec, dq, dFace, curve && curve.key && curve.key.version, curve && curve.building,
      govCurve && govCurve.key && govCurve.key.version, benchKey, tradeDate, ready],
  );

  // Seed (or re-seed after a curve rebuild) an untouched Treasury quote with
  // its price off the Treasury curve; a typed quote is never replaced.
  const stored = sec && quotes[sec.id];
  const gov = govCurve;
  const seedable = sec && sec.source === 'auction' && sec.kind !== 'bill' && gov && gov.recipe && !gov.building
    && (!stored || (stored.seed && stored.seed.version !== gov.key.version));
  useEffect(() => {
    if (!seedable) return;
    const sig = `${sec.id}|${gov.key.version}`;
    if (seeding.current === sig) return;
    seeding.current = sig;
    engine.call(R.bondPrice(sec, settlement, R.spreadMethod(gov.key, gov.spreadRole, 0), [gov.recipe]),
      { label: `${sec.ticker} curve-implied price` }).then((res) => {
      if (seeding.current !== sig) return;
      setQuotes((all) => {
        const cur = all[sec.id];
        if (cur && !cur.seed) return all;
        if (!res.ok) return { ...all, [sec.id]: { kind: 'price', text: '' } };
        const seed = { version: gov.key.version, curve: gov.def.label, asOf: gov.dataset.asOf };
        return { ...all, [sec.id]: { kind: 'price', text: formatThirtySeconds(res.data.clean_per100), seed } };
      });
    });
  }, [seedable, sec, settlement, gov, engine]);

  const callable = sec && (sec.calls || []).length > 0;
  const workout = useComputation(
    () => (callable && data && Number.isFinite(data.clean)
      ? engine.call(workoutPaths(sec, tradeDate, settlement, { price: data.clean, convention: cashConvention(sec) }),
        { label: `${sec.ticker} workouts` })
      : null),
    [callable, data && data.clean, sec && sec.id, tradeDate],
  );

  const setQuote = (q) => setQuotes((all) => ({ ...all, [sec.id]: { ...quote, ...q, seed: null } }));
  const switchKind = (kind) => {
    if (!data || data.fatal) return setQuote({ kind, text: '' });
    const text = {
      price: fmt(data.clean, 6).replace(/,/g, ''),
      yield: (data.ytm * 100).toFixed(5),
      zspread: data.zspread !== undefined ? (data.zspread * 10000).toFixed(2) : '',
      gspread: data.gspread ? data.gspread.bp.toFixed(2) : '',
    }[kind];
    setQuote({ kind, text });
  };

  if (!sec) return <Empty>Choose a security from the command line or SEC.</Empty>;
  if (sec.kind === 'bill') {
    return (
      <div className="screen">
        <ScreenHead fn={fn} />
        <SecurityBand sec={sec} settlement={settlement} />
        <div className="note">Treasury bills are quoted on a discount basis. Open <button type="button" className="btn btn--sm"
          onClick={() => dispatch({ type: 'screen', screen: 'BILL' })}>BILL</button> for this security.</div>
      </div>
    );
  }

  const usd32 = usd && sec.source === 'auction';
  const r = data && !data.fatal ? data : null;
  const fatal = data && data.fatal;
  const fatalOrError = fatal || error;

  return (
    <div className="screen">
      <ScreenHead fn={fn} lede="Price, yield, spread and risk from one quote, every figure computed by the library." />
      <SecurityBand sec={sec} settlement={settlement} />

      <section className="panel" aria-label="Quote" style={{ marginBottom: 16 }}>
        <div className="panel__body">
          <div className="row" style={{ alignItems: 'flex-start', gap: 16 }}>
            <Field label="Quote as">
              <Seg value={quote.kind} onChange={switchKind} options={usd ? KINDS : KINDS.slice(0, 3)} label="Quote type" />
            </Field>
            <Field label={quote.kind === 'price' ? (usd32 ? 'Clean price (decimal or 32nds, e.g. 99-16+)' : 'Clean price')
              : quote.kind === 'yield' ? 'Yield, ICMA, % ' : quote.kind === 'zspread' ? `Z-spread over ${curve ? curve.def.label : 'curve'}, bp` : 'G-spread, bp'}
              hint={quote.seed ? (
                <span title="Type over it to quote your own price.">
                  <Prov status="derived">Curve-implied</Prov> {quote.seed.curve}, {formatLong(quote.seed.asOf)}
                </span>
              ) : null}>
              <input className="input input--quote" style={{ width: 220 }} value={quote.text}
                aria-invalid={quote.text !== '' && !quoteValid(quote)} onChange={(e) => setQuote({ text: e.target.value })} />
            </Field>
            <Field label="Face amount">
              <input className="input input--num" style={{ width: 150 }} value={face}
                onChange={(e) => dispatch({ type: 'quote', quote: { face: e.target.value.replace(/[^\d.]/g, '') } })} />
            </Field>
            <CurvePicker />
            <div className="grow" />
            <div className="dim" style={{ fontSize: 12, textAlign: 'right' }}>
              {busy || (curve && curve.building) ? <><Spinner /> Calculating</> : r ? 'Calculated' : ''}
              <div>{curve && curve.dataset && <Prov status={curve.dataset.status}>{curve.def.label} curve · {formatLong(curve.dataset.asOf)}</Prov>}</div>
            </div>
          </div>
        </div>
      </section>

      {curve && curve.error && <EngineError error={curve.error} context="Curve" />}
      {fatalOrError && <EngineError error={fatalOrError} context="Quote" />}
      {callable && (
        <div className="note note--warn" style={{ marginBottom: 16 }}>
          This bond is callable. Straight figures below are to maturity with the calls removed; the workout table
          prices every legal path, and OAS values the option.
        </div>
      )}

      {r && (
        <>
          <div className="panel" style={{ marginBottom: 16 }}>
            <div className="figures">
              <Figure label="Clean price" value={fmt(r.clean, 6)} sub={usd32 ? formatThirtySeconds(r.clean) : `Dirty ${fmt(r.dirty, 6)}`} />
              <Figure label="Yield to maturity" value={pct(r.ytm, 4)} unit="%" sub="ICMA, street-equivalent" />
              <Figure label={curve && curve.def.role === 'discount' ? 'Z-spread to swaps' : 'Z-spread'}
                value={r.zspread !== undefined ? signedBp(r.zspread, 1) : '—'} unit="bp"
                sub={curve ? `over ${curve.def.label}` : ''} />
              {usd && <Figure label="G-spread" value={r.gspread ? signed(r.gspread.bp, 1) : '—'} unit="bp" sub="vs on-the-run Treasuries" />}
              <Figure label="Modified duration" value={r.risk ? fmt(r.risk.modified, 3) : '—'} sub={r.risk ? `Convexity ${fmt(r.risk.convexity, 2)}` : ''} />
              <Figure label={`DV01 for ${money(face, 0)}`} value={r.risk ? fmt(r.risk.dv01, 2) : '—'} sub={r.risk ? `${fmt(r.risk.dv01Per100, 5)} per 100` : ''} />
            </div>
          </div>

          <div className="cols cols--3" style={{ marginBottom: 16 }}>
            <Panel title="Price">
              <KV rows={[
                ['Clean price', fmt(r.clean, 6)],
                usd32 && ['In 32nds', formatThirtySeconds(r.clean)],
                ['Accrued interest', fmt(r.accrued, 6), r.accruedDays !== undefined ? `${r.accruedDays} days since ${formatLong(r.prevCoupon)}` : null],
                ['Dirty price', fmt(r.dirty, 6)],
                ['Next coupon', r.nextCoupon ? formatLong(r.nextCoupon) : '—'],
                ['Invoice for face', r.invoice ? money(r.invoice.total, 2) : '—', `${money(face, 0)} ${sec.currency} face, dirty × face / 100`],
                ['ICMA consideration', r.consideration ? money(r.consideration.value, 2) : '—', 'engine, same face'],
              ]} />
              <EngineError error={r.errors.invoice} context="Invoice" />
            </Panel>
            <Panel title="Yields" note="annual, %">
              <KV rows={[
                ...r.yields.map((y) => [y.label, Number.isFinite(y.value) ? pct(y.value, 4) : '—']),
                ['Current yield', pct(r.currentYield, 4), 'coupon ÷ clean price'],
                ['Simple yield', pct(r.simpleYield, 4), 'undiscounted, Act/365 years'],
              ]} />
            </Panel>
            <Panel title="Spreads and risk">
              <KV rows={[
                ['Z-spread', r.zspread !== undefined ? `${signedBp(r.zspread, 2)} bp` : '—', curve ? `constant spread over ${curve.key.id}` : null],
                usd && ['G-spread', r.gspread ? `${signed(r.gspread.bp, 2)} bp` : '—',
                  r.gspread ? `vs interpolated benchmark yield ${pct(r.gspread.benchYield, 4)}%` : null],
                ['Spread duration', r.spreadDuration !== undefined ? fmt(r.spreadDuration, 3) : '—', 'from Z ± 1 bp'],
                ['CS01 for face', r.cs01 !== undefined ? fmt(r.cs01, 2) : '—'],
                ['Modified duration', r.risk ? fmt(r.risk.modified, 4) : '—', 'from ICMA yield ± 1 bp'],
                ['Macaulay duration', r.risk ? fmt(r.risk.macaulay, 4) : '—'],
                ['Convexity', r.risk ? fmt(r.risk.convexity, 3) : '—'],
              ]} />
              <EngineError error={r.errors.zspread} context="Z-spread" />
              {usd && <EngineError error={r.errors.gspread} context="G-spread" />}
              <EngineError error={r.errors.risk} context="Risk" />
            </Panel>
          </div>

          {callable && (
            <Panel title="Workouts" note="dated cash yield to every legal redemption path" style={{ marginBottom: 16 }} flush>
              {workout.data && workout.data.ok ? (
                <WorkoutTable rows={workoutRows(workout.data.data)} />
              ) : workout.data && !workout.data.ok ? (
                <div className="panel__body"><EngineError error={workout.data.error} context="Workout" /></div>
              ) : <div className="empty"><Spinner /></div>}
            </Panel>
          )}

          <div className="cols cols--wide" style={{ marginBottom: 16 }}>
            <Panel title="Price and yield" note="engine prices at shifted ICMA yields">
              {r.ladder ? (
                <LineChart height={280}
                  series={[{ id: 'py', label: 'Dirty price', color: '--s2',
                    points: r.ladder.map((l) => ({ x: l.yield * 100, y: l.dirty })),
                    markers: [{ x: r.ytm * 100, y: r.dirty }] }]}
                  xFormat={(v) => `${v.toFixed(2)}%`} yFormat={(v) => v.toFixed(1)}
                  tipX={(v) => `Yield ${v.toFixed(3)}%`} tipY={(v) => fmt(v, 4)}
                  refs={[{ x: r.ytm * 100, label: 'Quote' }]} />
              ) : <Empty>Risk ladder unavailable.</Empty>}
            </Panel>
            <Panel title="Sensitivity ladder" flush>
              {r.ladder && (
                <div className="table-wrap">
                  <table className="grid ladder">
                    <thead><tr><th className="num">Shift</th><th className="num">Yield %</th><th className="num">Clean</th><th className="num">P&amp;L for face</th></tr></thead>
                    <tbody>
                      {r.ladder.map((l) => (
                        <tr key={l.bp} className={l.bp === 0 ? 'is-active' : ''}>
                          <td className="num">{l.bp === 0 ? 'quote' : `${l.bp > 0 ? '+' : ''}${l.bp} bp`}</td>
                          <td className="num">{pct(l.yield, 4)}</td>
                          <td className="num">{fmt(l.clean, 4)}</td>
                          <td className={`num ${l.change > 0 ? 'pos' : l.change < 0 ? 'neg' : ''}`}>{signed((l.change * Number(face)) / 100, 0)}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
            </Panel>
          </div>

          {usd && r.gspread && (
            <Panel title="G-spread benchmarks" flush
              note="actual on-the-run securities; type a price to replace the curve-implied one"
              foot={`Common dated-cash convention: ${dayCountLabel(r.gspread.convention.day_count)}, ${r.gspread.convention.compounding}. Benchmark yield interpolated linearly by maturity. Prices you enter are kept in this browser.`}>
              <div className="table-wrap">
                <table className="grid">
                  <thead><tr><th>Bucket</th><th>Security</th><th>CUSIP</th><th className="num">Coupon</th><th>Maturity</th><th className="num">Clean price</th><th className="num">Weight</th><th>Quote source</th></tr></thead>
                  <tbody>
                    {r.gspread.benchmarks.map((b, i) => {
                      const s = r.gspread.selected || {};
                      const w = i === s.lower_index ? s.lower_weight : i === s.upper_index ? s.upper_weight : null;
                      return (
                        <tr key={b.sec.cusip} className={w ? 'is-active' : ''}>
                          <td>{b.bucket}Y</td><td>{b.sec.ticker}</td><td className="dim">{b.sec.cusip}</td>
                          <td className="num">{b.sec.coupon}</td><td>{formatLong(b.sec.maturity)}</td>
                          <td className="edit">
                            <BenchPrice cusip={b.sec.cusip} own={benchPrices[b.sec.cusip]} implied={Number(b.quote.value)}
                              onSet={(text) => dispatch({ type: 'benchPrice', cusip: b.sec.cusip, text })} />
                          </td>
                          <td className="num">{w !== null && w !== undefined ? fmt(w, 4) : ''}</td>
                          <td>
                            {b.provenance === 'manual' ? (
                              <span className="row" style={{ gap: 6 }}>
                                <Prov status="manual">Yours</Prov>
                                <button type="button" className="btn btn--ghost btn--sm" onClick={() => dispatch({ type: 'benchPrice', cusip: b.sec.cusip, text: '' })}>Use curve</button>
                              </span>
                            ) : <Prov status="derived">Curve-implied</Prov>}
                          </td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              </div>
            </Panel>
          )}
        </>
      )}
      {!r && !fatalOrError && (
        <Empty>{curve && curve.building ? 'Building the curve…' : seedable ? 'Pricing off the curve…' : 'Enter a quote to analyse this security.'}</Empty>
      )}
    </div>
  );
}

/** Benchmark price cell: shows your price or the curve-implied one, commits on blur or Enter. */
function BenchPrice({ cusip, own, implied, onSet }) {
  const [draft, setDraft] = useState(null);
  const shown = draft ?? own ?? fmt(implied, 4).replace(/,/g, '');
  const commit = (text) => {
    setDraft(null);
    const t = text.trim();
    if (!t) return onSet('');
    if (own === undefined && t === fmt(implied, 4).replace(/,/g, '')) return undefined;
    try {
      if (parsePrice(t) > 0) onSet(t);
    } catch {
      /* not a price: keep the previous one */
    }
    return undefined;
  };
  let invalid = false;
  if (draft !== null && draft.trim()) {
    try {
      invalid = !(parsePrice(draft) > 0);
    } catch {
      invalid = true;
    }
  }
  return (
    <input value={shown} aria-label={`Clean price for ${cusip}`} aria-invalid={invalid}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={(e) => draft !== null && commit(e.target.value)}
      onKeyDown={(e) => e.key === 'Enter' && e.currentTarget.blur()} />
  );
}

function WorkoutTable({ rows }) {
  return (
    <div className="table-wrap">
      <table className="grid">
        <thead><tr><th>Path</th><th>Date</th><th className="num">Yield %</th><th className="num">Clean</th><th className="num">Redemption cash</th><th /></tr></thead>
        <tbody>
          {rows.paths.map((p) => (
            <tr key={p.index} className={rows.worst && p.index === rows.worst.index ? 'is-active' : ''}>
              <td>{p.kind === 'maturity' ? 'Maturity' : `Call ${p.provision !== null ? `#${p.provision + 1}` : ''}`}</td>
              <td>{p.date ? formatLong(p.date) : '—'}</td>
              <td className="num">{pct(p.decimalYield, 4)}</td>
              <td className="num">{fmt(p.cleanPer100, 4)}</td>
              <td className="num">{p.redemptionCash ? fmt(Number(p.redemptionCash.value ?? p.redemptionCash), 4) : '—'}</td>
              <td>{rows.worst && p.index === rows.worst.index ? <b>Yield to worst</b> : ''}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

export { WorkoutTable };
