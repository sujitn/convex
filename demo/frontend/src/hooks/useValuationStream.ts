// =============================================================================
// Valuation Stream Hook
// Live runtime valuations: seed from GET /api/v1/runtime/results, subscribe to
// the same instruments, then follow the WebSocket `valuation` messages.
// =============================================================================

import { useState, useEffect, useCallback, useRef } from 'react';
import { useWebSocket, type ValuationMessage, type WebSocketMessage } from './useWebSocket';
import {
  ConvexApiError,
  getRuntimeHealth,
  getRuntimeResults,
  type MeasureValue,
  type PublishedValuation,
  type RuntimeHealth,
} from '../lib/api';

// The server admits at most 256 valuation subscriptions per session.
const MAX_SUBSCRIPTIONS = 256;
const MAX_HISTORY = 50;
// While the runtime has published nothing yet, look again this often.
const EMPTY_RETRY_MS = 5000;

// The latest published result for one instrument
export interface StreamedValuation {
  instrumentId: string;
  valuation: PublishedValuation;
  // Clean mid per applied revision, for the sparkline
  priceHistory: number[];
}

interface UseValuationStreamReturn {
  valuations: StreamedValuation[];
  health: RuntimeHealth | null;
  loadError: string | null;
  demoLabelled: boolean;
  connectionState: string;
  sessionId: string | null;
  latency: number | null;
  updateCount: number;
  lastUpdate: number | null;
  start: () => void;
  stop: () => void;
}

/** The instrument a result values, or null for any other subject. */
export function instrumentOf(valuation: PublishedValuation): string | null {
  const subject = valuation.result.provenance.subject;
  return subject.kind === 'instrument_version' ? subject.instrument_id : null;
}

/** A finite number from a measure value; undefined when absent or not numeric. */
export function measureNumber(value: MeasureValue | undefined): number | undefined {
  if (!value) return undefined;
  const number = typeof value.value === 'number' ? value.value : Number(value.value);
  return Number.isFinite(number) ? number : undefined;
}

function cleanMid(valuation: PublishedValuation): number | undefined {
  return measureNumber(valuation.result.results.clean_price_mid?.value);
}

function withPrice(history: number[], valuation: PublishedValuation): number[] {
  const price = cleanMid(valuation);
  return price === undefined ? history : [...history, price].slice(-MAX_HISTORY);
}

/**
 * Stream live runtime valuations. The REST read is the current state; the
 * WebSocket adds each later revision. Results are deduplicated by
 * (runtime_id, revision), and `RESYNC_REQUIRED` re-reads the current state.
 */
export function useValuationStream(autoConnect = true): UseValuationStreamReturn {
  const [valuations, setValuations] = useState<Map<string, StreamedValuation>>(new Map());
  const [health, setHealth] = useState<RuntimeHealth | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [demoLabelled, setDemoLabelled] = useState(false);
  const [updateCount, setUpdateCount] = useState(0);
  const [lastUpdate, setLastUpdate] = useState<number | null>(null);

  const subscribedRef = useRef<Set<string>>(new Set());
  const subscribeRef = useRef<(ids: string[]) => void>(() => {});

  // Replace the display with the runtime's current results, keeping the price
  // history of an instrument whose runtime and revision order are unchanged.
  const seed = useCallback(async () => {
    const [healthResult, resultsResult] = await Promise.allSettled([
      getRuntimeHealth(),
      getRuntimeResults(),
    ]);
    setHealth(healthResult.status === 'fulfilled' ? healthResult.value : null);
    if (resultsResult.status === 'rejected') {
      const reason = resultsResult.reason;
      // A server without a pricing runtime has no runtime routes; the empty state says so.
      if (reason instanceof ConvexApiError && reason.status === 404) {
        setLoadError(null);
      } else if (reason instanceof ConvexApiError) {
        setLoadError(`GET /api/v1/runtime/results failed with HTTP ${reason.status}`);
      } else {
        setLoadError('The runtime results could not be read');
      }
      return;
    }
    setLoadError(null);
    const current = resultsResult.value;
    setValuations((prev) => {
      const next = new Map<string, StreamedValuation>();
      for (const valuation of current) {
        const instrumentId = instrumentOf(valuation);
        if (!instrumentId) continue;
        const existing = prev.get(instrumentId);
        const continues =
          existing !== undefined &&
          existing.valuation.runtime_id === valuation.runtime_id &&
          existing.valuation.revision <= valuation.revision;
        const history = continues ? existing.priceHistory : [];
        next.set(instrumentId, {
          instrumentId,
          valuation,
          priceHistory:
            continues && existing.valuation.revision === valuation.revision
              ? history
              : withPrice(history, valuation),
        });
      }
      return next;
    });
    const fresh = current
      .map(instrumentOf)
      .filter((id): id is string => id !== null && !subscribedRef.current.has(id))
      .slice(0, Math.max(0, MAX_SUBSCRIPTIONS - subscribedRef.current.size));
    if (fresh.length > 0) {
      fresh.forEach((id) => subscribedRef.current.add(id));
      subscribeRef.current(fresh);
    }
  }, []);

  const apply = useCallback((valuation: PublishedValuation) => {
    const instrumentId = instrumentOf(valuation);
    if (!instrumentId) return;
    setValuations((prev) => {
      const existing = prev.get(instrumentId);
      if (
        existing !== undefined &&
        existing.valuation.runtime_id === valuation.runtime_id &&
        existing.valuation.revision >= valuation.revision
      ) {
        return prev;
      }
      const next = new Map(prev);
      next.set(instrumentId, {
        instrumentId,
        valuation,
        priceHistory: withPrice(existing?.priceHistory ?? [], valuation),
      });
      return next;
    });
    setUpdateCount((count) => count + 1);
    setLastUpdate(Date.now());
  }, []);

  const handleMessage = useCallback((msg: WebSocketMessage) => {
    if (msg.data_quality === 'demo') setDemoLabelled(true);
    if (msg.type === 'valuation') {
      apply(msg as ValuationMessage);
    } else if (msg.type === 'error' && msg.code === 'RESYNC_REQUIRED') {
      void seed();
    } else if (msg.type === 'error') {
      setLoadError(`${msg.code}: ${msg.message}`);
    }
  }, [apply, seed]);

  // Each connection is a new server session with no subscriptions.
  const handleConnect = useCallback(() => {
    subscribedRef.current = new Set();
    void seed();
  }, [seed]);

  const {
    connectionState,
    sessionId,
    latency,
    connect,
    disconnect,
    subscribeValuations,
  } = useWebSocket({
    autoConnect,
    onMessage: handleMessage,
    onConnect: handleConnect,
  });

  useEffect(() => {
    subscribeRef.current = subscribeValuations;
  }, [subscribeValuations]);

  // A runtime that has published nothing yet has no instruments to subscribe to.
  const hasRuntime = health !== null;
  useEffect(() => {
    if (connectionState !== 'connected' || !hasRuntime || valuations.size > 0) return;
    const timer = setInterval(() => void seed(), EMPTY_RETRY_MS);
    return () => clearInterval(timer);
  }, [connectionState, hasRuntime, valuations.size, seed]);

  const start = useCallback(() => connect(), [connect]);
  const stop = useCallback(() => disconnect(), [disconnect]);

  return {
    valuations: Array.from(valuations.values()).sort((a, b) =>
      a.instrumentId.localeCompare(b.instrumentId)
    ),
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
  };
}

export default useValuationStream;
