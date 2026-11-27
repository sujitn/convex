import { useQueries } from '@tanstack/react-query'
import { useSession } from '@convex/api-client'
import { memo, useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent } from 'react'
import { BookPicker } from '../components/BookPicker'
import { StreamBadge } from '../components/StreamBadge'
import { serverNow, useServerNow } from '../lib/clock'
import { isPositiveDecimal } from '../lib/decimal'
import { failureNotice, failureStatus } from '../lib/failure'
import { formatQuoteValue, formatThirtySeconds } from '../lib/format'
import { useFollowedBooks } from '../lib/followedBooks'
import { LIMIT_TONE } from '../lib/limits'
import { readBond, readRfqRecords } from '../lib/reads'
import {
  blotterOrder,
  checkSummary,
  formatCountdown,
  formatFace,
  formatQuotedPrice,
  isAnswerable,
  isOpen,
  nudgedPrice,
  onBooks,
  secondsLeft,
  sentPrice,
  stateLabel,
  tickedPrice,
  TREASURY_TICK,
  unanswerable,
  quotingTick,
  withoutRows,
  withRows,
  worstStatus,
  type RfqRow,
  type RfqState,
} from '../lib/rfqs'
import { followRfqs, MAX_RFQ_BOOKS } from '../lib/rfqTopic'
import { isQuotedInThirtySeconds, type Bond } from '../lib/sheet'
import type { StreamStatus } from '../lib/stream'
import { RfqDrawer } from './RfqDrawer'

// Each request for quote on the followed books, open ones by deadline, then the latest
// final ones. The keyboard selects a request and answers it: Send quotes the auto price on
// the tick in the desk's favour, Adjust a typed price, Pass declines. The topic brings the
// answer back as the request's next row.

type Rows = ReadonlyMap<string, RfqRow>

/** A row's bond: read, unreadable (null), or still being read (undefined). */
type BondRead = Bond | null | undefined

