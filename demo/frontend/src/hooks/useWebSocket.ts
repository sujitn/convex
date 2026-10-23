// =============================================================================
// WebSocket Connection Hook
// Provides auto-reconnecting WebSocket connection to convex-server
// =============================================================================

import { useState, useEffect, useCallback, useRef } from 'react';
import type { PublishedValuation } from '../lib/api';

// WebSocket message from server. Demo servers label every message with
// `profile: "demo"` and `data_quality: "demo"`.
export interface WebSocketMessage {
  type: string;
  session_id?: string;
  code?: string;
  message?: string;
  profile?: string;
  data_quality?: string;
}

// One runtime result; the PublishedValuation fields sit beside `type`.
export type ValuationMessage = WebSocketMessage & PublishedValuation & { type: 'valuation' };

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
  subscribeValuations: (instrumentIds: string[]) => void;
  unsubscribeValuations: (instrumentIds: string[]) => void;
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
        if (wsRef.current !== ws) {
          ws.close();
          return;
        }
        console.log('WebSocket connected to', WS_URL);
        setConnectionState('connected');
        reconnectCountRef.current = 0;

        // Start ping interval for latency measurement
        pingIntervalRef.current = setInterval(() => {
          if (ws.readyState === WebSocket.OPEN) {
            lastPingRef.current = Date.now();
            ws.send(JSON.stringify({ type: 'ping', timestamp: lastPingRef.current }));
          }
        }, 10000);
      };

      ws.onmessage = (event) => {
        try {
          const data = JSON.parse(event.data) as WebSocketMessage;

          // A pong answers our ping; a heartbeat is unsolicited and carries no latency
          if (data.type === 'pong') {
            setLatency(Date.now() - lastPingRef.current);
            return;
          }
          if (data.type === 'heartbeat') {
            return;
          }

          // Handle connection confirmation
          if (data.type === 'connected') {
            const sid = data.session_id || 'unknown';
            setSessionId(sid);
            onConnect?.(sid);
            return;
          }

          // Handle subscription confirmation
          if (data.type === 'subscribed' || data.type === 'unsubscribed') {
            return;
          }

          // Pass valuations and errors to callback
          onMessage?.(data);
        } catch (e) {
          console.error('Failed to parse WebSocket message:', e);
        }
      };

      ws.onclose = () => {
        // A socket already replaced or deliberately closed must not touch the current state
        if (wsRef.current !== ws) return;
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

  // Subscribe to runtime valuations
  const subscribeValuations = useCallback((instrumentIds: string[]) => {
    sendMessage({
      type: 'subscribe_valuations',
      instrument_ids: instrumentIds,
    });
  }, [sendMessage]);

  // Unsubscribe from runtime valuations
  const unsubscribeValuations = useCallback((instrumentIds: string[]) => {
    sendMessage({
      type: 'unsubscribe_valuations',
      instrument_ids: instrumentIds,
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
    subscribeValuations,
    unsubscribeValuations,
    sendMessage,
  };
}

export default useWebSocket;
