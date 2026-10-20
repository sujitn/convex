import React, { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from 'react';
import { useStore } from './store';
import { SNAPSHOT } from '../market/snapshot';
import { loadSource, SOURCES } from '../market/feeds';
import { buildCurve, CURVE_DEFS } from '../analytics/curves';
import { treasuryUniverse, onTheRun, EXAMPLES, curveForCurrency } from '../analytics/securities';
import { settlementFor } from '../lib/terms';
import { todayIso } from '../lib/dates';

// Derived workspace: market datasets (snapshot → live → edited), curves built
// from them by the engine, the security universe and the selected security.

const WorkspaceContext = createContext(null);

export function WorkspaceProvider({ engine, children }) {
  const { state, dispatch } = useStore();
  const [curves, setCurves] = useState({});
  const [curveOptions, setCurveOptions] = useState({});
  const building = useRef({});

  const datasets = useMemo(() => {
    const out = { ...SNAPSHOT.datasets };
    for (const [id, d] of Object.entries(state.datasets)) if (d) out[id] = d;
    return out;
  }, [state.datasets]);

  const loadOne = useCallback(
    async (id) => {
      dispatch({ type: 'loading', id, value: true });
      try {
        const ds = await loadSource(id, { proxy: state.proxy || null });
        dispatch({ type: 'dataset', dataset: ds });
        return ds;
      } catch (error) {
        dispatch({ type: 'loadError', id, message: error.message });
        return null;
      }
    },
    [dispatch, state.proxy],
  );

  const loadAll = useCallback(
    () => Promise.all(SOURCES.filter((s) => !s.corsRequired || state.proxy).map((s) => loadOne(s.id))),
    [loadOne, state.proxy],
  );

  const editDataset = useCallback(
    (id, points) => {
      const base = datasets[id];
      dispatch({
        type: 'dataset',
        dataset: { ...base, points, status: 'manual', editedAt: new Date().toISOString(), revision: Date.now() % 100000 },
      });
    },
    [datasets, dispatch],
  );

  const resetDataset = useCallback(
    (id) => dispatch({ type: 'dataset', dataset: SNAPSHOT.datasets[id] }),
    [dispatch],
  );

  // Rebuild a curve whenever its dataset (or chosen interpolation) changes.
  useEffect(() => {
    for (const [cid, def] of Object.entries(CURVE_DEFS)) {
      const ds = datasets[def.dataset];
      if (!ds) continue;
      const interpolation = curveOptions[cid];
      const sig = `${ds.asOf}|${ds.status}|${ds.revision || ''}|${ds.fetchedAt || ''}|${interpolation || ''}`;
      if (building.current[cid] === sig) continue;
      building.current[cid] = sig;
      setCurves((c) => ({ ...c, [cid]: { ...(c[cid] || {}), id: cid, def, building: true } }));
      buildCurve(engine, cid, ds, { interpolation }).then((curve) => {
        if (building.current[cid] !== sig) return;
        setCurves((c) => ({ ...c, [cid]: { ...curve, building: false } }));
      });
    }
  }, [datasets, engine, curveOptions]);

  const treasuries = useMemo(() => treasuryUniverse(datasets['ust-auctions']), [datasets]);
  const benchmarks = useMemo(() => onTheRun(treasuries), [treasuries]);
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
  const curveId = selected ? curveForCurrency(selected.currency) : 'USD.UST';
  const curve = curves[curveId] || null;

  const ust = datasets['ust-par'];
  const tradeDate = state.tradeDateOverride && state.tradeDate ? state.tradeDate : (ust && ust.asOf) || todayIso();
  const settlement = selected ? settlementFor(selected, tradeDate) : tradeDate;

  const value = useMemo(
    () => ({
      engine,
      datasets,
      curves,
      curve,
      curveId,
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
    [engine, datasets, curves, curve, curveId, curveOptions, loadOne, loadAll, editDataset, resetDataset,
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
 * dropped. Returns { data, error, busy }.
 */
export function useComputation(fn, deps) {
  const [out, setOut] = useState({ data: null, error: null, busy: false });
  const seq = useRef(0);
  useEffect(() => {
    const n = ++seq.current;
    setOut((o) => ({ ...o, busy: true }));
    Promise.resolve()
      .then(fn)
      .then(
        (data) => n === seq.current && setOut({ data, error: null, busy: false }),
        (error) => n === seq.current && setOut({ data: null, error: { code: 'app', message: error.message }, busy: false }),
      );
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
  return out;
}
