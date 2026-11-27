import { countStatuses, type LimitStatus, type StatusCounts } from './limits'
import { openStream, type Send, type StreamMessage, type StreamStatus } from './stream'

// The book screen's model: each followed book's latest summary from the `book` topic, its
// limits as defined, and its P&L since the close by position. Pure, so the merge and the
// wording are tested apart from the screen.

/** The books one subscription names, as a session follows them. */
export const MAX_SUMMARY_BOOKS = 64

/** The instruments a reason names: the first few, and how many in all. */
export interface InstrumentList {
  count: number
  first: string[]
}

/** Why a book measure or limit is not known, as a limit on it gives the reason. */
export interface UnknownReason {
  code: string
  instruments?: InstrumentList
  instrument?: string
  measure?: string
  detail?: string
}

/** A risk measure: its value, or why it is not known. */
export type RiskMeasure = { status: 'known'; value: string } | { status: 'unknown'; reason: UnknownReason }

export type KeyRateDv01 = RiskMeasure & { tenor: string }

export type BookRisk =
  | {
      status: 'measured'
      market_value: RiskMeasure
      gross_market_value: RiskMeasure
      dv01: RiskMeasure
      cs01: RiskMeasure
      modified_duration: RiskMeasure
      key_rate_dv01: KeyRateDv01[]
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

/** A limit's status of the moment, its value and utilisation. */
export interface LimitStatusNow {
  limit_id: string
  status: LimitStatus
  value?: string
  utilisation?: string
}

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
  limits: LimitStatusNow[]
}

/** The members a limit bounds: each of them, or one. */
export type LimitMember = 'each' | { one: string }

/** What a limit bounds. */
export type LimitMeasure =
  | { kind: 'face'; instrument: LimitMember }
  | { kind: 'market_value' | 'gross_market_value' | 'dv01' | 'cs01' | 'modified_duration' }
  | { kind: 'key_rate_dv01'; tenor: string }
  | { kind: 'group_market_value' | 'group_share'; by: string; group: LimitMember }

/** One limit of a book's set. */
export interface LimitDefinition {
  limit_id: string
  measure: LimitMeasure
  lower?: string
  upper?: string
  warning_fraction?: string
}

/** A book's limit set, as `GET .../limits` answers. */
export interface LimitSet {
  version: number
  limits: LimitDefinition[]
}