export function RfqBlotter() {
  const { call, token } = useSession()
  const books = useFollowedBooks(MAX_RFQ_BOOKS)
  const { bookIds } = books

  const [rows, setRows] = useState<Rows>(() => new Map())
  const [status, setStatus] = useState<StreamStatus | 'idle'>('idle')
  const [notice, setNotice] = useState<string | null>(null)
  const [withheld, setWithheld] = useState<ReadonlyMap<string, number>>(() => new Map())
  const [selectedKey, setSelectedKey] = useState<string | null>(null)
  const [editingKey, setEditingKey] = useState<string | null>(null)
  const [isDrawerOpen, setDrawerOpen] = useState(false)
  const blotter = useRef<HTMLDivElement>(null)

  // Requests with an answer sent, until the topic shows them moved; a ref, since a held
  // key repeats faster than the page renders, published for the rows to show.
  const answering = useRef(new Set<string>())
  const [answeringShown, setAnsweringShown] = useState<ReadonlySet<string>>(() => new Set())
  const setAnswering = useCallback((rfqKey: string, isAnswering: boolean) => {
    if (isAnswering) answering.current.add(rfqKey)
    else answering.current.delete(rfqKey)
    setAnsweringShown(new Set(answering.current))
  }, [])

  // The rows as merged, ahead of the render, so a new subscription starts from them.
  const rowsRef = useRef(rows)
  const update = useCallback((change: (current: Rows) => Rows) => {
    rowsRef.current = change(rowsRef.current)
    setRows(rowsRef.current)
  }, [])

  // Rows and withheld requests arrive in bursts, a seed or a timer's expiries; they are
  // merged once a frame.
  const pendingRows = useRef<RfqRow[]>([])
  const pendingWithheld = useRef(new Map<string, number>())
  const frame = useRef<number | null>(null)
  const flush = useCallback(() => {
    frame.current = null
    const arrived = pendingRows.current
    pendingRows.current = []
    if (arrived.length > 0) update((current) => withRows(current, arrived))
    const counted = pendingWithheld.current
    pendingWithheld.current = new Map()
    if (counted.size > 0) {
      setWithheld((current) => {
        const next = new Map(current)
        for (const [bookId, count] of counted) next.set(bookId, (next.get(bookId) ?? 0) + count)
        return next
      })
    }
  }, [update])

  useEffect(() => {
    // Rows still to merge are merged first, so a book no longer followed leaves with them.
    flush()
    update((current) => onBooks(current, bookIds))
    setWithheld(new Map())
    const schedule = () => {
      frame.current ??= requestAnimationFrame(flush)
    }
    return followRfqs(
      token,
      bookIds,
      {
        onRow: (row) => {
          pendingRows.current.push(row)
          schedule()
        },
        onGone: (rfqKeys) => {
          // Rows still to merge are merged first, so none of these comes back with them.
          flush()
          update((current) => withoutRows(current, rfqKeys))
        },
        onWithheld: (bookId) => {
          pendingWithheld.current.set(bookId, (pendingWithheld.current.get(bookId) ?? 0) + 1)
          schedule()
        },
        onRefusal: (code, message) => setNotice(`${code}: ${message}`),
        onReadFailure: (error) => setNotice(`A request could not be read again: ${failureNotice(error)}`),
        onStatus: setStatus,
        readRecords: (rfqKey) => readRfqRecords(call, rfqKey),
      },
      rowsRef.current.values(),
    )
  }, [bookIds, token, call, update, flush])

  const ordered = useMemo(() => blotterOrder(rows), [rows])

  // The selection is a request, never a position, so a request arriving or ending moves no
  // key onto another. The first row is selected once; a selection that leaves the blotter
  // selects nothing, so no key lands on a request the trader did not choose.
  const hasSelected = useRef(false)
  useEffect(() => {
    if (selectedKey !== null && !rows.has(selectedKey)) setSelectedKey(null)
    else if (selectedKey === null && !hasSelected.current && ordered[0]) setSelectedKey(ordered[0].rfq_key)
    if (selectedKey !== null) hasSelected.current = true
  }, [ordered, rows, selectedKey])

  // An answer is in flight until the request moves on; an adjust closes once it cannot be sent.
  useEffect(() => {
    for (const rfqKey of answering.current) {
      if (rows.get(rfqKey)?.state !== 'priced') setAnswering(rfqKey, false)
    }
    if (editingKey !== null && rows.get(editingKey)?.state !== 'priced') setEditingKey(null)
  }, [rows, editingKey, setAnswering])

  const instrumentIds = useMemo(() => [...new Set([...rows.values()].map((row) => row.instrument_id))].sort(), [rows])
  const bondReads = useQueries({
    queries: instrumentIds.map((instrumentId) => ({
      queryKey: ['bond', instrumentId],
      queryFn: () => readBond(call, instrumentId),
      staleTime: Infinity,
      // A bond read is needed for its tick: one refused for a while is read again, one the
      // identity may not read or the server does not hold is not.
      retry: (failures: number, error: Error) => failures < 3 && ![403, 404].includes(failureStatus(error) ?? 0),
    })),
  })
  const bonds = new Map<string, BondRead>(
    instrumentIds.map((instrumentId, index) => {
      const read = bondReads[index]
      return [instrumentId, read === undefined || read.isPending ? undefined : (read.data ?? null)]
    }),
  )

  // Every answer is checked as it is sent, a typed price included: the request may have
  // moved or passed its deadline since the key was pressed.
  const answer = useCallback(
    async (rfqKey: string, action: 'quote' | 'pass', cleanPrice?: string) => {
      if (answering.current.has(rfqKey)) return
      const row = rowsRef.current.get(rfqKey)
      if (!row || !isAnswerable(row, serverNow(), false)) {
        setNotice(`Nothing was sent: ${unanswerable(row)}.`)
        return
      }
      setAnswering(rfqKey, true)
      try {
        await call(`/api/v1/rfqs/${rfqKey}/${action}`, {
          method: 'POST',
          body: action === 'quote' ? { clean_price: cleanPrice } : undefined,
        })
        setNotice(null)
      } catch (error) {
        setAnswering(rfqKey, false)
        setNotice(failureNotice(error))
      }
    },
    [call, setAnswering],
  )

  // A typed price sent or cancelled from the keyboard hands the keys back to the blotter.
  const onAdjusted = useCallback(
    (rfqKey: string, cleanPrice: string | null, returnsFocus: boolean) => {
      setEditingKey(null)
      if (cleanPrice !== null) void answer(rfqKey, 'quote', cleanPrice)
      if (returnsFocus) blotter.current?.focus()
    },
    [answer],
  )

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (editingKey !== null) return
    const index = ordered.findIndex((row) => row.rfq_key === selectedKey)
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault()
      const to = event.key === 'ArrowDown' ? index + 1 : index - 1
      const row = ordered[Math.max(0, Math.min(ordered.length - 1, to))]
      if (row) setSelectedKey(row.rfq_key)
      return
    }
    if (event.key === 'Enter' || event.key === 'Escape') {
      event.preventDefault()
      const opens = event.key === 'Enter'
      setDrawerOpen((open) => opens && !open)
      return
    }
    const row = ordered[index]
    if (!row || event.repeat || event.metaKey || event.ctrlKey || event.altKey) return
    if (!isAnswerable(row, serverNow(), answering.current.has(row.rfq_key))) return
    const key = event.key.toLowerCase()
    if (key === 'p') {
      event.preventDefault()
      void answer(row.rfq_key, 'pass')
      return
    }
    // A price's tick depends on the bond, so no price is set before it is read.
    const bond = bonds.get(row.instrument_id)
    if (bond === undefined || (key !== 's' && key !== 'a')) return
    event.preventDefault()
    // Sent on one key only on a known tick and from a two-way: a mid, an unpriced request
    // or a bond that could not be read has its price typed.
    const tick = quotingTick(bond)
    const price = tick === null ? null : sentPrice(row.auto_price, tick)
    if (key === 's' && price !== null) void answer(row.rfq_key, 'quote', price)
    else setEditingKey(row.rfq_key)
  }

  const selected = selectedKey === null ? undefined : rows.get(selectedKey)
  const withheldCount = [...withheld.values()].reduce((sum, count) => sum + count, 0)

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-4 text-xs text-slate-400">
        <BookPicker books={books} most={MAX_RFQ_BOOKS} />
        <span>
          {ordered.filter((row) => isOpen(row.state)).length} open · stream <StreamBadge status={status} />
        </span>
        <span>↑↓ select · S send · A adjust · P pass · Enter inputs</span>
        {withheldCount > 0 && (
          <span className="text-amber-300" title="The identity lacks a grant one of the book's instruments, or the bond, needs.">
            {withheldCount} withheld on {[...withheld.keys()].join(', ')}
          </span>
        )}
        {books.listingError && <span className="text-red-400">{failureNotice(books.listingError)}</span>}
        {notice && <span className="text-amber-300">{notice}</span>}
      </div>
      <div className="flex gap-3">
        <div
          ref={blotter}
          tabIndex={0}
          onKeyDown={onKeyDown}
          className="flex-1 overflow-auto rounded border border-slate-800 focus:outline-none focus:ring-1 focus:ring-sky-700"
        >
          <table className="w-full border-collapse text-xs">
            <thead className="sticky top-0 bg-slate-900 text-slate-400">
              <tr>
                {HEADINGS.map(([heading, isNumeric]) => (
                  <th key={heading} className={`px-2 py-1 font-medium ${isNumeric ? 'text-right' : 'text-left'}`}>
                    {heading}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {ordered.map((row) => (
                <Row
                  key={row.rfq_key}
                  row={row}
                  bond={bonds.get(row.instrument_id)}
                  isSelected={row.rfq_key === selectedKey}
                  isEditing={row.rfq_key === editingKey}
                  isAnswering={answeringShown.has(row.rfq_key)}
                  onSelect={setSelectedKey}
                  onAdjusted={onAdjusted}
                />
              ))}
            </tbody>
          </table>
          {ordered.length === 0 && bookIds.length > 0 && (
            <p className="px-2 py-3 text-slate-500">No requests on {bookIds.join(', ')}.</p>
          )}
        </div>
        {isDrawerOpen && selected && (
          <RfqDrawer
            row={selected}
            thirtySeconds={isQuotedInThirtySeconds(bonds.get(selected.instrument_id) ?? null)}
            onClose={() => {
              setDrawerOpen(false)
              blotter.current?.focus()
            }}
          />
        )}
      </div>
    </div>
  )
}

/** Each column's heading, and whether it holds numbers. */
const HEADINGS: [string, boolean][] = [
  ['Left', true],
  ['Client', false],
  ['Side', false],
  ['Size', true],
  ['Bond', false],
  ['CUSIP', false],
  ['Book', false],
  ['Desk', false],
  ['Level', true],
  ['Price', true],
  ['Limits', false],
  ['State', false],
  ['Quoted', true],
]

const STATE_TONE: Partial<Record<RfqState, string>> = {
  priced: 'text-sky-300',
  quoted: 'text-slate-200',
  done: 'text-emerald-400',
  traded_away: 'text-amber-300',
  covered: 'text-amber-300',
}

/** The time to an open request's deadline on the server's clock. */
function CountdownCell({ deadline }: { deadline: string }) {
  const seconds = secondsLeft(deadline, useServerNow())
  const tone = seconds <= 10 ? 'text-red-400' : seconds <= 30 ? 'text-amber-300' : 'text-slate-300'
  return <td className={`px-2 py-0.5 text-right font-mono tabular-nums ${tone}`}>{formatCountdown(seconds)}</td>
}

const Row = memo(function Row({
  row,
  bond,
  isSelected,
  isEditing,
  isAnswering,
  onSelect,
  onAdjusted,
}: {
  row: RfqRow
  bond: BondRead
  isSelected: boolean
  isEditing: boolean
  isAnswering: boolean
  onSelect: (rfqKey: string) => void
  onAdjusted: (rfqKey: string, cleanPrice: string | null, returnsFocus: boolean) => void
}) {
  const { request, auto_price: autoPrice } = row
  const tick = quotingTick(bond)
  const thirtySeconds = tick === TREASURY_TICK
  const price = tick === null ? null : tickedPrice(autoPrice, tick)
  const worst = worstStatus(row.check)
  const open = isOpen(row.state)
  return (
    <tr
      onClick={() => onSelect(row.rfq_key)}
      className={`border-t border-slate-900 ${isSelected ? 'bg-slate-800' : 'hover:bg-slate-900'} ${open ? '' : 'text-slate-500'}`}
    >
      {open ? <CountdownCell deadline={request.deadline} /> : <td />}
      <td className="px-2 py-0.5" title={`tier ${request.client_tier}`}>
        {request.client_id} <span className="text-slate-500">{request.client_tier}</span>
      </td>
      <td className={`px-2 py-0.5 ${request.side === 'buy' ? 'text-sky-300' : 'text-rose-300'}`}>
        {request.side === 'buy' ? 'Buy' : 'Sell'}
      </td>
      <td className="px-2 py-0.5 text-right font-mono tabular-nums">{formatFace(request.face)}</td>
      <td className="px-2 py-0.5 text-slate-200" title={row.instrument_id}>
        {bond?.description ?? row.instrument_id}
      </td>
      <td className="px-2 py-0.5 font-mono text-slate-400">{bond?.cusip ?? bond?.isin ?? ''}</td>
      <td className="px-2 py-0.5 text-slate-400">{request.book_id}</td>
      <td className="px-2 py-0.5 text-slate-400">{request.side === 'buy' ? 'ask' : 'bid'}</td>
      <td className="px-2 py-0.5 text-right font-mono tabular-nums">
        {autoPrice.kind === 'priced' && autoPrice.level ? formatQuoteValue(autoPrice.level) : ''}
      </td>
      <td className="px-2 py-0.5 text-right font-mono tabular-nums">
        {isEditing ? (
          <PriceInput
            initial={price ?? ''}
            tick={tick}
            onDone={(typed, returnsFocus) => onAdjusted(row.rfq_key, typed, returnsFocus)}
          />
        ) : autoPrice.kind === 'unpriced' ? (
          <span className="text-amber-300" title={autoPrice.reason}>
            unpriced
          </span>
        ) : price === null ? (
          bond === null && (
            <span className="text-slate-500" title="The bond could not be read, so its tick is not known: type the price.">
              type
            </span>
          )
        ) : (
          <span title={autoPrice.mid_only ?? undefined}>
            {autoPrice.mid_only && <span className="mr-1 text-amber-300">mid</span>}
            {formatQuotedPrice(price, thirtySeconds)}
          </span>
        )}
      </td>
      <td
        className={`px-2 py-0.5 ${worst ? LIMIT_TONE[worst] : 'text-slate-500'}`}
        title={row.check.kind === 'unavailable' ? row.check.reason : undefined}
      >
        {checkSummary(row.check)}
      </td>
      <td className={`px-2 py-0.5 ${STATE_TONE[row.state] ?? ''}`}>{isAnswering ? 'sending…' : stateLabel(row.state)}</td>
      <td className="px-2 py-0.5 text-right font-mono tabular-nums">
        {row.quote ? formatQuotedPrice(row.quote.clean_price, thirtySeconds) : ''}
      </td>
    </tr>
  )
})

/** A typed clean price, sent as written; ↑ and ↓ move it a tick, when the tick is known. */
function PriceInput({
  initial,
  tick,
  onDone,
}: {
  initial: string
  tick: string | null
  onDone: (cleanPrice: string | null, returnsFocus: boolean) => void
}) {
  const [text, setText] = useState(initial)
  const isValid = isPositiveDecimal(text)
  return (
    <input
      autoFocus
      value={text}
      onChange={(event) => setText(event.target.value.trim())}
      onKeyDown={(event) => {
        event.stopPropagation()
        if (event.key === 'Enter' && isValid && !event.repeat) onDone(text, true)
        else if (event.key === 'Escape') onDone(null, true)
        else if ((event.key === 'ArrowUp' || event.key === 'ArrowDown') && isValid && tick !== null) {
          event.preventDefault()
          setText(nudgedPrice(text, tick, event.key === 'ArrowUp' ? 1 : -1))
        }
      }}
      onBlur={() => onDone(null, false)}
      title={isValid && tick === TREASURY_TICK ? formatThirtySeconds(text) : undefined}
      className={`w-28 rounded border bg-slate-950 px-1 text-right font-mono ${isValid ? 'border-sky-700' : 'border-red-700'}`}
    />
  )
}
