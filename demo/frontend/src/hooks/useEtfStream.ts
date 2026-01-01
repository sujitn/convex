// =============================================================================
// ETF Stream Hook
// Subscribe to real-time ETF iNAV updates via WebSocket
// =============================================================================

import { useState, useEffect, useCallback } from 'react';
import { useWebSocket, EtfQuoteMessage, WebSocketMessage } from './useWebSocket';

// ETF quote with history and tracking
export interface StreamedEtfQuote {
  etfId: string;
  inav: number;
  nav: number;
  marketPrice?: number;
  premiumDiscountPct: number;
  holdingsCount: number;
  weightedDuration: number;
  weightedYield: number;
  timestamp: string;
  // Change tracking
  inavChange?: number;
  premiumChange?: number;
  // History for charts
  inavHistory: number[];
  premiumHistory: number[];
}

// ETF stream state
interface EtfStreamState {
  etfs: Map<string, StreamedEtfQuote>;
  isStreaming: boolean;
  connectionState: string;
  sessionId: string | null;
  latency: number | null;
  tickCount: number;
  lastUpdate: string | null;
}

// Hook options
interface UseEtfStreamOptions {
  etfIds?: string[];
  historyLength?: number;
  autoConnect?: boolean;
}

// Hook return type
interface UseEtfStreamReturn extends EtfStreamState {
  start: () => void;
  stop: () => void;
  subscribe: (etfIds: string[]) => void;
  unsubscribe: (etfIds: string[]) => void;
  clearHistory: () => void;
  getEtf: (etfId: string) => StreamedEtfQuote | undefined;
  getAllEtfs: () => StreamedEtfQuote[];
  setMarketPrice: (etfId: string, price: number) => void;
}

const MAX_HISTORY = 100;

/**
 * Hook for streaming ETF iNAV updates via WebSocket
 */
export function useEtfStream(options: UseEtfStreamOptions = {}): UseEtfStreamReturn {
  const {
    etfIds = [],
    historyLength = MAX_HISTORY,
    autoConnect = true,
  } = options;

  const [etfs, setEtfs] = useState<Map<string, StreamedEtfQuote>>(new Map());
  const [isStreaming, setIsStreaming] = useState(false);
  const [tickCount, setTickCount] = useState(0);
  const [lastUpdate, setLastUpdate] = useState<string | null>(null);

  // Handle incoming ETF quote messages
  const handleMessage = useCallback((msg: WebSocketMessage) => {
    if (msg.type !== 'etf_quote') return;

    const quote = msg as unknown as EtfQuoteMessage;

    setEtfs((prev) => {
      const newEtfs = new Map(prev);
      const existing = newEtfs.get(quote.etf_id);

      // Calculate changes
      const inavChange = existing ? quote.inav - existing.inav : undefined;
      const premiumChange = existing
        ? quote.premium_discount_pct - existing.premiumDiscountPct
        : undefined;

      // Update history arrays
      const inavHistory = existing?.inavHistory || [];
      const premiumHistory = existing?.premiumHistory || [];

      inavHistory.push(quote.inav);
      if (inavHistory.length > historyLength) {
        inavHistory.shift();
      }

      premiumHistory.push(quote.premium_discount_pct);
      if (premiumHistory.length > historyLength) {
        premiumHistory.shift();
      }

      newEtfs.set(quote.etf_id, {
        etfId: quote.etf_id,
        inav: quote.inav,
        nav: quote.nav,
        marketPrice: existing?.marketPrice,
        premiumDiscountPct: quote.premium_discount_pct,
        holdingsCount: quote.holdings_count,
        weightedDuration: quote.weighted_duration,
        weightedYield: quote.weighted_yield,
        timestamp: quote.timestamp,
        inavChange,
        premiumChange,
        inavHistory: [...inavHistory],
        premiumHistory: [...premiumHistory],
      });

      return newEtfs;
    });

    setTickCount((prev) => prev + 1);
    setLastUpdate(quote.timestamp);
  }, [historyLength]);

  // WebSocket connection
  const {
    connectionState,
    sessionId,
    latency,
    connect,
    disconnect,
    subscribeEtfs,
    unsubscribeEtfs,
  } = useWebSocket({
    autoConnect: false,
    onMessage: handleMessage,
    onConnect: () => {
      console.log('ETF stream connected');
      // Auto-subscribe if configured
      if (etfIds.length > 0) {
        subscribeEtfs(etfIds);
      }
    },
    onDisconnect: () => {
      console.log('ETF stream disconnected');
      setIsStreaming(false);
    },
  });

  // Start streaming
  const start = useCallback(() => {
    setIsStreaming(true);
    connect();
  }, [connect]);

  // Stop streaming
  const stop = useCallback(() => {
    setIsStreaming(false);
    disconnect();
  }, [disconnect]);

  // Subscribe to ETFs
  const subscribe = useCallback((ids: string[]) => {
    subscribeEtfs(ids);
  }, [subscribeEtfs]);

  // Unsubscribe from ETFs
  const unsubscribe = useCallback((ids: string[]) => {
    unsubscribeEtfs(ids);
  }, [unsubscribeEtfs]);

  // Clear history
  const clearHistory = useCallback(() => {
    setEtfs(new Map());
    setTickCount(0);
    setLastUpdate(null);
  }, []);

  // Get single ETF
  const getEtf = useCallback((etfId: string) => {
    return etfs.get(etfId);
  }, [etfs]);

  // Get all ETFs as array
  const getAllEtfs = useCallback(() => {
    return Array.from(etfs.values());
  }, [etfs]);

  // Set market price for premium/discount calculation
  const setMarketPrice = useCallback((etfId: string, price: number) => {
    setEtfs((prev) => {
      const newEtfs = new Map(prev);
      const existing = newEtfs.get(etfId);
      if (existing) {
        newEtfs.set(etfId, {
          ...existing,
          marketPrice: price,
        });
      }
      return newEtfs;
    });
  }, []);

  // Auto-connect on mount if enabled
  useEffect(() => {
    if (autoConnect && etfIds.length > 0) {
      start();
    }

    return () => {
      stop();
    };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  return {
    etfs,
    isStreaming,
    connectionState,
    sessionId,
    latency,
    tickCount,
    lastUpdate,
    start,
    stop,
    subscribe,
    unsubscribe,
    clearHistory,
    getEtf,
    getAllEtfs,
    setMarketPrice,
  };
}

export default useEtfStream;
