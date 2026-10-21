import React, { useEffect, useMemo, useRef, useState } from 'react';
import { StoreProvider, useStore } from './state/store';
import { WorkspaceProvider, useWorkspace } from './state/workspace';
import Inspector from './ui/Inspector';
import { Mark } from './ui/Rosette';
import { Icon, Prov } from './ui/primitives';
import { bytes, ms } from './lib/format';
import { formatLong } from './lib/dates';
import Market from './screens/Market';
import Curves from './screens/Curves';
import Security from './screens/Security';
import Yas from './screens/Yas';
import Cashflows from './screens/Cashflows';
import Callable from './screens/Callable';
import Bills from './screens/Bills';
import Frn from './screens/Frn';
import Cds from './screens/Cds';
import Scenarios from './screens/Scenarios';
import Calendars from './screens/Calendars';
import ApiConsole from './screens/ApiConsole';

export const FUNCTIONS = [
  { code: 'MKT', name: 'Market', title: 'Market data', Screen: Market },
  { code: 'CRV', name: 'Curves', title: 'Curve workbench', Screen: Curves },
  { code: 'SEC', name: 'Security', title: 'Security description', Screen: Security },
  { code: 'YAS', name: 'Yield', title: 'Yield and spread analysis', Screen: Yas },
  { code: 'CFL', name: 'Cash', title: 'Cash flows', Screen: Cashflows },
  { code: 'SCN', name: 'Scenario', title: 'Scenarios and key-rate risk', Screen: Scenarios },
  { code: 'OAS', name: 'Callable', title: 'Callable bonds: OAS and workouts', Screen: Callable },
  { code: 'FRN', name: 'Floaters', title: 'Floating-rate notes: discount margin', Screen: Frn },
  { code: 'CDS', name: 'Credit', title: 'Credit default swaps', Screen: Cds },
  { code: 'BILL', name: 'Bills', title: 'Treasury bills', Screen: Bills },
  { code: 'CAL', name: 'Calendars', title: 'Holiday calendars', Screen: Calendars, sep: true },
  { code: 'API', name: 'API', title: 'Library console', Screen: ApiConsole },
];

