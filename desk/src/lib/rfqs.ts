import type { QuoteInput, QuoteValue } from '@convex/api-client'
import { addDecimal, roundToTick } from './decimal'
import { formatThirtySeconds } from './format'
import { isQuotedInThirtySeconds, type Bond } from './sheet'

// The RFQ blotter's model: each request for quote on the followed books as the `rfq` topic
// last showed it, in the order a trader works them, with what may be done with each. Pure,
// so the merge, the catch-up, the sent price and the keys are tested apart from the screen.

export type RfqState =
  | 'received'
  | 'priced'
  | 'quoted'
  | 'done'
  | 'traded_away'
  | 'covered'
  | 'passed'
  | 'expired'
  | 'cancelled'

/** A limit's status, from the least severe to the most, as the server orders them. */
const LIMIT_STATUSES = ['within', 'warning', 'unknown', 'breach'] as const
export type LimitStatus = (typeof LIMIT_STATUSES)[number]

/** A request as its source sent it. */
export interface RfqRequest {
  source_id: string
  rfq_id: string
  client_id: string
  client_tier: string
  /** The bond by its id, unless the source named it by `identifier`. */
  instrument_id?: string
  identifier?: { kind: string; value: string }
  side: 'buy' | 'sell'
  face: string
  settlement_date: string
  book_id: string
  deadline: string
}

/** The server's price on the side of the desk's quote the client trades on, or why it has none. */
export type AutoPrice =
  | {
      kind: 'priced'
      desk_side: 'bid' | 'ask'
      clean_price: string
      /** The desk side's level, for a bond quoted by one. */
      level: QuoteValue | null
      clean_price_mid: string
      /** Why the price is the mid: the bond has no two-way width. */
      mid_only: string | null
    }
  | { kind: 'unpriced'; reason: string }

/** The pre-trade check: how many of the book's limits it put in each status, or why it did not run. */
export type LimitCheck =
  | { kind: 'evaluated'; statuses: Partial<Record<LimitStatus, number>> }
  | { kind: 'unavailable'; reason: string }

export interface SentQuote {
  clean_price: string
  quoted_at: string
}

/** A request as the list and the topic show it, as its entry `record_id` left it. */
export interface RfqRow {
  rfq_key: string
  record_id: number
  state: RfqState
  request: RfqRequest
  /** The bond, as the server resolved it at receipt. */
  instrument_id: string
  instrument_version: number
  received_at: string
  auto_price: AutoPrice
  check: LimitCheck
  quote: SentQuote | null
}

/** The configuration a request was priced under, and its client tier's factor there. */
export interface PricedUnder {
  configuration_id: string
  version: number
  activation_revision: number
  client_tier: string
  width_factor: string
}

/** The rule as the auto price read it: its model, version and method, and what its stage read. */
export interface RuleRead {
  model: string
  version: number
  method: { kind: string; tenor?: string }
  benchmark_set?: {
    benchmark_set_id: string
    version: number
    securities: { tenor: string; instrument_id: string; version: number; clean_price_mid: string }[]
  }
  benchmark?: { tenor: string; instrument_id: string; ytm: number | string }
  two_way?: { unit: string; width: string; bid: string; ask: string }
  g_spread_refused?: string
}

/** The auto price as its entry records it, with what it was made from. */
export type RecordedAutoPrice =
  | {
      kind: 'priced'
      desk_side: 'bid' | 'ask'
      clean_price: string
      level?: QuoteValue
      clean_price_mid: string
      mid_only?: string
      quote_input?: QuoteInput
      pricing_rule?: RuleRead
      curves: { curve_id: string; version: number }[]
    }
  | { kind: 'unpriced'; reason: string }

/** One limit as the check evaluated it with the trade done. */
export interface CheckedLimit {
  limit_id: string
  measure: { kind: string }
  status: LimitStatus
  value?: string
  member?: string
  lower?: string
  upper?: string
  reason?: { code: string }
}

/** The check as its entry records it: each limit, or why it did not run. */
export type RecordedCheck = { kind: 'evaluated'; status: { limits: CheckedLimit[] } } | { kind: 'unavailable'; reason: string }

