// =============================================================================
// WebSocket Connection Hook
// Provides auto-reconnecting WebSocket connection to convex-server
// =============================================================================

import { useState, useEffect, useCallback, useRef } from 'react';

// WebSocket message types from server
export interface WebSocketMessage {
  type: string;
  payload?: unknown;
  session_id?: string;
  instrument_id?: string;
  etf_id?: string;
  portfolio_id?: string;
  timestamp?: string;
}

// Bond quote update message
export interface BondQuoteMessage {
  type: 'bond_quote';
  instrument_id: string;
  clean_price_mid?: number;
  dirty_price_mid?: number;
  ytm_mid?: number;
  modified_duration?: number;
  convexity?: number;
  z_spread_mid?: number;
  i_spread_mid?: number;
  dv01?: number;
  timestamp: string;
}

// ETF quote update message
export interface EtfQuoteMessage {
  type: 'etf_quote';
  etf_id: string;
  inav: number;
  nav: number;
  premium_discount_pct: number;
  holdings_count: number;
  weighted_duration: number;
  weighted_yield: number;
  timestamp: string;
}

// Portfolio update message
export interface PortfolioMessage {
  type: 'portfolio_analytics';
  portfolio_id: string;
  total_market_value: number;
  modified_duration: number;
  weighted_yield: number;
  dv01: number;
  timestamp: string;
}

// Connection state
export type ConnectionState = 'disconnected' | 'connecting' | 'connected' | 'reconnecting';

// Hook options
interface UseWebSocketOptions {
  autoConnect?: boolean;
  reconnectAttempts?: number;
  reconnectInterval?: number;
  onMessage?: (msg: WebSocketMessage) => void;
  onConnect?: (sessionId: string) => void;
  onDisconnect?: () => void;
  onError?: (error: Event) => void;
}

// Hook return type
interface UseWebSocketReturn {
  connectionState: ConnectionState;
  sessionId: string | null;
  latency: number | null;
  lastError: string | null;
  connect: () => void;
  disconnect: () => void;
  subscribeBonds: (instrumentIds?: string[]) => void;
  unsubscribeBonds: (instrumentIds?: string[]) => void;
  subscribeAllBonds: () => void;
  unsubscribeAllBonds: () => void;
  subscribeEtfs: (etfIds: string[]) => void;
  unsubscribeEtfs: (etfIds: string[]) => void;
  subscribePortfolios: (portfolioIds: string[]) => void;
  unsubscribePortfolios: (portfolioIds: string[]) => void;
  sendMessage: (msg: object) => void;
}

