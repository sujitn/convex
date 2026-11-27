import { codeWords } from './format'
import type { LimitStatus, UnknownReason } from './limits'
import { bookTopicSubscription, followTopic, type TopicHandlers } from './stream'

// The book screen's model: each followed book's latest summary from the `book` topic, and
// its P&L since the close by position. Pure, so the merge and the wording are tested apart
// from the screen.

/** The books one subscription names, as a session follows them. */
export const MAX_SUMMARY_BOOKS = 64

/** A risk measure: its value, or why it is not known. */
export type RiskMeasure = { status: 'known'; value: string } | { status: 'unknown'; reason: UnknownReason }

export type BookRisk =
  | {
      status: 'measured'
      market_value: RiskMeasure
      gross_market_value: RiskMeasure
      dv01: RiskMeasure
      cs01: RiskMeasure
      modified_duration: RiskMeasure
      key_rate_dv01: (RiskMeasure & { tenor: string })[]
    }
  | { status: 'unknown'; reason: UnknownReason }

/** Why a book has no P&L since its close. */
export type NoBookPnlReason =
  | 'no_close'
  | 'book_currency_required'
  | 'book_currency_changed'
  | 'settlement_date_unavailable'
  | 'close_settles_later'
  | 'refused'

export type BookPnl =
  | {
      status: 'measured'
      close_version: number
      close_date: string
      pnl: string
      entitled_cash: string
      excluded_count: number
    }
  | { status: 'unknown'; reason: NoBookPnlReason }

/** A `book_summary`: one evaluation of a watched book. */
export interface BookSummary {
  book_id: string
  book_version: number
  limit_set_version?: number
  sequence: number
  monitor_started_at: string
  evaluated_at: string
  currency?: string
  settlement_date?: string
  risk: BookRisk
  pnl: BookPnl
  /** Each limit's status of the moment, its value and use. */
  limits: { limit_id: string; status: LimitStatus; value?: string; utilisation?: string }[]
}

/** Why a position has no P&L since the close. */
export type NoPnlReason =
  | 'added_since_close'
  | 'removed_since_close'
  | 'side_changed'
  | 'unvalued_at_close'
  | 'unvalued_now'
  | 'cash_not_projected'

/** A position held at the close or now, as `GET .../pnl` lists it. */
export interface PositionPnl {
  instrument_id: string
  close_face: string
  close_dirty_price?: string
  face: string
  dirty_price?: string
  held_face?: string
  pnl?: string
  entitled_cash?: string
  no_pnl?: NoPnlReason
}

/** P&L since the latest close, by position, as `GET .../pnl` answers. */
export interface BookPnlDetail {
  close: { version: number; close_date: string }
  currency: string
  pnl: string
  entitled_cash: string
  excluded_count: number
  positions: PositionPnl[]
}

/** A close as `POST .../close` answers; only what the screen reports is read. */
export interface BookClose {
  version: number
  close_date: string
  positions: { value: { status: 'valued' | 'unvalued' } }[]
}

/**
 * Whether `summary` is later than `held`: one from a later start of the monitor, or from
 * the same start with a higher sequence. A summary sent at subscription can be newer than
 * one still on its way, so the order they arrive in says nothing.
 */
function isLaterSummary(summary: BookSummary, held: BookSummary | undefined): boolean {
  if (held === undefined) return true
  const started = Date.parse(summary.monitor_started_at) - Date.parse(held.monitor_started_at)
  return started > 0 || (started === 0 && summary.sequence > held.sequence)
}

/** Keep `summary` in `held` if it is the later of its book. */
export function holdLater(held: Map<string, BookSummary>, summary: BookSummary): void {
  if (isLaterSummary(summary, held.get(summary.book_id))) held.set(summary.book_id, summary)
}

/** `summaries` with each of `arrived` kept that is the later of its book; the same map when none is. */
export function withSummaries(
  summaries: ReadonlyMap<string, BookSummary>,
  arrived: Iterable<BookSummary>,
): ReadonlyMap<string, BookSummary> {
  let next: Map<string, BookSummary> | null = null
  for (const summary of arrived) {
    if (!isLaterSummary(summary, (next ?? summaries).get(summary.book_id))) continue
    next ??= new Map(summaries)
    next.set(summary.book_id, summary)
  }
  return next ?? summaries
}

/** `summaries` of `bookIds` only. */
export function summariesOn(summaries: ReadonlyMap<string, BookSummary>, bookIds: readonly string[]): ReadonlyMap<string, BookSummary> {
  const followed = new Set(bookIds)
  return new Map([...summaries].filter(([bookId]) => followed.has(bookId)))
}

/** The `subscribe_books` message for `bookIds`, at most [`MAX_SUMMARY_BOOKS`]. */
export function bookSubscription(bookIds: readonly string[]): string {
  return bookTopicSubscription('subscribe_books', bookIds, MAX_SUMMARY_BOOKS)
}

/**
 * Follow the summaries of `bookIds` until the returned function is called. Subscribing
 * sends each book's latest summary first, so a reconnection subscribes again and the later
 * summary of each book is kept.
 */
export function followBooks(token: string | null, bookIds: readonly string[], handlers: TopicHandlers<BookSummary>): () => void {
  if (bookIds.length === 0) return () => {}
  return followTopic(token, [bookSubscription(bookIds)], 'book_summary', handlers)
}

/** Why a book has no P&L, in a line. */
export function noBookPnlText(reason: NoBookPnlReason): string {
  return reason === 'no_close' ? 'no close yet' : codeWords(reason)
}

/** A close as the screen reports it once taken. */
export function closeTaken(bookId: string, close: BookClose): string {
  const unvalued = close.positions.filter((position) => position.value.status === 'unvalued').length
  const valued = close.positions.length - unvalued
  return `${bookId} closed for ${close.close_date} (version ${close.version}): ${valued} valued${unvalued > 0 ? `, ${unvalued} unvalued` : ''}`
}
