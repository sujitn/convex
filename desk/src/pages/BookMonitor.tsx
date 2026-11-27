import { keepPreviousData, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useSession } from '@convex/api-client'
import { memo, useCallback, useEffect, useRef, useState, type KeyboardEvent } from 'react'
import { BookPicker } from '../components/BookPicker'
import { StreamBadge } from '../components/StreamBadge'
import {
  boundsLabel,
  closeTaken,
  followBooks,
  formatAmount,
  formatLimitValue,
  formatPercent,
  limitCounts,
  MAX_SUMMARY_BOOKS,
  measureLabel,
  noBookPnlText,
  noPnlText,
  onBooks,
  unknownReasonText,
  withSummary,
  type BookClose,
  type BookPnlDetail,
  type BookSummary,
  type LimitEvaluation,
  type LimitSet,
  type RiskMeasure,
} from '../lib/books'
import { useServerNow } from '../lib/clock'
import { failureNotice, failureStatus } from '../lib/failure'
import { useFollowedBooks } from '../lib/followedBooks'
import { formatAge, formatPrice } from '../lib/format'
import { countsSummary, LIMIT_TONE, worstCounted } from '../lib/limits'
import type { StreamStatus } from '../lib/stream'

// Each followed book's risk, P&L since its close and limits, from the `book` topic, and the
// selected book's limits as defined and its P&L by position. A book is summarized only
// while the server watches it: while it holds a limit set, or is in force with a close.

type Summaries = ReadonlyMap<string, BookSummary>

/** How often the selected book's P&L by position is read again while its summaries arrive. */
const PNL_READ_MS = 2000