/** One entry of `GET /api/v1/rfqs/:rfq_key`: the state it moved the request to, who sent it, and what it carries. */
export type RfqRecord = {
  record_id: number
  actor: 'server' | 'trader' | 'source'
  user: string | null
  recorded_at: string
} & (
  | { state: 'received'; request: RfqRequest; instrument_id: string; instrument_version: number; received_at: string }
  | { state: 'priced'; configuration: PricedUnder; auto_price: RecordedAutoPrice; check: RecordedCheck; priced_at: string }
  | ({ state: 'quoted' } & SentQuote)
  | { state: 'expired'; reason: 'deadline' | 'restart' }
  | { state: 'done' | 'traded_away' | 'covered' | 'passed' | 'cancelled' }
)

/** The final requests the blotter keeps beside the open ones, the latest. */
export const MAX_FINAL_ROWS = 100

/** Whether nothing final has happened to the request yet. */
export function isOpen(state: RfqState): boolean {
  return state === 'received' || state === 'priced' || state === 'quoted'
}

/**
 * `rows` with each of `incoming` in, unless a later row of its request is in already: of
 * two rows of one request the one with the higher `record_id` is newer, since a request
 * sent at subscription can be newer than a transition still on its way. Only the latest
 * [`MAX_FINAL_ROWS`] final requests are kept.
 */
export function withRows(rows: ReadonlyMap<string, RfqRow>, incoming: readonly RfqRow[]): ReadonlyMap<string, RfqRow> {
  let next: Map<string, RfqRow> | null = null
  for (const row of incoming) {
    const current = (next ?? rows).get(row.rfq_key)
    if (current && current.record_id >= row.record_id) continue
    next ??= new Map(rows)
    next.set(row.rfq_key, row)
  }
  if (next === null) return rows
  const finals = [...next.values()].filter((row) => !isOpen(row.state))
  if (finals.length > MAX_FINAL_ROWS) {
    finals.sort((a, b) => b.record_id - a.record_id)
    for (const row of finals.slice(MAX_FINAL_ROWS)) next.delete(row.rfq_key)
  }
  return next
}

/** `rows` without the requests on books no longer followed. */
export function onBooks(rows: ReadonlyMap<string, RfqRow>, bookIds: readonly string[]): ReadonlyMap<string, RfqRow> {
  const followed = new Set(bookIds)
  const kept = [...rows.values()].filter((row) => followed.has(row.request.book_id))
  return kept.length === rows.size ? rows : new Map(kept.map((row) => [row.rfq_key, row]))
}

/** `rows` without `rfqKeys`. */
export function withoutRows(rows: ReadonlyMap<string, RfqRow>, rfqKeys: readonly string[]): ReadonlyMap<string, RfqRow> {
  if (!rfqKeys.some((key) => rows.has(key))) return rows
  const next = new Map(rows)
  for (const key of rfqKeys) next.delete(key)
  return next
}

/** The rows as the blotter lists them: the open requests by deadline, soonest first, then the final ones, latest first. */
export function blotterOrder(rows: ReadonlyMap<string, RfqRow>): RfqRow[] {
  const open: RfqRow[] = []
  const final: RfqRow[] = []
  for (const row of rows.values()) (isOpen(row.state) ? open : final).push(row)
  open.sort(
    (a, b) => Date.parse(a.request.deadline) - Date.parse(b.request.deadline) || a.rfq_key.localeCompare(b.rfq_key),
  )
  final.sort((a, b) => b.record_id - a.record_id)
  return [...open, ...final]
}

/** `row` as its entries leave it: the state and ID of the last, and the quote sent. */
export function rowFromRecords(row: RfqRow, records: readonly RfqRecord[]): RfqRow {
  const last = records.at(-1)
  if (!last) return row
  let quote = row.quote
  for (const record of records) {
    if (record.state === 'quoted') quote = { clean_price: record.clean_price, quoted_at: record.quoted_at }
  }
  return { ...row, state: last.state, record_id: last.record_id, quote }
}

/** The check's most severe status; none when it did not run or evaluated no limit. */
export function worstStatus(check: LimitCheck): LimitStatus | null {
  if (check.kind !== 'evaluated') return null
  for (let index = LIMIT_STATUSES.length - 1; index >= 0; index -= 1) {
    const status = LIMIT_STATUSES[index]!
    if ((check.statuses[status] ?? 0) > 0) return status
  }
  return null
}

