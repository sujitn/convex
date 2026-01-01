// =============================================================================
// Streaming Demo Panel
// Real-time bond quote streaming with simulation controls
// =============================================================================

import { useState, useEffect } from 'react';
import {
  Play,
  Pause,
  RefreshCw,
  Wifi,
  WifiOff,
  Activity,
  TrendingUp,
  TrendingDown,
  AlertTriangle,
  Zap,
  ArrowDown,
  ArrowUp,
  Gauge,
  Radio,
  Server,
  ArrowRight,
  Database,
  LineChart,
} from 'lucide-react';
import {
  startQuoteSimulation,
  stopQuoteSimulation,
  getQuoteProviderStatus,
  getQuoteProviderQuotes,
  triggerQuoteTick,
  applyStressScenario,
  refreshQuoteProviderCurves,
  type QuoteState,
  type QuoteProviderStatus,
} from '../lib/api';
import { useBondQuoteStream } from '../hooks/useBondQuoteStream';

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

// Quote row with change indicators
function QuoteRow({
  quote,
  priceHistory,
}: {
  quote: QuoteState;
  priceHistory: number[];
}) {
  const [flash, setFlash] = useState<'up' | 'down' | null>(null);
  const [prevMid, setPrevMid] = useState(quote.mid);

  useEffect(() => {
    if (quote.mid !== prevMid) {
      setFlash(quote.mid > prevMid ? 'up' : 'down');
      setPrevMid(quote.mid);
      const timer = setTimeout(() => setFlash(null), 500);
      return () => clearTimeout(timer);
    }
  }, [quote.mid, prevMid]);

  const bidAskSpread = quote.ask - quote.bid;
  const flashClass = flash === 'up' ? 'bg-green-100' : flash === 'down' ? 'bg-red-100' : '';

  return (
    <tr className={`border-b border-gray-100 transition-colors ${flashClass}`}>
      <td className="px-4 py-2 font-mono text-sm font-medium">{quote.instrument_id}</td>
      <td className="px-4 py-2 text-right font-mono text-sm">{quote.bid.toFixed(3)}</td>
      <td className="px-4 py-2 text-right font-mono text-sm font-medium">
        {quote.mid.toFixed(3)}
        {flash === 'up' && <ArrowUp className="inline ml-1 h-3 w-3 text-green-600" />}
        {flash === 'down' && <ArrowDown className="inline ml-1 h-3 w-3 text-red-600" />}
      </td>
      <td className="px-4 py-2 text-right font-mono text-sm">{quote.ask.toFixed(3)}</td>
      <td className="px-4 py-2 text-right font-mono text-sm text-gray-600">{bidAskSpread.toFixed(3)}</td>
      <td className="px-4 py-2 text-right font-mono text-sm">{(quote.yield * 100).toFixed(2)}%</td>
      <td className="px-4 py-2">
        <Sparkline
          data={priceHistory}
          color={priceHistory.length > 1 && priceHistory[priceHistory.length - 1] >= priceHistory[0] ? '#10b981' : '#ef4444'}
        />
      </td>
    </tr>
  );
}

// Calculation cascade step
interface CascadeStep {
  name: string;
  icon: React.ReactNode;
  status: 'idle' | 'active' | 'completed';
  count?: number;
  latencyMs?: number;
  lastUpdate?: string;
}

