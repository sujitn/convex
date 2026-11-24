import type { PublishedValuation } from '@convex/api-client'
import { observeServerTime } from './clock'

// The stream, `/api/v1/ws`: Live valuations, and requests for quote by book. A browser
// cannot send `Authorization` on a WebSocket, so the page offers its token as a
// subprotocol beside the stream's own, which the server selects. The token never goes in
// the URL.

/** The stream's protocol, which the server selects. */
const STREAM_PROTOCOL = 'convex.v1'
/** The subprotocol a token is sent in, base64url without padding. */
const BEARER_PROTOCOL_PREFIX = 'convex.bearer.'
/** Instruments one subscription message names, as the server takes them. */
export const MAX_SUBSCRIPTION_INSTRUMENTS = 256
/** The longest wait between reconnection attempts. */
const MAX_RECONNECT_MS = 30_000

/** The subprotocols a page offers: the stream's own, and its token when it holds one. */
export function streamProtocols(token: string | null): string[] {
  if (!token) return [STREAM_PROTOCOL]
  const encoded = btoa(token).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')
  return [STREAM_PROTOCOL, `${BEARER_PROTOCOL_PREFIX}${encoded}`]
}

/** `subscribe_valuations` messages naming every instrument, at most 256 each. */
export function subscriptionMessages(instrumentIds: string[]): string[] {
  const messages: string[] = []
  for (let start = 0; start < instrumentIds.length; start += MAX_SUBSCRIPTION_INSTRUMENTS) {
    const instrument_ids = instrumentIds.slice(start, start + MAX_SUBSCRIPTION_INSTRUMENTS)
    messages.push(JSON.stringify({ type: 'subscribe_valuations', instrument_ids }))
  }
  return messages
}

export type StreamStatus = 'connecting' | 'open' | 'reconnecting'

/** A message the stream sent: its type, and the fields of that type. */
export interface StreamMessage {
  type: string
  code?: string
  message?: string
  [field: string]: unknown
}

/** Send one message on the open connection. */
export type Send = (message: string) => void

export interface ConnectionHandlers {
  /** The connection opened, or opened again: subscribe. */
  onOpen: (send: Send) => void
  onMessage: (message: StreamMessage, send: Send) => void
  onStatus: (status: StreamStatus) => void
}

/**
 * Connect to the stream until the returned function is called, connecting again after
 * each close, at most 30 seconds apart. Each connection starts with no subscription, so
 * `onOpen` subscribes each time.
 */
export function openStream(token: string | null, handlers: ConnectionHandlers): () => void {
  let socket: WebSocket | null = null
  let stopped = false
  let attempts = 0
  let retry: ReturnType<typeof setTimeout> | undefined
  // A message for a connection not open is dropped: the next one subscribes when it opens.
  const send: Send = (message) => {
    if (socket?.readyState === WebSocket.OPEN) socket.send(message)
  }
  const connect = () => {
    handlers.onStatus(attempts === 0 ? 'connecting' : 'reconnecting')
    const scheme = window.location.protocol === 'https:' ? 'wss' : 'ws'
    socket = new WebSocket(`${scheme}://${window.location.host}/api/v1/ws`, streamProtocols(token))
    socket.onopen = () => {
      if (stopped) return
      attempts = 0
      handlers.onStatus('open')
      handlers.onOpen(send)
    }
    socket.onmessage = (event: MessageEvent<string>) => {
      if (stopped) return
      const message = JSON.parse(event.data) as StreamMessage
      // The connection and each heartbeat carry the server's clock.
      if (typeof message.server_time === 'number') observeServerTime(message.server_time)
      handlers.onMessage(message, send)
    }
    socket.onclose = () => {
      if (stopped) return
      attempts += 1
      handlers.onStatus('reconnecting')
      retry = setTimeout(connect, Math.min(MAX_RECONNECT_MS, 1000 * 2 ** Math.min(attempts - 1, 5)))
    }
  }
  connect()
  return () => {
    stopped = true
    clearTimeout(retry)
    socket?.close()
  }
}

export interface StreamHandlers {
  onValuation: (valuation: PublishedValuation) => void
  onStatus: (status: StreamStatus) => void
  /** A refusal the stream sent, such as `FORBIDDEN` or `RESULT_TOO_LARGE`. */
  onRefusal: (code: string, message: string) => void
}

/**
 * Follow `instrumentIds` until the returned function is called. Subscribing sends each
 * instrument's current result first, so a reconnection, or a `RESYNC_REQUIRED` after the
 * session fell behind, subscribes again and the newer result of each wins.
 */
export function followValuations(
  token: string | null,
  instrumentIds: string[],
  handlers: StreamHandlers,
): () => void {
  if (instrumentIds.length === 0) return () => {}
  const subscribe = (send: Send) => {
    for (const message of subscriptionMessages(instrumentIds)) send(message)
  }
  return openStream(token, {
    onOpen: subscribe,
    onStatus: handlers.onStatus,
    onMessage: (message, send) => {
      if (message.type === 'valuation') {
        handlers.onValuation(message as unknown as PublishedValuation)
      } else if (message.type === 'error' && message.code === 'RESYNC_REQUIRED') {
        subscribe(send)
      } else if (message.type === 'error') {
        handlers.onRefusal(message.code ?? 'ERROR', message.message ?? '')
      }
    },
  })
}
