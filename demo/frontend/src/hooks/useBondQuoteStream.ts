// =============================================================================
// Bond Quote Stream Hook
// Subscribe to real-time bond quote updates via WebSocket
// =============================================================================

import { useState, useEffect, useCallback } from 'react';
import { useWebSocket, BondQuoteMessage, WebSocketMessage } from './useWebSocket';

// Individual quote with history
export interface StreamedQuote {
  instrumentId: string;
  bid?: number;
  mid?: number;
  ask?: number;
  ytm?: number;
  duration?: number;
  convexity?: number;
  zSpread?: number;
  iSpread?: number;
  dv01?: number;
  timestamp: string;
  // Change tracking
  priceChange?: number;
  yieldChange?: number;
  // History for sparklines
  priceHistory: number[];
  yieldHistory: number[];
}

// Quote stream state
interface QuoteStreamState {
  quotes: Map<string, StreamedQuote>;
  isStreaming: boolean;
  connectionState: string;
  sessionId: string | null;
  latency: number | null;
  tickCount: number;
  lastUpdate: string | null;
}

// Hook options
interface UseBondQuoteStreamOptions {
  instrumentIds?: string[];
  subscribeAll?: boolean;
  historyLength?: number;
  autoConnect?: boolean;
}

// Hook return type
interface UseBondQuoteStreamReturn extends QuoteStreamState {
  start: () => void;
  stop: () => void;
  subscribe: (instrumentIds: string[]) => void;
  unsubscribe: (instrumentIds: string[]) => void;
  clearHistory: () => void;
  getQuote: (instrumentId: string) => StreamedQuote | undefined;
  getAllQuotes: () => StreamedQuote[];
}

const MAX_HISTORY = 50;

/**
 * Hook for streaming bond quotes via WebSocket
 */
export function useBondQuoteStream(
  options: UseBondQuoteStreamOptions = {}
): UseBondQuoteStreamReturn {
  const {
    instrumentIds = [],
    subscribeAll = false,
    historyLength = MAX_HISTORY,
    autoConnect = true,
  } = options;

  const [quotes, setQuotes] = useState<Map<string, StreamedQuote>>(new Map());
  const [isStreaming, setIsStreaming] = useState(false);
  const [tickCount, setTickCount] = useState(0);
  const [lastUpdate, setLastUpdate] = useState<string | null>(null);

  // Handle incoming quote messages
  const handleMessage = useCallback((msg: WebSocketMessage) => {
    // Debug: log all messages
    console.log('[WS] Received message:', msg.type, msg);

    if (msg.type !== 'bond_quote') return;

    const quote = msg as unknown as BondQuoteMessage;

    setQuotes((prev) => {
      const newQuotes = new Map(prev);
      const existing = newQuotes.get(quote.instrument_id);

      // Calculate changes
      const priceChange = existing?.mid && quote.clean_price_mid
        ? quote.clean_price_mid - existing.mid
        : undefined;
      const yieldChange = existing?.ytm && quote.ytm_mid
        ? quote.ytm_mid - existing.ytm
        : undefined;

      // Update history arrays
      const priceHistory = existing?.priceHistory || [];
      const yieldHistory = existing?.yieldHistory || [];

      if (quote.clean_price_mid) {
        priceHistory.push(quote.clean_price_mid);
        if (priceHistory.length > historyLength) {
          priceHistory.shift();
        }
      }

      if (quote.ytm_mid) {
        yieldHistory.push(quote.ytm_mid);
        if (yieldHistory.length > historyLength) {
          yieldHistory.shift();
        }
      }

      newQuotes.set(quote.instrument_id, {
        instrumentId: quote.instrument_id,
        mid: quote.clean_price_mid,
        ytm: quote.ytm_mid,
        duration: quote.modified_duration,
        convexity: quote.convexity,
        zSpread: quote.z_spread_mid,
        iSpread: quote.i_spread_mid,
        dv01: quote.dv01,
        timestamp: quote.timestamp,
        priceChange,
        yieldChange,
        priceHistory: [...priceHistory],
        yieldHistory: [...yieldHistory],
      });

      return newQuotes;
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
    subscribeBonds,
    unsubscribeBonds,
    subscribeAllBonds,
    unsubscribeAllBonds,
  } = useWebSocket({
    autoConnect: false,
    onMessage: handleMessage,
    onConnect: () => {
      console.log('Bond quote stream connected');
      // Auto-subscribe if configured
      if (subscribeAll) {
        subscribeAllBonds();
      } else if (instrumentIds.length > 0) {
        subscribeBonds(instrumentIds);
      }
    },
    onDisconnect: () => {
      console.log('Bond quote stream disconnected');
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
    unsubscribeAllBonds();
    disconnect();
  }, [disconnect, unsubscribeAllBonds]);

  // Subscribe to specific instruments
  const subscribe = useCallback((ids: string[]) => {
    subscribeBonds(ids);
  }, [subscribeBonds]);

  // Unsubscribe from specific instruments
  const unsubscribe = useCallback((ids: string[]) => {
    unsubscribeBonds(ids);
  }, [unsubscribeBonds]);

  // Clear all history
  const clearHistory = useCallback(() => {
    setQuotes(new Map());
    setTickCount(0);
    setLastUpdate(null);
  }, []);

  // Get single quote
  const getQuote = useCallback((instrumentId: string) => {
    return quotes.get(instrumentId);
  }, [quotes]);

  // Get all quotes as array
  const getAllQuotes = useCallback(() => {
    return Array.from(quotes.values());
  }, [quotes]);

  // Auto-connect on mount if enabled
  useEffect(() => {
    if (autoConnect) {
      start();
    }

    return () => {
      stop();
    };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  return {
    quotes,
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
    getQuote,
    getAllQuotes,
  };
}

export default useBondQuoteStream;
