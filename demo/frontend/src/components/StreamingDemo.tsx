// =============================================================================
// Streaming Demo Panel
// Live runtime valuations streamed over the WebSocket
// =============================================================================

import { Wifi, WifiOff, Activity, AlertTriangle, Radio } from 'lucide-react';
import type { MeasureResult, ResultEnvelope } from '../lib/api';
import {
  useValuationStream,
  measureNumber,
  type StreamedValuation,
} from '../hooks/useValuationStream';

// Sparkline component for price history
function Sparkline({ data, color = '#10b981', height = 24 }: { data: number[]; color?: string; height?: number }) {
  if (data.length < 2) return null;

  const min = Math.min(...data);
  const max = Math.max(...data);
  const range = max - min || 1;

  const points = data
    .map((val, i) => {
      const x = (i / (data.length - 1)) * 100;
      const y = height - ((val - min) / range) * height;
      return `${x},${y}`;
    })
    .join(' ');

  return (
    <svg width="100" height={height} className="inline-block">
      <polyline
        points={points}
        fill="none"
        stroke={color}
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

// The runtime measures, in display order
const MEASURES: Array<{ id: string; label: string }> = [
  { id: 'clean_price_mid', label: 'Clean' },
  { id: 'dirty_price_mid', label: 'Dirty' },
  { id: 'accrued_interest', label: 'Accrued' },
  { id: 'ytm_mid', label: 'YTM' },
  { id: 'modified_duration_mid', label: 'Mod Dur' },
  { id: 'convexity_mid', label: 'Convexity' },
  { id: 'dv01_mid', label: 'DV01' },
  { id: 'z_spread_mid', label: 'Z-Spread' },
];

// Format a measure by the unit it carries; an unknown unit shows the raw value.
function formatMeasure(measure: MeasureResult | undefined): string {
  if (!measure) return '-';
  const value = measureNumber(measure.value);
  if (value === undefined) return String(measure.value.value);
  switch (measure.value.unit) {
    case 'price_per_100':
      return value.toFixed(4);
    case 'decimal_rate':
      return `${(value * 100).toFixed(3)}%`;
    case 'bp':
      return `${value.toFixed(1)} bp`;
    case 'years':
      return value.toFixed(3);
    case 'years_squared':
      return value.toFixed(2);
    default:
      return `${value} ${measure.value.unit}`;
  }
}

function statusClass(status: ResultEnvelope['status']): string {
  if (status === 'Complete') return 'bg-green-100 text-green-700';
  if (status === 'Partial') return 'bg-amber-100 text-amber-700';
  return 'bg-red-100 text-red-700';
}

function ValuationRow({ item }: { item: StreamedValuation }) {
  const { valuation } = item;
  // After a failed refresh the runtime keeps the last valid result, for display only.
  const showLastGood = valuation.result.status === 'Failed' && valuation.last_good !== null;
  const shown = showLastGood && valuation.last_good ? valuation.last_good : valuation.result;
  const history = item.priceHistory;

  return (
    <>
      <tr className="border-b border-gray-100">
        <td className="px-3 py-2 font-mono text-sm font-medium whitespace-nowrap">{item.instrumentId}</td>
        <td className="px-3 py-2 text-sm whitespace-nowrap">
          <span className={`px-2 py-0.5 rounded-full text-xs font-medium ${statusClass(valuation.result.status)}`}>
            {valuation.result.status}
          </span>
          <span className="ml-1 text-xs text-gray-500">{valuation.result.quality}</span>
        </td>
        {MEASURES.map((m) => (
          <td
            key={m.id}
            className={`px-3 py-2 text-right font-mono text-sm whitespace-nowrap ${showLastGood ? 'text-gray-400' : ''}`}
          >
            {formatMeasure(shown.results[m.id])}
          </td>
        ))}
        <td className="px-3 py-2 text-right font-mono text-xs text-gray-500 whitespace-nowrap">
          {valuation.revision}
        </td>
        <td className="px-3 py-2 text-right font-mono text-xs text-gray-500 whitespace-nowrap">
          {new Date(valuation.result.provenance.calculated_at).toLocaleTimeString()}
        </td>
        <td className="px-3 py-2">
          <Sparkline
            data={history}
            color={history.length > 1 && history[history.length - 1] >= history[0] ? '#10b981' : '#ef4444'}
          />
        </td>
      </tr>
      {(valuation.result.errors.length > 0 || showLastGood) && (
        <tr className="border-b border-gray-100 bg-red-50">
          <td colSpan={MEASURES.length + 5} className="px-3 py-2 text-xs text-red-700">
            {showLastGood && (
              <span className="mr-3">
                Showing the last good result ({valuation.retained_quality ?? 'retained'}).
              </span>
            )}
            {valuation.result.errors.map((e, i) => (
              <span key={i} className="mr-3">
                <code>{e.code}</code>
                {e.field ? ` ${e.field}` : ''}: {e.message}
              </span>
            ))}
          </td>
        </tr>
      )}
    </>
  );
}

export function StreamingDemo() {
  const {
    valuations,
    health,
    loadError,
    demoLabelled,
    connectionState,
    sessionId,
    latency,
    updateCount,
    lastUpdate,
    start,
    stop,
  } = useValuationStream();
  const connected = connectionState === 'connected';
  const demo = demoLabelled || valuations.some((v) => v.valuation.result.quality === 'Demo');

  return (
    <div className="p-6 space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-2xl font-bold text-gray-900">Live Valuations</h2>
          <p className="text-gray-600">
            Results published by the pricing runtime, streamed over the WebSocket
          </p>
        </div>
        <div className="flex items-center gap-4">
          {demo && (
            <span className="px-2 py-1 rounded-full text-xs font-semibold bg-amber-100 text-amber-800">
              Demo data
            </span>
          )}
          {connected ? (
            <span className="flex items-center gap-1 text-blue-600">
              <Wifi className="h-4 w-4" />
              WebSocket: {connectionState}
            </span>
          ) : (
            <span className="flex items-center gap-1 text-gray-400">
              <WifiOff className="h-4 w-4" />
              WebSocket: {connectionState}
            </span>
          )}
          {connectionState === 'disconnected' ? (
            <button
              onClick={start}
              className="flex items-center gap-2 bg-blue-600 hover:bg-blue-700 text-white px-4 py-2 rounded-lg text-sm"
            >
              <Wifi className="h-4 w-4" />
              Connect
            </button>
          ) : (
            <button
              onClick={stop}
              className="flex items-center gap-2 bg-gray-600 hover:bg-gray-700 text-white px-4 py-2 rounded-lg text-sm"
            >
              <WifiOff className="h-4 w-4" />
              Disconnect
            </button>
          )}
        </div>
      </div>

      {loadError && (
        <div className="bg-red-50 border border-red-200 text-red-700 px-4 py-3 rounded-lg flex items-center gap-2">
          <AlertTriangle className="h-5 w-5" />
          {loadError}
        </div>
      )}

      {/* Stats Bar */}
      <div className="grid grid-cols-2 md:grid-cols-5 gap-4">
        <div className="bg-white rounded-lg shadow-sm border p-4">
          <div className="text-sm text-gray-600">Runtime</div>
          <div className="text-lg font-semibold text-gray-900">
            {health ? `${health.phase}${health.mode ? ` · ${health.mode}` : ''}` : 'unavailable'}
          </div>
        </div>
        <div className="bg-white rounded-lg shadow-sm border p-4">
          <div className="text-sm text-gray-600">Instruments</div>
          <div className="text-2xl font-bold text-gray-900">{valuations.length}</div>
        </div>
        <div className="bg-white rounded-lg shadow-sm border p-4">
          <div className="text-sm text-gray-600">Updates Received</div>
          <div className="text-2xl font-bold text-blue-600">{updateCount}</div>
        </div>
        <div className="bg-white rounded-lg shadow-sm border p-4">
          <div className="text-sm text-gray-600">Last Update</div>
          <div className="text-lg font-mono text-gray-900">
            {lastUpdate ? new Date(lastUpdate).toLocaleTimeString() : '-'}
          </div>
        </div>
        <div className="bg-white rounded-lg shadow-sm border p-4">
          <div className="text-sm text-gray-600">Session / Latency</div>
          <div className="text-sm font-mono text-gray-900">
            {sessionId ?? '-'} / {latency !== null ? `${latency}ms` : '-'}
          </div>
        </div>
      </div>

      {/* Valuations Grid */}
      {valuations.length > 0 ? (
        <div className="bg-white rounded-lg shadow-sm border overflow-hidden">
          <div className="px-6 py-4 border-b bg-blue-50">
            <h3 className="text-lg font-semibold flex items-center gap-2">
              <Radio className="h-5 w-5 text-blue-600" />
              Runtime Results
              <span className="text-xs font-normal text-blue-600 bg-blue-100 px-2 py-1 rounded-full">
                {valuations[0].valuation.runtime_id}
              </span>
            </h3>
          </div>
          <div className="overflow-x-auto">
            <table className="w-full">
              <thead className="bg-blue-50">
                <tr>
                  <th className="px-3 py-3 text-left text-xs font-medium text-blue-700 uppercase tracking-wider">
                    Instrument
                  </th>
                  <th className="px-3 py-3 text-left text-xs font-medium text-blue-700 uppercase tracking-wider">
                    Status
                  </th>
                  {MEASURES.map((m) => (
                    <th
                      key={m.id}
                      className="px-3 py-3 text-right text-xs font-medium text-blue-700 uppercase tracking-wider"
                    >
                      {m.label}
                    </th>
                  ))}
                  <th className="px-3 py-3 text-right text-xs font-medium text-blue-700 uppercase tracking-wider">
                    Rev
                  </th>
                  <th className="px-3 py-3 text-right text-xs font-medium text-blue-700 uppercase tracking-wider">
                    Calculated
                  </th>
                  <th className="px-3 py-3 text-left text-xs font-medium text-blue-700 uppercase tracking-wider">
                    Clean (history)
                  </th>
                </tr>
              </thead>
              <tbody>
                {valuations.map((item) => (
                  <ValuationRow key={item.instrumentId} item={item} />
                ))}
              </tbody>
            </table>
          </div>
        </div>
      ) : (
        <div className="bg-white rounded-lg shadow-sm border p-12 text-center">
          <Activity className="h-12 w-12 text-gray-300 mx-auto mb-4" />
          <h3 className="text-lg font-semibold text-gray-900 mb-2">No Live Valuations</h3>
          <p className="text-gray-600 mb-4">
            {health === null
              ? 'This server does not expose a pricing runtime.'
              : health.mode === 'live'
                ? 'The live runtime has not published a result yet. This panel checks again every few seconds.'
                : `The runtime is ${health.phase}${health.mode ? ` in ${health.mode} mode` : ''}, not live.`}
          </p>
          <p className="text-sm text-gray-500">
            The panel reads the current results from <code>GET /api/v1/runtime/results</code>, then
            subscribes to those instruments with <code>subscribe_valuations</code>.
          </p>
        </div>
      )}
    </div>
  );
}

export default StreamingDemo;