export function BookMonitor() {
  const { token } = useSession()
  const books = useFollowedBooks(MAX_SUMMARY_BOOKS)
  const { bookIds } = books
  const [summaries, setSummaries] = useState<Summaries>(() => new Map())
  const [status, setStatus] = useState<StreamStatus | 'idle'>('idle')
  const [notice, setNotice] = useState<string | null>(null)
  const [selectedId, setSelectedId] = useState<string | null>(null)

  // Summaries arrive at up to four a second a book; they are merged once a frame.
  const pending = useRef<BookSummary[]>([])
  const frame = useRef<number | null>(null)
  const flush = useCallback(() => {
    frame.current = null
    const arrived = pending.current
    pending.current = []
    if (arrived.length > 0) setSummaries((current) => arrived.reduce(withSummary, current))
  }, [])

  useEffect(() => {
    flush()
    setSummaries((current) => onBooks(current, bookIds))
    return followBooks(token, bookIds, {
      onSummary: (summary) => {
        pending.current.push(summary)
        frame.current ??= requestAnimationFrame(flush)
      },
      onRefusal: (code, message) => setNotice(`${code}: ${message}`),
      onStatus: setStatus,
    })
  }, [bookIds, token, flush])

  // The first book is selected once; a selection the books no longer hold selects nothing.
  useEffect(() => {
    if (selectedId !== null && !bookIds.includes(selectedId)) setSelectedId(null)
    else if (selectedId === null && bookIds[0]) setSelectedId(bookIds[0])
  }, [bookIds, selectedId])

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return
    event.preventDefault()
    const index = selectedId === null ? -1 : bookIds.indexOf(selectedId)
    const to = event.key === 'ArrowDown' ? index + 1 : index - 1
    const bookId = bookIds[Math.max(0, Math.min(bookIds.length - 1, to))]
    if (bookId) setSelectedId(bookId)
  }

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-4 text-xs text-slate-400">
        <BookPicker books={books} most={MAX_SUMMARY_BOOKS} />
        <span>
          {summaries.size} of {bookIds.length} summarized · stream <StreamBadge status={status} />
        </span>
        <span>↑↓ select</span>
        {books.listingError && <span className="text-red-400">{failureNotice(books.listingError)}</span>}
        {notice && <span className="text-amber-300">{notice}</span>}
      </div>
      <div
        tabIndex={0}
        onKeyDown={onKeyDown}
        className="overflow-auto rounded border border-slate-800 focus:outline-none focus:ring-1 focus:ring-sky-700"
      >
        <table className="w-full border-collapse text-xs">
          <thead className="sticky top-0 bg-slate-900 text-slate-400">
            <tr>
              {BOOK_HEADINGS.map(([heading, isNumeric]) => (
                <th key={heading} className={`px-2 py-1 font-medium ${isNumeric ? 'text-right' : 'text-left'}`}>
                  {heading}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {bookIds.map((bookId) => (
              <BookRow
                key={bookId}
                bookId={bookId}
                summary={summaries.get(bookId)}
                isSelected={bookId === selectedId}
                onSelect={setSelectedId}
              />
            ))}
          </tbody>
        </table>
        {bookIds.length === 0 && !books.isListing && <p className="px-2 py-3 text-slate-500">No books followed.</p>}
      </div>
      {selectedId !== null && <BookDetail key={selectedId} bookId={selectedId} summary={summaries.get(selectedId)} />}
    </div>
  )
}

/** Each column's heading, and whether it holds numbers. */
const BOOK_HEADINGS: [string, boolean][] = [
  ['Book', false],
  ['Ccy', false],
  ['Market value', true],
  ['DV01', true],
  ['CS01', true],
  ['Mod dur', true],
  ['P&L since close', true],
  ['Close', false],
  ['Limits', false],
  ['Evaluated', true],
]

/** A risk measure in a cell: its value, or a dash with why it is not known. */
function MeasureCell({ measure, format }: { measure: RiskMeasure | undefined; format: (value: string) => string }) {
  if (measure === undefined) return <td className="px-2 py-0.5 text-right text-slate-600">—</td>
  if (measure.status === 'unknown') {
    return (
      <td className="px-2 py-0.5 text-right text-amber-300" title={unknownReasonText(measure.reason)}>
        unknown
      </td>
    )
  }
  return <td className="px-2 py-0.5 text-right font-mono tabular-nums text-slate-200">{format(measure.value)}</td>
}

/** A P&L amount, green when made and red when lost. */
function pnlTone(value: string): string {
  const amount = Number(value)
  return amount > 0 ? 'text-emerald-400' : amount < 0 ? 'text-red-400' : 'text-slate-300'
}

/** How long ago the summary was evaluated, on the server's clock. */
function AgeCell({ at }: { at: string }) {
  return <td className="px-2 py-0.5 text-right font-mono tabular-nums text-slate-400">{formatAge(at, useServerNow())}</td>
}

const twoPlaces = (value: string) => Number(value).toFixed(2)

const BookRow = memo(function BookRow({
  bookId,
  summary,
  isSelected,
  onSelect,
}: {
  bookId: string
  summary: BookSummary | undefined
  isSelected: boolean
  onSelect: (bookId: string) => void
}) {
  const base = `cursor-pointer border-t border-slate-800 ${isSelected ? 'bg-sky-950' : 'hover:bg-slate-900'}`
  if (summary === undefined) {
    return (
      <tr className={base} onClick={() => onSelect(bookId)}>
        <td className="px-2 py-0.5 font-mono text-slate-200">{bookId}</td>
        <td colSpan={BOOK_HEADINGS.length - 1} className="px-2 py-0.5 text-slate-500">
          no summary: a book is summarized while it holds limits or a close
        </td>
      </tr>
    )
  }
  const risk = summary.risk.status === 'measured' ? summary.risk : undefined
  const unknownRisk = summary.risk.status === 'unknown' ? summary.risk.reason : undefined
  const counts = limitCounts(summary)
  const worst = worstCounted(counts)
  const { pnl } = summary
  return (
    <tr className={base} onClick={() => onSelect(bookId)}>
      <td className="px-2 py-0.5 font-mono text-slate-200">{bookId}</td>
      <td className="px-2 py-0.5 text-slate-400">{summary.currency ?? '—'}</td>
      {unknownRisk ? (
        <td colSpan={4} className="px-2 py-0.5 text-right text-amber-300" title={unknownReasonText(unknownRisk)}>
          risk unknown
        </td>
      ) : (
        <>
          <MeasureCell measure={risk?.market_value} format={formatAmount} />
          <MeasureCell measure={risk?.dv01} format={formatAmount} />
          <MeasureCell measure={risk?.cs01} format={formatAmount} />
          <MeasureCell measure={risk?.modified_duration} format={twoPlaces} />
        </>
      )}
      {pnl.status === 'measured' ? (
        <td
          className={`px-2 py-0.5 text-right font-mono tabular-nums ${pnlTone(pnl.pnl)}`}
          title={`of which entitled cash ${formatAmount(pnl.entitled_cash)}${pnl.excluded_count > 0 ? `; ${pnl.excluded_count} positions without P&L` : ''}`}
        >
          {formatAmount(pnl.pnl)}
          {pnl.excluded_count > 0 && <span className="text-amber-300">*</span>}
        </td>
      ) : (
        <td className="px-2 py-0.5 text-right text-slate-500">{noBookPnlText(pnl.reason)}</td>
      )}
      <td className="px-2 py-0.5 text-slate-400">{pnl.status === 'measured' ? pnl.close_date : '—'}</td>
      <td className={`px-2 py-0.5 ${worst ? LIMIT_TONE[worst] : 'text-slate-500'}`}>{countsSummary(counts)}</td>
      <AgeCell at={summary.evaluated_at} />
    </tr>
  )
})

/** The selected book: its limits as defined, its key-rate DV01, its P&L by position, and its close. */
function BookDetail({ bookId, summary }: { bookId: string; summary: BookSummary | undefined }) {
  const { call } = useSession()
  const path = `/api/v1/books/${encodeURIComponent(bookId)}`
  const limitSetVersion = summary?.limit_set_version
  const limitSet = useQuery({
    queryKey: ['limit-set', bookId, limitSetVersion],
    queryFn: async () => (await call<LimitSet>(`${path}/limits`)).data,
    enabled: limitSetVersion !== undefined,
    staleTime: Infinity,
  })
  // Reasons stay on the status route: it is read again when a limit's status changes.
  const statuses = summary?.limits.map((limit) => `${limit.limit_id}:${limit.status}`).join(',') ?? ''
  const hasUnknown = summary?.limits.some((limit) => limit.status === 'unknown') ?? false
  const evaluations = useQuery({
    queryKey: ['limit-status', bookId, statuses],
    queryFn: async () => (await call<{ limits: LimitEvaluation[] }>(`${path}/limits/status`)).data.limits,
    enabled: hasUnknown,
    staleTime: Infinity,
    placeholderData: keepPreviousData,
  })
  const reasons = new Map((evaluations.data ?? []).map((evaluation) => [evaluation.limit_id, evaluation.reason]))
  const definitions = new Map((limitSet.data?.limits ?? []).map((limit) => [limit.limit_id, limit]))

  const hasClose = summary?.pnl.status === 'measured'
  const readSlot = summary === undefined ? 0 : Math.floor(Date.parse(summary.evaluated_at) / PNL_READ_MS)
  const pnl = useQuery({
    queryKey: ['book-pnl', bookId, readSlot],
    queryFn: async () => (await call<BookPnlDetail>(`${path}/pnl`)).data,
    enabled: hasClose,
    staleTime: Infinity,
    placeholderData: keepPreviousData,
    retry: (failures: number, error: Error) => failures < 2 && failureStatus(error) !== 404,
  })

  const client = useQueryClient()
  const [closeNotice, setCloseNotice] = useState<string | null>(null)
  const close = useMutation({
    mutationFn: async () =>
      (await call<BookClose>(`${path}/close`, { method: 'POST', headers: { 'Idempotency-Key': crypto.randomUUID() } })).data,
    onSuccess: (taken) => {
      setCloseNotice(closeTaken(bookId, taken))
      void client.invalidateQueries({ queryKey: ['book-pnl', bookId] })
    },
    onError: (error) => setCloseNotice(failureNotice(error)),
  })
  const takeClose = () => {
    if (window.confirm(`Close ${bookId} now? Its P&L is then measured from this close.`)) close.mutate()
  }

  const risk = summary?.risk.status === 'measured' ? summary.risk : undefined
  return (
    <div className="flex flex-col gap-3 rounded border border-slate-800 p-3">
      <div className="flex flex-wrap items-center gap-4 text-xs text-slate-400">
        <span className="font-mono text-sm text-slate-200">{bookId}</span>
        {summary && (
          <span>
            version {summary.book_version} · settles {summary.settlement_date ?? '—'}
            {risk?.gross_market_value.status === 'known' && ` · gross ${formatAmount(risk.gross_market_value.value)}`}
          </span>
        )}
        <button
          type="button"
          disabled={close.isPending}
          onClick={takeClose}
          className="rounded bg-slate-800 px-2 py-0.5 text-slate-200 disabled:opacity-40"
        >
          {close.isPending ? 'Closing…' : 'Take close'}
        </button>
        {closeNotice && <span className="text-slate-300">{closeNotice}</span>}
      </div>
      <div className="flex flex-wrap gap-6">
        <section className="min-w-[28rem] flex-1">
          <h2 className="mb-1 text-xs font-medium text-slate-400">Limits</h2>
          {summary === undefined || summary.limits.length === 0 ? (
            <p className="text-xs text-slate-500">No limits.</p>
          ) : (
            <table className="w-full border-collapse text-xs">
              <thead className="text-slate-400">
                <tr>
                  <th className="px-2 py-0.5 text-left font-medium">Limit</th>
                  <th className="px-2 py-0.5 text-left font-medium">Measure</th>
                  <th className="px-2 py-0.5 text-right font-medium">Bounds</th>
                  <th className="px-2 py-0.5 text-right font-medium">Value</th>
                  <th className="px-2 py-0.5 text-left font-medium">Used</th>
                  <th className="px-2 py-0.5 text-left font-medium">Status</th>
                </tr>
              </thead>
              <tbody>
                {summary.limits.map((limit) => {
                  const definition = definitions.get(limit.limit_id)
                  const reason = reasons.get(limit.limit_id)
                  const used = limit.utilisation === undefined ? null : Number(limit.utilisation)
                  return (
                    <tr key={limit.limit_id} className="border-t border-slate-800">
                      <td className="px-2 py-0.5 font-mono text-slate-200">{limit.limit_id}</td>
                      <td className="px-2 py-0.5 text-slate-300">{definition ? measureLabel(definition.measure) : '…'}</td>
                      <td className="px-2 py-0.5 text-right font-mono tabular-nums text-slate-400">
                        {definition ? boundsLabel(definition) : ''}
                      </td>
                      <td className="px-2 py-0.5 text-right font-mono tabular-nums text-slate-200">
                        {limit.value !== undefined && definition ? formatLimitValue(definition.measure, limit.value) : '—'}
                      </td>
                      <td className="px-2 py-0.5">
                        {used !== null && (
                          <div className="flex items-center gap-2">
                            <div className="h-1.5 w-24 rounded bg-slate-800">
                              <div
                                className={`h-1.5 rounded ${used > 1 ? 'bg-red-500' : used > 0.8 ? 'bg-amber-400' : 'bg-emerald-500'}`}
                                style={{ width: `${Math.min(100, Math.max(0, used * 100))}%` }}
                              />
                            </div>
                            <span className="font-mono tabular-nums text-slate-400">{formatPercent(limit.utilisation!)}</span>
                          </div>
                        )}
                      </td>
                      <td className={`px-2 py-0.5 ${LIMIT_TONE[limit.status]}`} title={reason ? unknownReasonText(reason) : undefined}>
                        {limit.status}
                        {reason && <span className="ml-2 text-slate-500">{unknownReasonText(reason)}</span>}
                      </td>
                    </tr>
                  )
                })}
              </tbody>
            </table>
          )}
          {limitSet.error && <p className="text-xs text-red-400">{failureNotice(limitSet.error)}</p>}
        </section>
        {risk && risk.key_rate_dv01.length > 0 && (
          <section>
            <h2 className="mb-1 text-xs font-medium text-slate-400">Key-rate DV01</h2>
            <table className="border-collapse text-xs">
              <tbody>
                {risk.key_rate_dv01.map((point) => (
                  <tr key={point.tenor} className="border-t border-slate-800">
                    <td className="px-2 py-0.5 text-slate-400">{point.tenor}</td>
                    <MeasureCell measure={point} format={formatAmount} />
                  </tr>
                ))}
              </tbody>
            </table>
          </section>
        )}
      </div>
      <section>
        <h2 className="mb-1 text-xs font-medium text-slate-400">
          P&amp;L since the close
          {pnl.data && ` of ${pnl.data.close.close_date} (version ${pnl.data.close.version}), ${pnl.data.currency}`}
        </h2>
        {summary?.pnl.status === 'unknown' && <p className="text-xs text-slate-500">{noBookPnlText(summary.pnl.reason)}.</p>}
        {pnl.error && <p className="text-xs text-red-400">{failureNotice(pnl.error)}</p>}
        {pnl.data && <PositionTable detail={pnl.data} />}
      </section>
    </div>
  )
}

/** Each position held at the close or now, with its P&L or why it has none, and the total. */
function PositionTable({ detail }: { detail: BookPnlDetail }) {
  const cell = 'px-2 py-0.5 text-right font-mono tabular-nums'
  return (
    <table className="w-full border-collapse text-xs">
      <thead className="text-slate-400">
        <tr>
          <th className="px-2 py-0.5 text-left font-medium">Instrument</th>
          <th className="px-2 py-0.5 text-right font-medium">Close face</th>
          <th className="px-2 py-0.5 text-right font-medium">Close dirty</th>
          <th className="px-2 py-0.5 text-right font-medium">Face</th>
          <th className="px-2 py-0.5 text-right font-medium">Dirty</th>
          <th className="px-2 py-0.5 text-right font-medium">Held through</th>
          <th className="px-2 py-0.5 text-right font-medium">Cash</th>
          <th className="px-2 py-0.5 text-right font-medium">P&amp;L</th>
        </tr>
      </thead>
      <tbody>
        {detail.positions.map((position) => (
          <tr key={position.instrument_id} className="border-t border-slate-800">
            <td className="px-2 py-0.5 font-mono text-slate-200">{position.instrument_id}</td>
            <td className={`${cell} text-slate-400`}>{formatAmount(position.close_face)}</td>
            <td className={`${cell} text-slate-400`}>{position.close_dirty_price ? formatPrice(position.close_dirty_price) : '—'}</td>
            <td className={`${cell} text-slate-200`}>{formatAmount(position.face)}</td>
            <td className={`${cell} text-slate-200`}>{position.dirty_price ? formatPrice(position.dirty_price) : '—'}</td>
            <td className={`${cell} text-slate-400`}>{position.held_face ? formatAmount(position.held_face) : '—'}</td>
            <td className={`${cell} text-slate-400`}>{position.entitled_cash ? formatAmount(position.entitled_cash) : '—'}</td>
            {position.pnl !== undefined ? (
              <td className={`${cell} ${pnlTone(position.pnl)}`}>{formatAmount(position.pnl)}</td>
            ) : (
              <td className="px-2 py-0.5 text-right text-slate-500">{position.no_pnl ? noPnlText(position.no_pnl) : '—'}</td>
            )}
          </tr>
        ))}
        <tr className="border-t border-slate-700 font-medium">
          <td className="px-2 py-0.5 text-slate-300">Total</td>
          <td colSpan={5} className="px-2 py-0.5 text-right text-slate-500">
            {detail.excluded_count > 0 && `${detail.excluded_count} positions without P&L left out`}
          </td>
          <td className={`${cell} text-slate-300`}>{formatAmount(detail.entitled_cash)}</td>
          <td className={`${cell} ${pnlTone(detail.pnl)}`}>{formatAmount(detail.pnl)}</td>
        </tr>
      </tbody>
    </table>
  )
}