// Determine WebSocket URL - prefer explicit env var, then derive from API URL, then use current host
const WS_URL = (() => {
  if (import.meta.env.VITE_WS_URL) {
    return import.meta.env.VITE_WS_URL;
  }
  if (import.meta.env.VITE_API_URL) {
    // Convert https://example.com to wss://example.com/ws
    const apiUrl = import.meta.env.VITE_API_URL;
    const wsProtocol = apiUrl.startsWith('https') ? 'wss:' : 'ws:';
    const host = apiUrl.replace(/^https?:\/\//, '');
    return `${wsProtocol}//${host}/ws`;
  }
  // Fallback to current host
  return `${window.location.protocol === 'https:' ? 'wss:' : 'ws:'}//${window.location.host}/ws`;
})();

/**
 * WebSocket connection hook with auto-reconnect and subscription management
 */
export function useWebSocket(options: UseWebSocketOptions = {}): UseWebSocketReturn {
  const {
    autoConnect = true,
    reconnectAttempts = 5,
    reconnectInterval = 2000,
    onMessage,
    onConnect,
    onDisconnect,
    onError,
  } = options;

  const [connectionState, setConnectionState] = useState<ConnectionState>('disconnected');
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [latency, setLatency] = useState<number | null>(null);
  const [lastError, setLastError] = useState<string | null>(null);

  const wsRef = useRef<WebSocket | null>(null);
  const reconnectCountRef = useRef(0);
  const reconnectTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pingIntervalRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const lastPingRef = useRef<number>(0);

  // Clear reconnect timeout
  const clearReconnectTimeout = useCallback(() => {
    if (reconnectTimeoutRef.current) {
      clearTimeout(reconnectTimeoutRef.current);
      reconnectTimeoutRef.current = null;
    }
  }, []);

  // Clear ping interval
  const clearPingInterval = useCallback(() => {
    if (pingIntervalRef.current) {
      clearInterval(pingIntervalRef.current);
      pingIntervalRef.current = null;
    }
  }, []);

  // Connect to WebSocket
  const connect = useCallback(() => {
    if (wsRef.current?.readyState === WebSocket.OPEN) {
      return;
    }

    setConnectionState('connecting');
    setLastError(null);

    try {
      const ws = new WebSocket(WS_URL);
      wsRef.current = ws;

      ws.onopen = () => {
        console.log('WebSocket connected to', WS_URL);
        setConnectionState('connected');
        reconnectCountRef.current = 0;

        // Start ping interval for latency measurement
        pingIntervalRef.current = setInterval(() => {
          if (ws.readyState === WebSocket.OPEN) {
            lastPingRef.current = Date.now();
            ws.send(JSON.stringify({ type: 'ping', timestamp: Date.now() }));
          }
        }, 10000);
      };

      ws.onmessage = (event) => {
        try {
          const data = JSON.parse(event.data) as WebSocketMessage;

          // Debug: log raw message type
          console.log('[WebSocket] Raw message received:', data.type);

          // Handle pong for latency
          if (data.type === 'pong' || data.type === 'heartbeat') {
            const latencyMs = Date.now() - lastPingRef.current;
            setLatency(latencyMs);
            return;
          }

          // Handle connection confirmation
          if (data.type === 'connected') {
            const sid = data.session_id || 'unknown';
            console.log('[WebSocket] Connected with session:', sid);
            setSessionId(sid);
            onConnect?.(sid);
            return;
          }

          // Handle subscription confirmation
          if (data.type === 'subscribed') {
            console.log('[WebSocket] Subscribed:', data);
            return;
          }

          // Pass other messages to callback
          console.log('[WebSocket] Passing to handler:', data.type);
          onMessage?.(data);
        } catch (e) {
          console.error('Failed to parse WebSocket message:', e);
        }
      };

      ws.onclose = () => {
        console.log('WebSocket disconnected');
        setConnectionState('disconnected');
        setSessionId(null);
        clearPingInterval();
        onDisconnect?.();

        // Attempt reconnect
        if (reconnectCountRef.current < reconnectAttempts) {
          reconnectCountRef.current++;
          setConnectionState('reconnecting');
          reconnectTimeoutRef.current = setTimeout(() => {
            console.log(`Reconnecting... (attempt ${reconnectCountRef.current}/${reconnectAttempts})`);
            connect();
          }, reconnectInterval * reconnectCountRef.current);
        }
      };

      ws.onerror = (error) => {
        console.error('WebSocket error:', error);
        setLastError('WebSocket connection error');
        onError?.(error);
      };
    } catch (error) {
      console.error('Failed to create WebSocket:', error);
      setLastError(String(error));
      setConnectionState('disconnected');
    }
  }, [onConnect, onDisconnect, onError, onMessage, reconnectAttempts, reconnectInterval, clearPingInterval]);

  // Disconnect from WebSocket
  const disconnect = useCallback(() => {
    clearReconnectTimeout();
    clearPingInterval();
    reconnectCountRef.current = reconnectAttempts; // Prevent reconnect

    if (wsRef.current) {
      wsRef.current.close();
      wsRef.current = null;
    }

    setConnectionState('disconnected');
    setSessionId(null);
  }, [clearReconnectTimeout, clearPingInterval, reconnectAttempts]);

  // Send message helper
  const sendMessage = useCallback((msg: object) => {
    if (wsRef.current?.readyState === WebSocket.OPEN) {
      wsRef.current.send(JSON.stringify(msg));
    } else {
      console.warn('WebSocket not connected, message not sent');
    }
  }, []);

  // Subscribe to bonds
  const subscribeBonds = useCallback((instrumentIds?: string[]) => {
    sendMessage({
      type: 'subscribe_bonds',
      instrument_ids: instrumentIds,
    });
  }, [sendMessage]);

  // Unsubscribe from bonds
  const unsubscribeBonds = useCallback((instrumentIds?: string[]) => {
    sendMessage({
      type: 'unsubscribe_bonds',
      instrument_ids: instrumentIds,
    });
  }, [sendMessage]);

  // Subscribe to all bonds
  const subscribeAllBonds = useCallback(() => {
    sendMessage({ type: 'subscribe_all_bonds' });
  }, [sendMessage]);

  // Unsubscribe from all bonds
  const unsubscribeAllBonds = useCallback(() => {
    sendMessage({ type: 'unsubscribe_all_bonds' });
  }, [sendMessage]);

  // Subscribe to ETFs
  const subscribeEtfs = useCallback((etfIds: string[]) => {
    sendMessage({
      type: 'subscribe_etfs',
      etf_ids: etfIds,
    });
  }, [sendMessage]);

  // Unsubscribe from ETFs
  const unsubscribeEtfs = useCallback((etfIds: string[]) => {
    sendMessage({
      type: 'unsubscribe_etfs',
      etf_ids: etfIds,
    });
  }, [sendMessage]);

  // Subscribe to portfolios
  const subscribePortfolios = useCallback((portfolioIds: string[]) => {
    sendMessage({
      type: 'subscribe_portfolios',
      portfolio_ids: portfolioIds,
    });
  }, [sendMessage]);

  // Unsubscribe from portfolios
  const unsubscribePortfolios = useCallback((portfolioIds: string[]) => {
    sendMessage({
      type: 'unsubscribe_portfolios',
      portfolio_ids: portfolioIds,
    });
  }, [sendMessage]);

  // Auto-connect on mount
  useEffect(() => {
    if (autoConnect) {
      connect();
    }

    return () => {
      disconnect();
    };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  return {
    connectionState,
    sessionId,
    latency,
    lastError,
    connect,
    disconnect,
    subscribeBonds,
    unsubscribeBonds,
    subscribeAllBonds,
    unsubscribeAllBonds,
    subscribeEtfs,
    unsubscribeEtfs,
    subscribePortfolios,
    unsubscribePortfolios,
    sendMessage,
  };
}

export default useWebSocket;