/** One limit evaluated, as `GET .../limits/status` answers; only its reason is read here. */
export interface LimitEvaluation {
  limit_id: string
  status: LimitStatus
  reason?: UnknownReason
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

/** P&L since the latest close, as `GET .../pnl` answers. */
export interface BookPnlDetail {
  book_id: string
  close: { version: number; close_date: string; taken_at: string }
  currency: string
  evaluated_at: string
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
export function isLaterSummary(summary: BookSummary, held: BookSummary | undefined): boolean {
  if (held === undefined) return true
  const started = Date.parse(summary.monitor_started_at) - Date.parse(held.monitor_started_at)
  return started > 0 || (started === 0 && summary.sequence > held.sequence)
}

/** `summaries` with `summary` kept if it is the later of its book. */
export function withSummary(
  summaries: ReadonlyMap<string, BookSummary>,
  summary: BookSummary,
): ReadonlyMap<string, BookSummary> {
  if (!isLaterSummary(summary, summaries.get(summary.book_id))) return summaries
  return new Map(summaries).set(summary.book_id, summary)
}

/** `summaries` of `bookIds` only. */
export function onBooks(summaries: ReadonlyMap<string, BookSummary>, bookIds: readonly string[]): ReadonlyMap<string, BookSummary> {
  const followed = new Set(bookIds)
  return new Map([...summaries].filter(([bookId]) => followed.has(bookId)))
}

/** The `subscribe_books` message for `bookIds`, at most [`MAX_SUMMARY_BOOKS`]. */
export function bookSubscription(bookIds: readonly string[]): string {
  if (bookIds.length > MAX_SUMMARY_BOOKS) throw new Error(`a session follows at most ${MAX_SUMMARY_BOOKS} books`)
  return JSON.stringify({ type: 'subscribe_books', book_ids: bookIds })
}

export interface BookTopicHandlers {
  onSummary: (summary: BookSummary) => void
  /** A refusal, such as a summary the session may not read, or a server without a monitor. */
  onRefusal: (code: string, message: string) => void
  onStatus: (status: StreamStatus) => void
}

/** Read one message of the topic; a `RESYNC_REQUIRED` subscribes again, which costs nothing. */
export function readBookMessage(message: StreamMessage, send: Send, bookIds: readonly string[], handlers: BookTopicHandlers): void {
  if (message.type === 'book_summary') handlers.onSummary(message as unknown as BookSummary)
  else if (message.type === 'error' && message.code === 'RESYNC_REQUIRED') send(bookSubscription(bookIds))
  else if (message.type === 'error') handlers.onRefusal(message.code ?? 'ERROR', message.message ?? '')
}

/**
 * Follow the summaries of `bookIds` until the returned function is called. Subscribing
 * sends each book's latest summary first, so a reconnection subscribes again and the later
 * summary of each book is kept.
 */
export function followBooks(token: string | null, bookIds: readonly string[], handlers: BookTopicHandlers): () => void {
  if (bookIds.length === 0) return () => {}
  return openStream(token, {
    onOpen: (send) => send(bookSubscription(bookIds)),
    onStatus: handlers.onStatus,
    onMessage: (message, send) => readBookMessage(message, send, bookIds, handlers),
  })
}

/** How many of a summary's limits are in each status. */
export function limitCounts(summary: BookSummary): StatusCounts {
  return countStatuses(summary.limits.map((limit) => limit.status))
}

/** A limit's member, as a label reads it. */
function memberLabel(member: LimitMember): string {
  return member === 'each' ? 'each' : member.one
}

/** What a limit bounds, as a desk names it. */
export function measureLabel(measure: LimitMeasure): string {
  switch (measure.kind) {
    case 'face':
      return `Face · ${memberLabel(measure.instrument)}`
    case 'market_value':
      return 'Market value'
    case 'gross_market_value':
      return 'Gross market value'
    case 'dv01':
      return 'DV01'
    case 'cs01':
      return 'CS01'
    case 'modified_duration':
      return 'Modified duration'
    case 'key_rate_dv01':
      return `Key-rate DV01 ${measure.tenor}`
    case 'group_market_value':
      return `${measure.by} market value · ${memberLabel(measure.group)}`
    case 'group_share':
      return `${measure.by} share · ${memberLabel(measure.group)}`
  }
}

/** A number of `decimals` places with thousands separators, signed when negative. */
function grouped(value: string, decimals: number): string {
  return Number(value).toLocaleString('en-US', { minimumFractionDigits: decimals, maximumFractionDigits: decimals })
}

/** An amount in the book's currency, or a face, in whole units. */
export function formatAmount(value: string): string {
  return grouped(value, 0)
}

/** A fraction (0.8512) as percent at one decimal (85.1%). */
export function formatPercent(value: string): string {
  return `${(Number(value) * 100).toFixed(1)}%`
}

/** A limit's value or bound in the measure's unit: a share in percent, a duration in years, else whole units. */
export function formatLimitValue(measure: LimitMeasure, value: string): string {
  if (measure.kind === 'group_share') return formatPercent(value)
  if (measure.kind === 'modified_duration') return grouped(value, 2)
  return formatAmount(value)
}

/** A limit's bounds, as a cell reads them. */
export function boundsLabel(limit: LimitDefinition): string {
  const shown = (bound: string) => formatLimitValue(limit.measure, bound)
  if (limit.lower !== undefined && limit.upper !== undefined) return `${shown(limit.lower)} to ${shown(limit.upper)}`
  if (limit.upper !== undefined) return `≤ ${shown(limit.upper)}`
  if (limit.lower !== undefined) return `≥ ${shown(limit.lower)}`
  return 'unbounded'
}

/** Words for a snake-case code. */
function words(code: string): string {
  return code.replace(/_/g, ' ')
}

/** Why a measure or limit is not known, in a line. */
export function unknownReasonText(reason: UnknownReason): string {
  const parts = [words(reason.code)]
  if (reason.measure) parts.push(reason.measure)
  if (reason.instruments) {
    const { count, first } = reason.instruments
    parts.push(`${count} position${count === 1 ? '' : 's'} (${first.join(', ')}${count > first.length ? ', …' : ''})`)
  }
  if (reason.instrument) parts.push(reason.instrument)
  if (reason.detail) parts.push(reason.detail)
  return parts.join(': ')
}

/** Why a book has no P&L, in a line. */
export function noBookPnlText(reason: NoBookPnlReason): string {
  return reason === 'no_close' ? 'no close yet' : words(reason)
}

/** Why a position has no P&L, in a line. */
export function noPnlText(reason: NoPnlReason): string {
  return words(reason)
}

/** A close as the screen reports it once taken. */
export function closeTaken(bookId: string, close: BookClose): string {
  const unvalued = close.positions.filter((position) => position.value.status === 'unvalued').length
  const valued = close.positions.length - unvalued
  return `${bookId} closed for ${close.close_date} (version ${close.version}): ${valued} valued${unvalued > 0 ? `, ${unvalued} unvalued` : ''}`
}
