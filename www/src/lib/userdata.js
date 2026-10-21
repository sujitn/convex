// Everything you supplied (edited datasets, your curves and securities,
// benchmark prices) as one JSON document, and the checked way back in.

import { OIS_INDICES } from './ois.js';
import { CALENDAR_RULES } from './calendars.js';

/** Everything you supplied, as one JSON document (the inverse of the importData action). */
export function exportData(state) {
  return {
    kind: 'convex-terminal.data',
    version: 1,
    exportedAt: new Date().toISOString(),
    datasets: Object.fromEntries(Object.entries(state.datasets).filter(([, d]) => d && d.status === 'manual')),
    custom: state.custom,
    customCurves: state.customCurves,
    benchPrices: state.benchPrices,
  };
}

/** Check an imported document's shape; returns the subset to import or throws. */
export function readImport(json) {
  if (!json || json.kind !== 'convex-terminal.data') throw new Error('Not a Convex Terminal data file.');
  const obj = (v) => (v && typeof v === 'object' && !Array.isArray(v) ? v : {});
  const datasets = Object.fromEntries(Object.entries(obj(json.datasets))
    .filter(([id, d]) => d && d.id === id && Array.isArray(d.points)).map(([id, d]) => [id, { ...d, status: 'manual' }]));
  const customCurves = Object.fromEntries(Object.entries(obj(json.customCurves))
    .filter(([id, c]) => c && c.id === id && c.custom && Array.isArray(c.points) && /^\d{4}-\d{2}-\d{2}$/.test(c.asOf)
      && (c.method === 'ois' ? !!OIS_INDICES[c.index] : (c.method === 'par' || c.method === 'zero') && !!CALENDAR_RULES[c.calendar])));
  const custom = Object.fromEntries(Object.entries(obj(json.custom)).filter(([id, c]) => c && c.id === id));
  const benchPrices = Object.fromEntries(Object.entries(obj(json.benchPrices)).filter(([, t]) => typeof t === 'string'));
  return { datasets, customCurves, custom, benchPrices };
}
