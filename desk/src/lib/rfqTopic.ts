import { failureStatus } from './failure'
import { isOpen, rowFromRecords, type RfqRecord, type RfqRow } from './rfqs'
import { openStream, type Send, type StreamMessage, type StreamStatus } from './stream'

// The `rfq` topic: each open request for quote on the followed books, then every
// transition, as the list's row with its entry's `record_id`.

/** The books a session follows on the topic, and so one subscription names. */
export const MAX_RFQ_BOOKS = 64
/** How long after a request could not be read again the reader subscribes again to retry. */
const READ_AGAIN_AFTER_MS = 5000

export interface RfqTopicHandlers {
  /** A row later than any given of its request. */
  onRow: (row: RfqRow) => void
  /** Requests read again that the identity may no longer read, or the server no longer knows. */
  onGone: (rfqKeys: string[]) => void
  /** A request on `bookId` the session may not read, sent in its place. */
  onWithheld: (bookId: string) => void
  /** A refusal, such as a subscription to a book the session may not follow. */
  onRefusal: (code: string, message: string) => void
  /** A request could not be read again; it stays as last given until a later subscription reads it. */
  onReadFailure: (error: unknown) => void
  onStatus: (status: StreamStatus) => void
  /** Every entry of a request, in order. */
  readRecords: (rfqKey: string) => Promise<RfqRecord[]>
}

/** The `subscribe_rfqs` message for `bookIds`, at most [`MAX_RFQ_BOOKS`]. */
export function rfqSubscription(bookIds: readonly string[]): string {
  if (bookIds.length > MAX_RFQ_BOOKS) throw new Error(`a session follows at most ${MAX_RFQ_BOOKS} books`)
  return JSON.stringify({ type: 'subscribe_rfqs', book_ids: bookIds })
}

/**
 * The topic as one page reads it, each later row of a request given once. Subscribing
 * sends each open request on the books, by deadline, then the acknowledgement. The server
 * sends the two together, after any `RESYNC_REQUIRED`, so only rows before one may be
 * missing later ones: a request named after the last `RESYNC_REQUIRED` before the
 * acknowledgement is covered, and each other request held open may have moved, or ended,
 * unseen, so it is read again from its entries. A `RESYNC_REQUIRED` outside a subscription
 * subscribes again, which costs the server nothing.
 */
export class RfqTopicReader {
  /** Each request given that is open, as last given. */
  private readonly open = new Map<string, RfqRow>()
  /** The entry ID each request given final ended at; nothing follows it. */
  private readonly ended = new Map<string, number>()
  /** The requests named since the subscription in flight was sent; none when none is. */
  private covered: Set<string> | null = null
  /** A subscription owed because a request could not be read again. */
  private retry: ReturnType<typeof setTimeout> | undefined

  constructor(
    private readonly bookIds: readonly string[],
    private readonly handlers: RfqTopicHandlers,
    held: Iterable<RfqRow> = [],
  ) {
    for (const row of held) this.remember(row)
  }

  subscribe(send: Send): void {
    clearTimeout(this.retry)
    this.retry = undefined
    this.covered = new Set()
    send(rfqSubscription(this.bookIds))
  }

  /** Owe no subscription: the topic is no longer followed. */
  stop(): void {
    clearTimeout(this.retry)
  }

  /** Read one message; an acknowledgement's catch-up is returned to be awaited. */
  read(message: StreamMessage, send: Send): Promise<void> | undefined {
    switch (message.type) {
      case 'rfq': {
        const row = message as unknown as RfqRow
        this.covered?.add(row.rfq_key)
        this.give(row)
        return
      }
      case 'rfq_withheld':
        this.handlers.onWithheld(String(message.book_id))
        return
      case 'subscribed': {
        const covered = this.covered
        if (message.subscription_type !== 'rfqs' || covered === null) return
        this.covered = null
        return this.catchUp(covered, send)
      }
      case 'error':
        if (message.code === 'RESYNC_REQUIRED') {
          if (this.covered) this.covered.clear()
          else this.subscribe(send)
          return
        }
        // While a subscription is in flight, an error is its refusal.
        this.covered = null
        this.handlers.onRefusal(message.code ?? 'ERROR', message.message ?? '')
    }
  }

  /**
   * Read again each request held open that `covered` does not name. One that cannot be read
   * now is read by a subscription after a pause, whose seed costs nothing.
   */
  private async catchUp(covered: ReadonlySet<string>, send: Send): Promise<void> {
    const gone: string[] = []
    let failure: unknown
    const unread = [...this.open.values()].filter((row) => !covered.has(row.rfq_key))
    await Promise.all(
      unread.map(async (row) => {
        try {
          this.give(rowFromRecords(row, await this.handlers.readRecords(row.rfq_key)))
        } catch (error) {
          const status = failureStatus(error)
          if (status !== 403 && status !== 404) {
            failure = error
            return
          }
          this.open.delete(row.rfq_key)
          gone.push(row.rfq_key)
        }
      }),
    )
    if (gone.length > 0) this.handlers.onGone(gone)
    if (failure !== undefined) {
      this.handlers.onReadFailure(failure)
      this.retry ??= setTimeout(() => this.subscribe(send), READ_AGAIN_AFTER_MS)
    }
  }

  /** Give `row` if it is later than any given of its request. */
  private give(row: RfqRow): void {
    if (this.remember(row)) this.handlers.onRow(row)
  }

  /** Whether `row` is later than any held of its request; if so it is now the latest. */
  private remember(row: RfqRow): boolean {
    const latest = this.open.get(row.rfq_key)?.record_id ?? this.ended.get(row.rfq_key)
    if (latest !== undefined && (this.ended.has(row.rfq_key) || row.record_id <= latest)) return false
    if (isOpen(row.state)) {
      this.open.set(row.rfq_key, row)
    } else {
      this.open.delete(row.rfq_key)
      this.ended.set(row.rfq_key, row.record_id)
    }
    return true
  }
}

/**
 * Follow the requests on `bookIds` until the returned function is called, from the rows
 * `held` already. Each connection, the first and each after a close, subscribes, and reads
 * again each request held open that its subscription does not cover.
 */
export function followRfqs(
  token: string | null,
  bookIds: readonly string[],
  handlers: RfqTopicHandlers,
  held: Iterable<RfqRow> = [],
): () => void {
  if (bookIds.length === 0) return () => {}
  const reader = new RfqTopicReader(bookIds, handlers, held)
  const close = openStream(token, {
    onOpen: (send) => reader.subscribe(send),
    onMessage: (message, send) => void reader.read(message, send),
    onStatus: handlers.onStatus,
  })
  return () => {
    reader.stop()
    close()
  }
}