export function StreamingDemo() {
  // Provider state
  const [status, setStatus] = useState<QuoteProviderStatus | null>(null);
  const [quotes, setQuotes] = useState<QuoteState[]>([]);
  const [priceHistories, setPriceHistories] = useState<Map<string, number[]>>(new Map());
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Simulation settings
  const [intervalMs, setIntervalMs] = useState(1000);
  const [volatility, setVolatility] = useState<'low' | 'medium' | 'high'>('medium');
  const [mode, setMode] = useState<'static' | 'random_walk' | 'mean_revert' | 'stress'>('random_walk');

  // Polling for quotes
  const [polling, setPolling] = useState(false);
  const [tickCount, setTickCount] = useState(0);

  // WebSocket viewer (shows server responses alongside simulation)
  const {
    isStreaming: wsIsStreaming,
    connectionState,
    sessionId,
    latency: wsLatency,
    tickCount: wsTickCount,
    start: startWsStream,
    stop: stopWsStream,
    getAllQuotes,
  } = useBondQuoteStream({ autoConnect: false, subscribeAll: true });

  // Cascade visualization state
  const [cascadeSteps, setCascadeSteps] = useState<CascadeStep[]>([
    { name: 'Market Data', icon: <Database className="h-4 w-4" />, status: 'idle' },
    { name: 'Quote Provider', icon: <Radio className="h-4 w-4" />, status: 'idle' },
    { name: 'Pricing Engine', icon: <Server className="h-4 w-4" />, status: 'idle' },
    { name: 'Analytics', icon: <LineChart className="h-4 w-4" />, status: 'idle' },
  ]);

  // Fetch initial status
  useEffect(() => {
    fetchStatus();
  }, []);

  // Update cascade visualization
  const updateCascade = (step: number, status: 'idle' | 'active' | 'completed', count?: number, latencyMs?: number) => {
    setCascadeSteps((prev) => {
      const newSteps = [...prev];
      newSteps[step] = {
        ...newSteps[step],
        status,
        count,
        latencyMs,
        lastUpdate: new Date().toISOString(),
      };
      return newSteps;
    });
  };

  // Polling loop for simulation
  useEffect(() => {
    if (!polling) return;

    const interval = setInterval(async () => {
      const tickStart = Date.now();
      try {
        // Step 1: Market Data active
        updateCascade(0, 'active');

        // Step 2: Quote Provider active
        updateCascade(1, 'active');

        // Trigger a tick
        const tickResult = await triggerQuoteTick();
        setTickCount(tickResult.tick);

        // Step 2: Quote Provider completed
        updateCascade(1, 'completed', tickResult.results.length);

        // Step 3: Pricing Engine active
        updateCascade(2, 'active');

        // Update quotes and histories
        const newQuotes = tickResult.results.map((r) => r.quote);
        setQuotes(newQuotes);

        // Step 3: Pricing Engine completed
        const pricingLatency = Date.now() - tickStart;
        updateCascade(2, 'completed', newQuotes.length, pricingLatency);

        // Step 4: Analytics completed
        updateCascade(3, 'completed', newQuotes.length, pricingLatency);

        setPriceHistories((prev) => {
          const newHistories = new Map(prev);
          for (const q of newQuotes) {
            const history = newHistories.get(q.instrument_id) || [];
            history.push(q.mid);
            if (history.length > 50) history.shift();
            newHistories.set(q.instrument_id, history);
          }
          return newHistories;
        });

        // Reset cascade for next tick
        setTimeout(() => {
          setCascadeSteps((prev) => prev.map((s) => ({ ...s, status: 'idle' as const })));
        }, 500);
      } catch (e) {
        console.error('Tick failed:', e);
        setCascadeSteps((prev) => prev.map((s) => ({ ...s, status: 'idle' as const })));
      }
    }, intervalMs);

    return () => clearInterval(interval);
  }, [polling, intervalMs]);

  const fetchStatus = async () => {
    try {
      const s = await getQuoteProviderStatus();
      setStatus(s);
    } catch (e) {
      console.error('Failed to fetch status:', e);
    }
  };

  const handleStart = async () => {
    setIsLoading(true);
    setError(null);
    try {
      const result = await startQuoteSimulation({
        interval_ms: intervalMs,
        volatility,
        mode,
        force: true, // Always force restart if already running
      });
      setQuotes(result.initial_quotes);

      // Initialize price histories
      const histories = new Map<string, number[]>();
      for (const q of result.initial_quotes) {
        histories.set(q.instrument_id, [q.mid]);
      }
      setPriceHistories(histories);

      setPolling(true);
      setTickCount(0);
      await fetchStatus();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to start');
    } finally {
      setIsLoading(false);
    }
  };

  const handleStop = async () => {
    setIsLoading(true);
    setPolling(false);
    try {
      await stopQuoteSimulation();
      await fetchStatus();
    } catch (e) {
      console.error('Failed to stop:', e);
    } finally {
      setIsLoading(false);
    }
  };

  const handleRefreshCurves = async () => {
    setIsLoading(true);
    try {
      await refreshQuoteProviderCurves();
      // Re-fetch quotes after curve refresh
      const quotesResult = await getQuoteProviderQuotes();
      setQuotes(quotesResult.quotes);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to refresh curves');
    } finally {
      setIsLoading(false);
    }
  };

  const handleStress = async (scenario: 'rates_up_100bp' | 'rates_down_100bp' | 'spreads_wide_50bp' | 'spreads_tight_50bp' | 'flight_to_quality' | 'risk_on') => {
    setIsLoading(true);
    try {
      const result = await applyStressScenario(scenario);
      setQuotes(result.new_quotes);

      // Update histories with new prices
      setPriceHistories((prev) => {
        const newHistories = new Map(prev);
        for (const q of result.new_quotes) {
          const history = newHistories.get(q.instrument_id) || [];
          history.push(q.mid);
          if (history.length > 50) history.shift();
          newHistories.set(q.instrument_id, history);
        }
        return newHistories;
      });
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to apply stress');
    } finally {
      setIsLoading(false);
    }
  };

  // WebSocket quotes for side-by-side comparison
  const wsQuotesList = getAllQuotes().map(q => ({
    instrument_id: q.instrumentId,
    bid: q.mid ? q.mid - 0.05 : 0,
    mid: q.mid || 0,
    ask: q.mid ? q.mid + 0.05 : 0,
    yield: q.ytm || 0,
    last_update: q.timestamp,
  }));

  return (
    <div className="p-6 space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-2xl font-bold text-gray-900">Streaming Demo</h2>
          <p className="text-gray-600">Real-time bond quote streaming with full server round-trip</p>
        </div>
        <div className="flex items-center gap-4">
          {/* Simulation Status */}
          {status?.running || polling ? (
            <span className="flex items-center gap-1 text-green-600">
              <Radio className="h-4 w-4" />
              Simulation Running
            </span>
          ) : (
            <span className="flex items-center gap-1 text-gray-400">
              <Radio className="h-4 w-4" />
              Simulation Stopped
            </span>
          )}

          {/* WebSocket Status */}
          {wsIsStreaming ? (
            <span className="flex items-center gap-1 text-blue-600">
              <Wifi className="h-4 w-4" />
              WebSocket: {connectionState}
            </span>
          ) : (
            <span className="flex items-center gap-1 text-gray-400">
              <WifiOff className="h-4 w-4" />
              WebSocket: Off
            </span>
          )}
        </div>
      </div>

      {error && (
        <div className="bg-red-50 border border-red-200 text-red-700 px-4 py-3 rounded-lg flex items-center gap-2">
          <AlertTriangle className="h-5 w-5" />
          {error}
        </div>
      )}

      {/* Control Panel - Simulation */}
      <div className="bg-white rounded-lg shadow-sm border p-6">
          <h3 className="text-lg font-semibold mb-4 flex items-center gap-2">
            <Gauge className="h-5 w-5" />
            Simulation Controls
          </h3>

          <div className="grid grid-cols-1 md:grid-cols-4 gap-4 mb-4">
            {/* Interval */}
            <div>
              <label className="block text-sm font-medium text-gray-700 mb-1">Tick Interval</label>
              <select
                value={intervalMs}
                onChange={(e) => setIntervalMs(Number(e.target.value))}
                className="w-full border rounded-lg px-3 py-2"
                disabled={polling}
              >
                <option value={500}>500ms (Fast)</option>
                <option value={1000}>1s (Normal)</option>
                <option value={2000}>2s (Slow)</option>
                <option value={5000}>5s (Very Slow)</option>
              </select>
            </div>

            {/* Volatility */}
            <div>
              <label className="block text-sm font-medium text-gray-700 mb-1">Volatility</label>
              <select
                value={volatility}
                onChange={(e) => setVolatility(e.target.value as 'low' | 'medium' | 'high')}
                className="w-full border rounded-lg px-3 py-2"
                disabled={polling}
              >
                <option value="low">Low</option>
                <option value="medium">Medium</option>
                <option value="high">High</option>
              </select>
            </div>

            {/* Mode */}
            <div>
              <label className="block text-sm font-medium text-gray-700 mb-1">Price Mode</label>
              <select
                value={mode}
                onChange={(e) => setMode(e.target.value as 'static' | 'random_walk' | 'mean_revert' | 'stress')}
                className="w-full border rounded-lg px-3 py-2"
                disabled={polling}
              >
                <option value="static">Static</option>
                <option value="random_walk">Random Walk</option>
                <option value="mean_revert">Mean Reverting</option>
                <option value="stress">Stress</option>
              </select>
            </div>

            {/* Start/Stop */}
            <div className="flex items-end gap-2">
              {!polling ? (
                <button
                  onClick={handleStart}
                  disabled={isLoading}
                  className="flex-1 flex items-center justify-center gap-2 bg-green-600 hover:bg-green-700 text-white px-4 py-2 rounded-lg disabled:opacity-50"
                >
                  <Play className="h-4 w-4" />
                  Start
                </button>
              ) : (
                <button
                  onClick={handleStop}
                  disabled={isLoading}
                  className="flex-1 flex items-center justify-center gap-2 bg-red-600 hover:bg-red-700 text-white px-4 py-2 rounded-lg disabled:opacity-50"
                >
                  <Pause className="h-4 w-4" />
                  Stop
                </button>
              )}
              <button
                onClick={handleRefreshCurves}
                disabled={isLoading}
                className="flex items-center justify-center gap-2 bg-gray-100 hover:bg-gray-200 text-gray-700 px-4 py-2 rounded-lg disabled:opacity-50"
                title="Refresh curves from FRED"
              >
                <RefreshCw className={`h-4 w-4 ${isLoading ? 'animate-spin' : ''}`} />
              </button>
            </div>
          </div>

          {/* Stress Scenarios */}
          <div className="border-t pt-4">
            <h4 className="text-sm font-medium text-gray-700 mb-2 flex items-center gap-1">
              <Zap className="h-4 w-4" />
              Stress Scenarios
            </h4>
            <div className="flex flex-wrap gap-2">
              <button
                onClick={() => handleStress('rates_up_100bp')}
                disabled={isLoading || !status?.running}
                className="flex items-center gap-1 px-3 py-1.5 text-sm bg-orange-100 hover:bg-orange-200 text-orange-700 rounded-lg disabled:opacity-50"
              >
                <TrendingUp className="h-3 w-3" />
                Rates +100bp
              </button>
              <button
                onClick={() => handleStress('rates_down_100bp')}
                disabled={isLoading || !status?.running}
                className="flex items-center gap-1 px-3 py-1.5 text-sm bg-blue-100 hover:bg-blue-200 text-blue-700 rounded-lg disabled:opacity-50"
              >
                <TrendingDown className="h-3 w-3" />
                Rates -100bp
              </button>
              <button
                onClick={() => handleStress('spreads_wide_50bp')}
                disabled={isLoading || !status?.running}
                className="flex items-center gap-1 px-3 py-1.5 text-sm bg-red-100 hover:bg-red-200 text-red-700 rounded-lg disabled:opacity-50"
              >
                <AlertTriangle className="h-3 w-3" />
                Spreads +50bp
              </button>
              <button
                onClick={() => handleStress('spreads_tight_50bp')}
                disabled={isLoading || !status?.running}
                className="flex items-center gap-1 px-3 py-1.5 text-sm bg-green-100 hover:bg-green-200 text-green-700 rounded-lg disabled:opacity-50"
              >
                <Activity className="h-3 w-3" />
                Spreads -50bp
              </button>
              <button
                onClick={() => handleStress('flight_to_quality')}
                disabled={isLoading || !status?.running}
                className="flex items-center gap-1 px-3 py-1.5 text-sm bg-purple-100 hover:bg-purple-200 text-purple-700 rounded-lg disabled:opacity-50"
              >
                Flight to Quality
              </button>
              <button
                onClick={() => handleStress('risk_on')}
                disabled={isLoading || !status?.running}
                className="flex items-center gap-1 px-3 py-1.5 text-sm bg-emerald-100 hover:bg-emerald-200 text-emerald-700 rounded-lg disabled:opacity-50"
              >
                Risk On
              </button>
            </div>
          </div>

          {/* WebSocket Toggle */}
          <div className="border-t pt-4 mt-4">
            <div className="flex items-center justify-between">
              <div>
                <h4 className="text-sm font-medium text-gray-700 flex items-center gap-1">
                  <Wifi className="h-4 w-4" />
                  WebSocket Viewer
                </h4>
                <p className="text-xs text-gray-500 mt-1">
                  Connect to server WebSocket to see quotes round-trip back from the pricing engine
                </p>
              </div>
              <div className="flex items-center gap-4">
                {wsIsStreaming && (
                  <div className="flex items-center gap-4 text-sm text-gray-600">
                    <span>Session: <code className="bg-gray-100 px-1 rounded">{sessionId?.slice(0, 8) || '-'}</code></span>
                    <span>Latency: <code className="bg-gray-100 px-1 rounded">{wsLatency ? `${wsLatency}ms` : '-'}</code></span>
                    <span>Quotes: <code className="bg-gray-100 px-1 rounded">{wsTickCount}</code></span>
                  </div>
                )}
                {!wsIsStreaming ? (
                  <button
                    onClick={startWsStream}
                    className="flex items-center gap-2 bg-blue-600 hover:bg-blue-700 text-white px-4 py-2 rounded-lg text-sm"
                  >
                    <Wifi className="h-4 w-4" />
                    Connect WebSocket
                  </button>
                ) : (
                  <button
                    onClick={stopWsStream}
                    className="flex items-center gap-2 bg-gray-600 hover:bg-gray-700 text-white px-4 py-2 rounded-lg text-sm"
                  >
                    <WifiOff className="h-4 w-4" />
                    Disconnect
                  </button>
                )}
              </div>
            </div>
          </div>
        </div>

      {/* Calculation Cascade Visualization */}
      {(polling || wsIsStreaming) && (
        <div className="bg-white rounded-lg shadow-sm border p-6">
          <h3 className="text-lg font-semibold mb-4 flex items-center gap-2">
            <Activity className="h-5 w-5" />
            Calculation Cascade
          </h3>
          <div className="flex items-center justify-center gap-2">
            {cascadeSteps.map((step, index) => (
              <div key={step.name} className="flex items-center">
                <div
                  className={`flex flex-col items-center p-4 rounded-lg border-2 transition-all min-w-[120px] ${
                    step.status === 'active'
                      ? 'border-blue-500 bg-blue-50 animate-pulse'
                      : step.status === 'completed'
                      ? 'border-green-500 bg-green-50'
                      : 'border-gray-200 bg-gray-50'
                  }`}
                >
                  <div
                    className={`mb-2 ${
                      step.status === 'active'
                        ? 'text-blue-600'
                        : step.status === 'completed'
                        ? 'text-green-600'
                        : 'text-gray-400'
                    }`}
                  >
                    {step.icon}
                  </div>
                  <div className="text-sm font-medium text-gray-700">{step.name}</div>
                  {step.count !== undefined && (
                    <div className="text-xs text-gray-500 mt-1">{step.count} items</div>
                  )}
                  {step.latencyMs !== undefined && (
                    <div className="text-xs text-gray-500">{step.latencyMs}ms</div>
                  )}
                </div>
                {index < cascadeSteps.length - 1 && (
                  <ArrowRight
                    className={`h-5 w-5 mx-2 ${
                      step.status === 'completed' ? 'text-green-500' : 'text-gray-300'
                    }`}
                  />
                )}
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Stats Bar */}
      {polling && (
        <div className="grid grid-cols-4 gap-4">
          <div className="bg-white rounded-lg shadow-sm border p-4">
            <div className="text-sm text-gray-600">Simulation Ticks</div>
            <div className="text-2xl font-bold text-gray-900">{tickCount}</div>
          </div>
          <div className="bg-white rounded-lg shadow-sm border p-4">
            <div className="text-sm text-gray-600">Instruments</div>
            <div className="text-2xl font-bold text-gray-900">{quotes.length}</div>
          </div>
          <div className="bg-white rounded-lg shadow-sm border p-4">
            <div className="text-sm text-gray-600">Last Sim Update</div>
            <div className="text-lg font-mono text-gray-900">
              {quotes[0]?.last_update ? new Date(quotes[0].last_update).toLocaleTimeString() : '-'}
            </div>
          </div>
          <div className="bg-white rounded-lg shadow-sm border p-4">
            <div className="text-sm text-gray-600">WebSocket Quotes</div>
            <div className="text-2xl font-bold text-blue-600">{wsIsStreaming ? wsTickCount : '-'}</div>
          </div>
        </div>
      )}

      {/* Quotes Grid - Simulation */}
      {quotes.length > 0 && (
        <div className="bg-white rounded-lg shadow-sm border overflow-hidden">
          <div className="px-6 py-4 border-b bg-gray-50">
            <h3 className="text-lg font-semibold flex items-center gap-2">
              <Radio className="h-5 w-5 text-green-600" />
              Simulation Quotes (from Quote Provider)
            </h3>
          </div>
          <div className="overflow-x-auto">
            <table className="w-full">
              <thead className="bg-gray-50">
                <tr>
                  <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
                    Instrument
                  </th>
                  <th className="px-4 py-3 text-right text-xs font-medium text-gray-500 uppercase tracking-wider">
                    Bid
                  </th>
                  <th className="px-4 py-3 text-right text-xs font-medium text-gray-500 uppercase tracking-wider">
                    Mid
                  </th>
                  <th className="px-4 py-3 text-right text-xs font-medium text-gray-500 uppercase tracking-wider">
                    Ask
                  </th>
                  <th className="px-4 py-3 text-right text-xs font-medium text-gray-500 uppercase tracking-wider">
                    Spread
                  </th>
                  <th className="px-4 py-3 text-right text-xs font-medium text-gray-500 uppercase tracking-wider">
                    Yield
                  </th>
                  <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
                    Price (20 ticks)
                  </th>
                </tr>
              </thead>
              <tbody>
                {quotes.map((quote) => (
                  <QuoteRow
                    key={quote.instrument_id}
                    quote={quote}
                    priceHistory={priceHistories.get(quote.instrument_id) || []}
                  />
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}

      {/* Quotes Grid - WebSocket */}
      {wsIsStreaming && wsQuotesList.length > 0 && (
        <div className="bg-white rounded-lg shadow-sm border overflow-hidden">
          <div className="px-6 py-4 border-b bg-blue-50">
            <h3 className="text-lg font-semibold flex items-center gap-2">
              <Wifi className="h-5 w-5 text-blue-600" />
              WebSocket Quotes (from Pricing Server)
              <span className="text-xs font-normal text-blue-600 bg-blue-100 px-2 py-1 rounded-full">
                {wsTickCount} received
              </span>
            </h3>
          </div>
          <div className="overflow-x-auto">
            <table className="w-full">
              <thead className="bg-blue-50">
                <tr>
                  <th className="px-4 py-3 text-left text-xs font-medium text-blue-700 uppercase tracking-wider">
                    Instrument
                  </th>
                  <th className="px-4 py-3 text-right text-xs font-medium text-blue-700 uppercase tracking-wider">
                    Mid Price
                  </th>
                  <th className="px-4 py-3 text-right text-xs font-medium text-blue-700 uppercase tracking-wider">
                    YTM
                  </th>
                  <th className="px-4 py-3 text-right text-xs font-medium text-blue-700 uppercase tracking-wider">
                    Last Update
                  </th>
                </tr>
              </thead>
              <tbody>
                {wsQuotesList.map((quote) => (
                  <tr key={quote.instrument_id} className="border-b border-blue-100">
                    <td className="px-4 py-2 font-mono text-sm font-medium">{quote.instrument_id}</td>
                    <td className="px-4 py-2 text-right font-mono text-sm">{quote.mid.toFixed(3)}</td>
                    <td className="px-4 py-2 text-right font-mono text-sm">{(quote.yield * 100).toFixed(2)}%</td>
                    <td className="px-4 py-2 text-right font-mono text-sm text-gray-500">
                      {quote.last_update ? new Date(quote.last_update).toLocaleTimeString() : '-'}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}

      {/* Empty State */}
      {quotes.length === 0 && !polling && (
        <div className="bg-white rounded-lg shadow-sm border p-12 text-center">
          <Activity className="h-12 w-12 text-gray-300 mx-auto mb-4" />
          <h3 className="text-lg font-semibold text-gray-900 mb-2">No Active Simulation</h3>
          <p className="text-gray-600 mb-4">
            Click "Start" to begin streaming synthetic bond quotes through the pricing server.
          </p>
          <p className="text-sm text-gray-500">
            The simulation generates prices based on live Treasury curves from FRED,
            applies sector/rating spreads, and pushes quotes to the Convex pricing server.
            Enable "WebSocket Viewer" to see quotes round-trip back from the server.
          </p>
        </div>
      )}
    </div>
  );
}

export default StreamingDemo;
