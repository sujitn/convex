import type { PublishedValuation } from '@convex/api-client'
import { observeServerTime } from './clock'

// The stream, `/api/v1/ws`: Live valuations, and requests for quote and summaries by book. A browser
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

export interface TopicHandlers<T> {
  /** An item the topic sent, its latest when sent; one sent at subscription can be newer than one still on its way, so the caller keeps the later. */
  onItem: (item: T) => void
  onStatus: (status: StreamStatus) => void
  /** A refusal the stream sent, such as `FORBIDDEN` or `RESULT_TOO_LARGE`. */
  onRefusal: (code: string, message: string) => void
}

/**
 * Read one message of a topic that sends each subscribed item's latest first: an item of
 * `itemType` is given, a `RESYNC_REQUIRED` subscribes again, which costs the server
 * nothing, and any other error is a refusal.
 */
export function readTopicMessage<T>(
  message: StreamMessage,
  send: Send,
  subscription: readonly string[],
  itemType: string,
  handlers: TopicHandlers<T>,
): void {
  if (message.type === itemType) handlers.onItem(message as unknown as T)
  else if (message.type === 'error' && message.code === 'RESYNC_REQUIRED') for (const sent of subscription) send(sent)
  else if (message.type === 'error') handlers.onRefusal(message.code ?? 'ERROR', message.message ?? '')
}

/**
 * Follow a topic until the returned function is called: send `subscription` on each
 * connection, and give each item of `itemType` as it arrives. Subscribing sends each
 * item's latest first, so a reconnection, or a `RESYNC_REQUIRED` after the session fell
 * behind, subscribes again; the caller keeps the later of each.
 */
export function followTopic<T>(
  token: string | null,
  subscription: readonly string[],
  itemType: string,
  handlers: TopicHandlers<T>,
): () => void {
  if (subscription.length === 0) return () => {}
  return openStream(token, {
    onOpen: (send) => {
      for (const sent of subscription) send(sent)
    },
    onStatus: handlers.onStatus,
    onMessage: (message, send) => readTopicMessage(message, send, subscription, itemType, handlers),
  })
}

/** Follow the Live valuations of `instrumentIds` until the returned function is called. */
export function followValuations(
  token: string | null,
  instrumentIds: string[],
  handlers: TopicHandlers<PublishedValuation>,
): () => void {
  return followTopic(token, subscriptionMessages(instrumentIds), 'valuation', handlers)
}

/** The message that follows `bookIds` on a topic by book, at most `most`: the session's bound on it. */
export function bookTopicSubscription(type: string, bookIds: readonly string[], most: number): string {
  if (bookIds.length > most) throw new Error(`a session follows at most ${most} books`)
  return JSON.stringify({ type, book_ids: bookIds })
}
