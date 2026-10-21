import React, { createContext, useContext, useEffect, useMemo, useReducer } from 'react';

// Application state: market datasets (with provenance), the security
// universe, the selected security and screen, and UI preferences. Your data
// (edited datasets, curves and securities you supply, benchmark prices) persists
// per browser as a convenience, and can be exported and imported as JSON.

const STORAGE_KEY = 'convex-terminal:v1';

function load() {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    return raw ? JSON.parse(raw) : {};
  } catch {
    return {};
  }
}

function save(state) {
  try {
    const keep = {
      theme: state.theme,
      screen: state.screen,
      // Only hand edits persist; live data is refetched rather than replayed as "live".
      datasets: Object.fromEntries(Object.entries(state.datasets).filter(([, d]) => d && d.status === 'manual')),
      custom: state.custom,
      customCurves: state.customCurves,
      credit: state.credit,
      spreadCurve: state.spreadCurve,
      benchPrices: state.benchPrices,
      benchmarkChoice: state.benchmarkChoice,
      selectedId: state.selectedId,
      proxy: state.proxy,
      tradeDate: state.tradeDateOverride ? state.tradeDate : undefined,
    };
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(keep));
  } catch {
    /* storage unavailable: preferences simply don't persist */
  }
}

function reducer(state, action) {
  switch (action.type) {
    case 'screen':
      return { ...state, screen: action.screen };
    case 'theme':
      return { ...state, theme: action.theme };
    case 'inspector':
      return { ...state, inspector: action.open ?? !state.inspector };
    case 'dataset':
      return {
        ...state,
        datasets: { ...state.datasets, [action.dataset.id]: action.dataset },
        loading: { ...state.loading, [action.dataset.id]: false },
        errors: { ...state.errors, [action.dataset.id]: null },
      };
    case 'loading':
      return { ...state, loading: { ...state.loading, [action.id]: action.value } };
    case 'loadError':
      return {
        ...state,
        loading: { ...state.loading, [action.id]: false },
        errors: { ...state.errors, [action.id]: action.message },
      };
    case 'select':
      return { ...state, selectedId: action.id };
    case 'custom': {
      const custom = { ...state.custom, [action.security.id]: action.security };
      return { ...state, custom, selectedId: action.select ? action.security.id : state.selectedId };
    }
    case 'removeCustom': {
      const custom = { ...state.custom };
      delete custom[action.id];
      return { ...state, custom };
    }
    case 'customCurve':
      return { ...state, customCurves: { ...state.customCurves, [action.curve.id]: action.curve } };
    case 'removeCustomCurve': {
      const customCurves = { ...state.customCurves };
      delete customCurves[action.id];
      const spreadCurve = Object.fromEntries(Object.entries(state.spreadCurve).filter(([, id]) => id !== action.id));
      return { ...state, customCurves, spreadCurve };
    }
    case 'benchmarkChoice':
      return { ...state, benchmarkChoice: { ...state.benchmarkChoice, ...action.choice } };
    case 'creditCurve':
      return { ...state, credit: { ...state.credit, [action.curve.id]: action.curve } };
    case 'removeCreditCurve': {
      const credit = { ...state.credit };
      delete credit[action.id];
      return { ...state, credit };
    }
    case 'spreadCurve':
      return { ...state, spreadCurve: { ...state.spreadCurve, [action.currency]: action.id } };
    case 'benchPrice': {
      const benchPrices = { ...state.benchPrices };
      if (action.text) benchPrices[action.cusip] = action.text;
      else delete benchPrices[action.cusip];
      return { ...state, benchPrices };
    }
    case 'importData': {
      const d = action.data;
      return {
        ...state,
        datasets: { ...state.datasets, ...(d.datasets || {}) },
        custom: { ...state.custom, ...(d.custom || {}) },
        customCurves: { ...state.customCurves, ...(d.customCurves || {}) },
        credit: { ...state.credit, ...(d.credit || {}) },
        benchPrices: { ...state.benchPrices, ...(d.benchPrices || {}) },
        spreadCurve: { ...state.spreadCurve, ...(d.spreadCurve || {}) },
      };
    }
    case 'tradeDate':
      return { ...state, tradeDate: action.date, tradeDateOverride: !!action.override };
    case 'proxy':
      return { ...state, proxy: action.proxy };
    case 'consoleRequest':
      return { ...state, consoleRequest: action.request, screen: 'API' };
    case 'quote':
      return { ...state, quote: { ...state.quote, ...action.quote } };
    default:
      return state;
  }
}

const StoreContext = createContext(null);

export function StoreProvider({ initial, children }) {
  const [state, dispatch] = useReducer(reducer, null, () => {
    const saved = load();
    return {
      theme: 'system',
      screen: 'YAS',
      inspector: false,
      datasets: {},
      loading: {},
      errors: {},
      custom: {},
      customCurves: {},
      credit: {},
      spreadCurve: {},
      benchPrices: {},
      // G-spread benchmarks: the on-the-run Treasuries, or securities you pick;
      // interpolated by maturity or the one maturing the same day.
      benchmarkChoice: { mode: 'otr', ids: [], selection: 'LinearMaturity' },
      selectedId: null,
      proxy: '',
      tradeDate: null,
      tradeDateOverride: false,
      quote: { kind: 'price', text: '', face: '1000000' },
      consoleRequest: null,
      ...initial,
      ...saved,
      datasets: { ...(initial && initial.datasets), ...(saved.datasets || {}) },
      tradeDateOverride: !!saved.tradeDate,
    };
  });

  useEffect(() => save(state), [state]);

  useEffect(() => {
    const el = document.documentElement;
    if (state.theme === 'system') el.removeAttribute('data-theme');
    else el.setAttribute('data-theme', state.theme);
  }, [state.theme]);

  const value = useMemo(() => ({ state, dispatch }), [state]);
  return <StoreContext.Provider value={value}>{children}</StoreContext.Provider>;
}

export function useStore() {
  const ctx = useContext(StoreContext);
  if (!ctx) throw new Error('useStore outside StoreProvider');
  return ctx;
}
