import React, { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from 'react';
import { useStore } from './store';
import { SNAPSHOT } from '../market/snapshot';
import { loadSource, SOURCES } from '../market/feeds';
import { buildCurve, CURVE_DEFS } from '../analytics/curves';
import { treasuryUniverse, onTheRun, EXAMPLES, curveForCurrency } from '../analytics/securities';
import { settlementFor } from '../lib/terms';
import { todayIso, yearsBetween } from '../lib/dates';

// Derived workspace: market datasets (snapshot → live → edited), curves built
// from them and from the curves you supply, the security universe, the selected
// security, and the curve each currency is spread against.

const WorkspaceContext = createContext(null);

export function WorkspaceProvider({ engine, children }) {
  const { state, dispatch } = useStore();
  const [curves, setCurves] = useState({});
  const [curveOptions, setCurveOptions] = useState({});
  const building = useRef({});
  // Bumped on every edit or reset so a live response that started earlier is dropped.
  const editGen = useRef({});
  const datasetsRef = useRef({});

  const datasets = useMemo(() => {
    const out = { ...SNAPSHOT.datasets };
    for (const [id, d] of Object.entries(state.datasets)) if (d) out[id] = d;
    return out;
  }, [state.datasets]);

  datasetsRef.current = datasets;

  const loadOne = useCallback(
    async (id) => {
      const gen = editGen.current[id] || 0;
      dispatch({ type: 'loading', id, value: true });
      try {
        const ds = await loadSource(id, { proxy: state.proxy || null });
        if ((editGen.current[id] || 0) !== gen) {
          // Edited while the fetch was in flight: keep the edit.
          dispatch({ type: 'loading', id, value: false });
          return null;
        }
        dispatch({ type: 'dataset', dataset: ds });
        return ds;
      } catch (error) {
        dispatch({ type: 'loadError', id, message: error.message });
        return null;
      }
    },
    [dispatch, state.proxy],
  );

  // The automatic startup load leaves your edits alone; the Market button passes
  // keepManual: false to replace them on purpose.
  const loadAll = useCallback(
    ({ keepManual = true } = {}) => Promise.all(
      SOURCES
        .filter((s) => !s.corsRequired || state.proxy)
        .filter((s) => !(keepManual && datasetsRef.current[s.id] && datasetsRef.current[s.id].status === 'manual'))
        .map((s) => loadOne(s.id)),
    ),
    [loadOne, state.proxy],
  );

  const editDataset = useCallback(
    (id, points) => {
      const base = datasets[id];
      editGen.current[id] = (editGen.current[id] || 0) + 1;
      dispatch({
        type: 'dataset',
        dataset: { ...base, points, status: 'manual', editedAt: new Date().toISOString(), revision: Date.now() % 100000 },
      });
    },
    [datasets, dispatch],
  );

  const resetDataset = useCallback(
    (id) => {
      editGen.current[id] = (editGen.current[id] || 0) + 1;
      dispatch({ type: 'dataset', dataset: SNAPSHOT.datasets[id] });
    },
    [dispatch],
  );

  // Built-in curves read their dataset; a curve you supply carries its own points.
  const curveDefs = useMemo(() => ({ ...CURVE_DEFS, ...state.customCurves }), [state.customCurves]);

  const saveCurve = useCallback((curve) => dispatch({ type: 'customCurve', curve }), [dispatch]);
  const removeCurve = useCallback((id) => dispatch({ type: 'removeCustomCurve', id }), [dispatch]);

  // Rebuild a curve whenever its dataset (or chosen interpolation) changes.
  useEffect(() => {
    for (const [cid, def] of Object.entries(curveDefs)) {
      const ds = def.custom ? def : datasets[def.dataset];
      if (!ds) continue;
      const interpolation = curveOptions[cid] || (def.custom ? def.interpolation : undefined);
      const sig = def.custom
        ? `${JSON.stringify(def)}|${interpolation || ''}`
        : `${ds.asOf}|${ds.status}|${ds.revision || ''}|${ds.fetchedAt || ''}|${interpolation || ''}`;
      if (building.current[cid] === sig) continue;
      building.current[cid] = sig;
      setCurves((c) => ({ ...c, [cid]: { ...(c[cid] || {}), id: cid, def, building: true } }));
      buildCurve(engine, def, ds, { interpolation })
        .catch((e) => ({ id: cid, def, recipe: null, error: { code: 'app', message: e.message } }))
        .then((curve) => {
          if (building.current[cid] !== sig) return;
          setCurves((c) => ({ ...c, [cid]: { ...curve, building: false } }));
        });
    }
    // Forget curves whose definitions were removed.
    for (const cid of Object.keys(building.current)) {
      if (!curveDefs[cid]) {
        delete building.current[cid];
        setCurves((c) => {
          const next = { ...c };
          delete next[cid];
          return next;
        });
      }
    }
  }, [datasets, engine, curveOptions, curveDefs]);

  const treasuries = useMemo(() => treasuryUniverse(datasets['ust-auctions']), [datasets]);
  const benchmarks = useMemo(() => onTheRun(treasuries), [treasuries]);
  const benchmarkChoice = state.benchmarkChoice;
  const universe = useMemo(
    () => [...treasuries, ...EXAMPLES.map((e) => state.custom[e.id] || e),
      ...Object.values(state.custom).filter((c) => !EXAMPLES.some((e) => e.id === c.id))],
    [treasuries, state.custom],
  );

  const defaultId = useMemo(() => {
    const ten = benchmarks.find((b) => b.bucket === 10);
    return ten ? ten.sec.id : universe[0] && universe[0].id;
  }, [benchmarks, universe]);

  const selected = universe.find((s) => s.id === state.selectedId) || universe.find((s) => s.id === defaultId) || null;
  // The government curve for the currency (G-spread benchmarks, scenarios) and
  // the curve spreads are quoted against, the government one unless you chose another.
  const currency = selected ? selected.currency : 'USD';
  const govId = curveForCurrency(currency);
  const curveChoices = useMemo(() => Object.values(curveDefs).filter((d) => d.currency === currency), [curveDefs, currency]);
  const chosen = state.spreadCurve[currency];
  const curveId = (chosen && curveDefs[chosen] && chosen) || govId || (curveChoices[0] && curveChoices[0].id) || null;
  const curve = (curveId && curves[curveId]) || null;
  const govCurve = (govId && curves[govId]) || null;
  const setSpreadCurve = useCallback((id) => dispatch({ type: 'spreadCurve', currency, id }), [dispatch, currency]);

  const ust = datasets['ust-par'];
  const tradeDate = state.tradeDateOverride && state.tradeDate ? state.tradeDate : (ust && ust.asOf) || todayIso();
  const settlement = selected ? settlementFor(selected, tradeDate) : tradeDate;

  // G-spread benchmarks in the selected security's currency: the on-the-run
  // Treasuries, or the bullet bonds you picked (any currency you have bonds in).
  // Never the security being analysed, and never callable or sinking-fund bonds.
  const selectedId = selected && selected.id;
  const benchmarkSet = useMemo(() => {
    // The on-the-run Treasuries are a USD set; other currencies use your selection.
    if (benchmarkChoice.mode !== 'custom' && currency === 'USD') return benchmarks.filter((b) => b.sec.id !== selectedId);
    return benchmarkChoice.ids
      .map((id) => universe.find((s) => s.id === id))
      .filter((s) => s && s.id !== selectedId && s.kind === 'fixed' && s.currency === currency
        && !(s.calls || []).length && !(s.sinking || []).length)
      .sort((a, b) => (a.maturity < b.maturity ? -1 : a.maturity > b.maturity ? 1 : 0))
      .map((s) => ({ bucket: Math.round(yearsBetween(tradeDate, s.maturity) * 10) / 10, sec: s }));
  }, [benchmarkChoice, benchmarks, universe, currency, tradeDate, selectedId]);
  const setBenchmarkChoice = useCallback((choice) => dispatch({ type: 'benchmarkChoice', choice }), [dispatch]);

  const value = useMemo(
    () => ({
      engine,
      datasets,
      curves,
      curve,
      curveId,
      govCurve,
      curveDefs,
      curveChoices,
      setSpreadCurve,
      saveCurve,
      removeCurve,
      benchPrices: state.benchPrices,
      benchmarkChoice,
      benchmarkSet,
      setBenchmarkChoice,
      setInterpolation: (cid, interp) => setCurveOptions((o) => ({ ...o, [cid]: interp })),
      curveOptions,
      loadOne,
      loadAll,
      editDataset,
      resetDataset,
      loading: state.loading,
      errors: state.errors,
      treasuries,
      benchmarks,
      universe,
      selected,
      tradeDate,
      settlement,
    }),
    [engine, datasets, curves, curve, curveId, govCurve, curveDefs, curveChoices, setSpreadCurve, saveCurve, removeCurve,
      state.benchPrices, benchmarkChoice, benchmarkSet, setBenchmarkChoice, curveOptions, loadOne, loadAll, editDataset, resetDataset,
      state.loading, state.errors, treasuries, benchmarks, universe, selected, tradeDate, settlement],
  );
  return <WorkspaceContext.Provider value={value}>{children}</WorkspaceContext.Provider>;
}

export function useWorkspace() {
  const ctx = useContext(WorkspaceContext);
  if (!ctx) throw new Error('useWorkspace outside WorkspaceProvider');
  return ctx;
}

/**
 * Run an async engine computation whenever `deps` change; stale results are
 * dropped. Returns { data, error, busy, current }; `current` is false while `data` still
 * belongs to earlier deps (it is kept so the screen doesn't flash empty).
 */
export function useComputation(fn, deps) {
  const [out, setOut] = useState({ data: null, error: null, busy: false, deps: null });
  const seq = useRef(0);
  useEffect(() => {
    const n = ++seq.current;
    setOut((o) => ({ ...o, busy: true }));
    Promise.resolve()
      .then(fn)
      .then(
        (data) => n === seq.current && setOut({ data, error: null, busy: false, deps }),
        (error) => n === seq.current && setOut({ data: null, error: { code: 'app', message: error.message }, busy: false, deps }),
      );
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
  const current = !!out.deps && out.deps.length === deps.length && out.deps.every((d, i) => Object.is(d, deps[i]));
  return { ...out, current };
}