/** The check as a cell reads it: each status with its count, the most severe first, or why there is none. */
export function checkSummary(check: LimitCheck): string {
  if (check.kind !== 'evaluated') return 'no check'
  const counted = [...LIMIT_STATUSES]
    .reverse()
    .filter((status) => (check.statuses[status] ?? 0) > 0)
    .map((status) => `${status} ${check.statuses[status]}`)
  return counted.length > 0 ? counted.join(' · ') : 'no limits'
}

/** A thousandth of a point, a corporate bond's tick. */
const PRICE_TICK = '0.001'
/** A 256th of a point, the finest 32nds a Treasury is shown in. */
export const TREASURY_TICK = '0.00390625'

/**
 * The tick a bond's price is quoted on: a Treasury's 256th, any other's thousandth. None
 * while the bond is unread, or if it cannot be read, since whether it is a Treasury is then
 * unknown.
 */
export function quotingTick(bond: Bond | null | undefined): string | null {
  if (!bond) return null
  return isQuotedInThirtySeconds(bond) ? TREASURY_TICK : PRICE_TICK
}

/**
 * The auto price on `tick`, in the desk's favour, so never tighter than priced: a bid
 * rounds down, an ask up. A mid is rounded as its side would be. None when unpriced.
 */
export function tickedPrice(autoPrice: AutoPrice, tick: string): string | null {
  if (autoPrice.kind !== 'priced') return null
  return roundToTick(autoPrice.clean_price, tick, autoPrice.desk_side === 'bid' ? 'down' : 'up')
}

/**
 * The price Send quotes on one key: the ticked auto price of a two-way. None for a mid or an
 * unpriced request, whose price the trader sets.
 */
export function sentPrice(autoPrice: AutoPrice, tick: string): string | null {
  return autoPrice.kind === 'priced' && autoPrice.mid_only === null ? tickedPrice(autoPrice, tick) : null
}

/**
 * `price` moved one tick, exactly, onto the tick: up lands on the tick above a typed price
 * off the tick, and down on the one below.
 */
export function nudgedPrice(price: string, tick: string, direction: 1 | -1): string {
  return roundToTick(addDecimal(price, direction < 0 ? `-${tick}` : tick), tick, direction < 0 ? 'up' : 'down')
}

/**
 * Whether the trader may answer `row` at `serverNow`: a priced request before its
 * deadline, once, while no answer to it is in flight.
 */
export function isAnswerable(row: RfqRow, serverNow: number, isAnswering: boolean): boolean {
  return row.state === 'priced' && !isAnswering && Date.parse(row.request.deadline) > serverNow
}

/** Why `row`, if it is not in flight, cannot be answered now: it is gone, has moved, or is past its deadline. */
export function unanswerable(row: RfqRow | undefined): string {
  if (!row) return 'the request is no longer on the blotter'
  if (row.state !== 'priced') return `the request is ${stateLabel(row.state)}`
  return 'the request is past its deadline'
}

/** Whole seconds to the deadline at `serverNow`; zero once it has passed. */
export function secondsLeft(deadline: string, serverNow: number): number {
  return Math.max(0, Math.floor((Date.parse(deadline) - serverNow) / 1000))
}

/** A countdown as `42s`, or `4:05` from a minute. */
export function formatCountdown(seconds: number): string {
  if (seconds < 60) return `${seconds}s`
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`
}

/** A face amount as a desk sizes a trade: thousands below a million (`250K`), millions from one (`5MM`, `2.5MM`). */
export function formatFace(face: string): string {
  const amount = Number(face)
  if (amount < 1_000_000) return `${Number((amount / 1000).toFixed(3))}K`
  return `${Number((amount / 1_000_000).toFixed(3))}MM`
}

/** A price to quote, as the desk shows it: a Treasury's in 32nds, any other as it is sent. */
export function formatQuotedPrice(price: string, thirtySeconds: boolean): string {
  return thirtySeconds ? formatThirtySeconds(price) : price
}

/** A state as a cell reads it. */
export function stateLabel(state: RfqState): string {
  return state.replace('_', ' ')
}

/** The book ids a typed list names, once each, in order: separated by commas or spaces. */
export function bookList(text: string): string[] {
  return [...new Set(text.split(/[\s,]+/).filter((id) => id.length > 0))]
}