function CommandLine() {
  const { dispatch } = useStore();
  const { universe } = useWorkspace();
  const [text, setText] = useState('');
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const input = useRef(null);

  useEffect(() => {
    const onKey = (e) => {
      const tag = document.activeElement && document.activeElement.tagName;
      if (e.key === '/' && !/INPUT|TEXTAREA|SELECT/.test(tag || '')) {
        e.preventDefault();
        if (input.current) input.current.focus();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  const items = useMemo(() => {
    const q = text.trim().toUpperCase();
    const words = q.split(/\s+/).filter(Boolean);
    const lead = FUNCTIONS.find((f) => f.code === words[0]);
    const term = (lead ? words.slice(1) : words).join(' ');
    const fns = FUNCTIONS.filter((f) => !q || f.code.startsWith(words[0] || '') || f.title.toUpperCase().includes(q))
      .map((f) => ({ kind: 'fn', key: f.code, code: f.code, label: f.title }));
    if (!term) return fns;
    const secs = universe
      .filter((s) => [s.cusip, s.isin, s.ticker, s.name].filter(Boolean).some((v) => v.toUpperCase().includes(term)))
      .slice(0, 8)
      .map((s) => ({ kind: 'sec', key: s.id, code: s.cusip || 'EX', label: s.ticker || s.name, sec: s, fn: lead }));
    return [...secs, ...fns.slice(0, 3)];
  }, [text, universe]);

  const choose = (item) => {
    if (!item) return;
    if (item.kind === 'fn') dispatch({ type: 'screen', screen: item.code });
    else {
      dispatch({ type: 'select', id: item.sec.id });
      dispatch({ type: 'screen', screen: item.fn ? item.fn.code : 'YAS' });
    }
    setText('');
    setOpen(false);
    if (input.current) input.current.blur();
  };

  return (
    <div className="cmd">
      <span className="cmd__prompt" aria-hidden="true">&gt;</span>
      <input
        ref={input}
        value={text}
        placeholder="Function or security, e.g. YAS 91282CRF0"
        aria-label="Command line. Press slash to focus."
        role="combobox"
        aria-expanded={open}
        aria-controls="cmd-menu"
        aria-activedescendant={open && items.length > 0 && items[active] ? `cmd-opt-${active}` : undefined}
        onChange={(e) => {
          setText(e.target.value);
          setOpen(true);
          setActive(0);
        }}
        onFocus={() => setOpen(true)}
        onBlur={() => setTimeout(() => setOpen(false), 120)}
        onKeyDown={(e) => {
          if (e.key === 'ArrowDown') setActive((a) => Math.min(items.length - 1, a + 1));
          else if (e.key === 'ArrowUp') setActive((a) => Math.max(0, a - 1));
          else if (e.key === 'Enter') choose(items[active]);
          else if (e.key === 'Escape') {
            setOpen(false);
            e.currentTarget.blur();
          }
        }}
      />
      {open && items.length > 0 && (
        <div className="cmd__menu" id="cmd-menu" role="listbox">
          {items.map((it, i) => (
            <div key={`${it.kind}${it.key}`} id={`cmd-opt-${i}`} className="cmd__item" role="option" aria-selected={i === active}
              onMouseDown={(e) => {
                e.preventDefault();
                choose(it);
              }}>
              <span className="cmd__code">{it.code}</span>
              <span>{it.label}</span>
              <span className="cmd__hint">{it.kind === 'sec' ? (it.fn ? it.fn.code : 'YAS') : ''}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function MarketChip() {
  const { datasets, curves, loading } = useWorkspace();
  const ust = datasets['ust-par'];
  const c = curves['USD.UST'];
  if (!ust) return null;
  const status = c && c.error ? 'error' : ust.status;
  return (
    <span className="chip hide-sm" title={`US Treasury par curve · ${ust.source ? ust.source.name : ''}`}>
      {loading['ust-par'] ? <span className="spinner" aria-label="Loading" /> : <Prov status={status}>UST</Prov>}
      <b>{formatLong(ust.asOf)}</b>
    </span>
  );
}

function StatusBar({ engine }) {
  const { tradeDate, settlement, selected, curve } = useWorkspace();
  const [status, setStatus] = useState(engine.getStatus());
  const [last, setLast] = useState(engine.getLog()[0]);
  useEffect(() => engine.onStatus(setStatus), [engine]);
  useEffect(() => engine.onLog((l) => setLast(l[0])), [engine]);
  return (
    <footer className="statusbar">
      <span>
        Engine <b>{status.state}</b>
        {status.bytes ? ` · ${bytes(status.bytes)}` : ''}
      </span>
      {status.restarts ? <span className="neg">Restarted after an engine error ×{status.restarts}</span> : null}
      <span>Trade <b>{formatLong(tradeDate)}</b></span>
      {selected && <span>Settle <b>{formatLong(settlement)}</b></span>}
      {curve && curve.key && <span>Curve <b>{curve.key.id}</b>{curve.building ? ' (building)' : ''}</span>}
      <span className="spacer" />
      {last && (
        <span>
          Last call <b>{last.op}{last.count > 1 ? ` ×${last.count}` : ''}</b> {ms(last.ms)}
          {last.ok ? '' : ' · failed'}
        </span>
      )}
    </footer>
  );
}

function Shell({ engine }) {
  const { state, dispatch } = useStore();
  const { loadAll } = useWorkspace();
  const fn = FUNCTIONS.find((f) => f.code === state.screen) || FUNCTIONS.find((f) => f.code === 'YAS');
  const { Screen } = fn;

  useEffect(() => {
    loadAll();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    document.title = `${fn.code} · Convex Terminal`;
  }, [fn]);

  const prefersDark = typeof window !== 'undefined' && window.matchMedia
    && window.matchMedia('(prefers-color-scheme: dark)').matches;
  const dark = state.theme === 'dark' || (state.theme === 'system' && prefersDark !== false);

  return (
    <div className="shell">
      <header className="topbar">
        <div className="brand" title="Convex Terminal"><Mark /></div>
        <span className="brand-name hide-sm">Convex <span>Terminal</span></span>
        <CommandLine />
        <div className="topbar__right">
          <MarketChip />
          <button type="button" className="icon-btn" aria-label={dark ? 'Switch to light theme' : 'Switch to dark theme'}
            title={dark ? 'Light theme' : 'Dark theme'} onClick={() => dispatch({ type: 'theme', theme: dark ? 'light' : 'dark' })}>
            <Icon name={dark ? 'sun' : 'moon'} />
          </button>
          <button type="button" className="icon-btn" aria-pressed={state.inspector} aria-label="Show library calls"
            title="Library calls" onClick={() => dispatch({ type: 'inspector' })}>
            <Icon name="code" />
          </button>
        </div>
      </header>
      <nav className="rail" aria-label="Functions">
        {FUNCTIONS.map((f) => (
          <React.Fragment key={f.code}>
            {f.sep && <div className="rail__sep" />}
            <button type="button" aria-current={f.code === fn.code ? 'page' : undefined} title={f.title}
              onClick={() => dispatch({ type: 'screen', screen: f.code })}>
              <span className="rail__code">{f.code}</span>
              <span className="rail__name">{f.name}</span>
            </button>
          </React.Fragment>
        ))}
      </nav>
      <main className="main" id="main">
        <Screen fn={fn} />
      </main>
      {state.inspector && (
        <Inspector engine={engine} onClose={() => dispatch({ type: 'inspector', open: false })}
          onOpenInConsole={(request) => dispatch({ type: 'consoleRequest', request })} />
      )}
      <StatusBar engine={engine} />
    </div>
  );
}

export default function App({ engine }) {
  return (
    <StoreProvider>
      <WorkspaceProvider engine={engine}>
        <Shell engine={engine} />
      </WorkspaceProvider>
    </StoreProvider>
  );
}
