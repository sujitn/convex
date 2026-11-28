import { keepPreviousData, useMutation, useQuery } from '@tanstack/react-query'
import { useSession } from '@convex/api-client'
import { memo, useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent } from 'react'
import { BookPicker } from '../components/BookPicker'
import { StreamBadge } from '../components/StreamBadge'
import { TableHead, type Column } from '../components/TableHead'
import {
  closeTaken,
  followBooks,
  holdLater,
  MAX_SUMMARY_BOOKS,
  noBookPnlText,
  summariesOn,
  withSummaries,
  type BookClose,
  type BookPnlDetail,
  type BookSummary,
  type RiskMeasure,
} from '../lib/books'
import { useServerNow } from '../lib/clock'
import { failureNotice } from '../lib/failure'
import { useFollowedBooks } from '../lib/followedBooks'
import { codeWords, formatAge, formatAmount, formatPercent, formatPrice, formatYears } from '../lib/format'
import {
  boundsLabel,
  countStatuses,
  countsSummary,
  formatLimitValue,
  LIMIT_FILL,
  LIMIT_TONE,
  measureLabel,
  unknownReasonText,
  worstCounted,
  type LimitStatus,
} from '../lib/limits'
import { readBookPnl, readLimitStatus } from '../lib/reads'
import type { StreamStatus } from '../lib/stream'

// Each followed book's risk, P&L since its close and limits, from the `book` topic, and the
// selected book's limits as the status route words them and its P&L by position. A book is
// summarized only while the server watches it: while it holds a limit set, or is in force
// with a close.

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
  // The book chosen; one the books no longer hold gives way to the first.
  const [chosenId, setChosenId] = useState<string | null>(null)
  const selectedId = chosenId !== null && bookIds.includes(chosenId) ? chosenId : (bookIds[0] ?? null)

  // Summaries arrive at up to four a second a book and are merged once a frame. Only the
  // later of each book is held meanwhile, so a page whose frames are paused, as a hidden
  // one's are, holds at most one a book.
  const pending = useRef(new Map<string, BookSummary>())
  const frame = useRef<number | null>(null)
  const flush = useCallback(() => {
    frame.current = null
    const arrived = pending.current
    if (arrived.size === 0) return
    pending.current = new Map()
    setSummaries((current) => withSummaries(current, arrived.values()))
  }, [])

  useEffect(() => {
    flush()
    setSummaries((current) => summariesOn(current, bookIds))
    return followBooks(token, bookIds, {
      onItem: (summary) => {
        holdLater(pending.current, summary)
        frame.current ??= requestAnimationFrame(flush)
      },
      onRefusal: (code, message) => setNotice(`${code}: ${message}`),
      onStatus: setStatus,
    })
  }, [bookIds, token, flush])

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return
    event.preventDefault()
    const index = selectedId === null ? -1 : bookIds.indexOf(selectedId)
    const to = event.key === 'ArrowDown' ? index + 1 : index - 1
    const bookId = bookIds[Math.max(0, Math.min(bookIds.length - 1, to))]
    if (bookId) setChosenId(bookId)
  }

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-4 text-xs text-slate-400">
        <BookPicker books={books} />
        <span>
          {summaries.size} of {bookIds.length} summarized · stream <StreamBadge status={status} />
        </span>
        <span>↑↓ select</span>
        {notice && <span className="text-amber-300">{notice}</span>}
      </div>
      <div
        tabIndex={0}
        onKeyDown={onKeyDown}
        className="overflow-auto rounded border border-slate-800 focus:outline-none focus:ring-1 focus:ring-sky-700"
      >
        <table className="w-full border-collapse text-xs">
          <TableHead columns={BOOK_COLUMNS} />
          <tbody>
            {bookIds.map((bookId) => (
              <BookRow
                key={bookId}
                bookId={bookId}
                summary={summaries.get(bookId)}
                isSelected={bookId === selectedId}
                onSelect={setChosenId}
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

const BOOK_COLUMNS: Column[] = [
  { heading: 'Book' },
  { heading: 'Ccy' },
  { heading: 'Market value', isNumeric: true },
  { heading: 'DV01', isNumeric: true },
  { heading: 'CS01', isNumeric: true },
  { heading: 'Mod dur', isNumeric: true },
  { heading: 'P&L since close', isNumeric: true },
  { heading: 'Close' },
  { heading: 'Limits' },
  { heading: 'Evaluated', isNumeric: true },
]

/** A risk measure in a cell: its value, or `unknown` with why on hover. */
function MeasureCell({ measure, format }: { measure: RiskMeasure; format: (value: string) => string }) {
  if (measure.status === 'unknown') {
    return (
      <td className="px-2 py-0.5 text-right text-amber-300" title={unknownReasonText(measure.reason)}>
        unknown
      </td>
    )
  }
  return <td className="px-2 py-0.5 text-right font-mono tabular-nums text-slate-200">{format(measure.value)}</td>
}

/** A P&L amount's colour: green when made, red when lost. */
function pnlTone(value: string): string {
  const amount = Number(value)
  return amount > 0 ? 'text-emerald-400' : amount < 0 ? 'text-red-400' : 'text-slate-300'
}

/** How long ago the summary was evaluated, on the server's clock. */
function AgeCell({ at }: { at: string }) {
  return <td className="px-2 py-0.5 text-right font-mono tabular-nums text-slate-400">{formatAge(at, useServerNow())}</td>
}

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
        <td colSpan={BOOK_COLUMNS.length - 1} className="px-2 py-0.5 text-slate-500">
          no summary: a book is summarized while it holds limits or a close
        </td>
      </tr>
    )
  }
  const { risk, pnl } = summary
  const counts = countStatuses(summary.limits.map((limit) => limit.status))
  const worst = worstCounted(counts)
  return (
    <tr className={base} onClick={() => onSelect(bookId)}>
      <td className="px-2 py-0.5 font-mono text-slate-200">{bookId}</td>
      <td className="px-2 py-0.5 text-slate-400">{summary.currency ?? '—'}</td>
      {risk.status === 'unknown' ? (
        <td colSpan={4} className="px-2 py-0.5 text-right text-amber-300" title={unknownReasonText(risk.reason)}>
          risk unknown
        </td>
      ) : (
        <>
          <MeasureCell measure={risk.market_value} format={formatAmount} />
          <MeasureCell measure={risk.dv01} format={formatAmount} />
          <MeasureCell measure={risk.cs01} format={formatAmount} />
          <MeasureCell measure={risk.modified_duration} format={formatYears} />
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

/** The selected book: its limits, its key-rate DV01, its P&L by position, and its close. */
const BookDetail = memo(function BookDetail({ bookId, summary }: { bookId: string; summary: BookSummary | undefined }) {
  const { call } = useSession()
  const close = useMutation({
    mutationFn: async () =>
      (
        await call<BookClose>(`/api/v1/books/${encodeURIComponent(bookId)}/close`, {
          method: 'POST',
          headers: { 'Idempotency-Key': crypto.randomUUID() },
        })
      ).data,
  })
  const closeNotice = close.error ? failureNotice(close.error) : close.data ? closeTaken(bookId, close.data) : null
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
        <LimitTable bookId={bookId} summary={summary} />
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
      <PnlSection bookId={bookId} summary={summary} />
    </div>
  )
})

const LIMIT_COLUMNS: Column[] = [
  { heading: 'Limit' },
  { heading: 'Measure' },
  { heading: 'Bounds', isNumeric: true },
  { heading: 'Value', isNumeric: true },
  { heading: 'Used' },
  { heading: 'Status' },
]

/**
 * Each limit's status, value and use of the moment from the summary, beside what the status
 * route says of it: its measure, the bounds it was judged against, the member that set its
 * status and, when unknown, why. The route is read again when a limit's status changes.
 */
function LimitTable({ bookId, summary }: { bookId: string; summary: BookSummary | undefined }) {
  const { call } = useSession()
  const setVersion = summary?.limit_set_version
  const evaluations = useQuery({
    queryKey: ['limit-status', bookId, setVersion],
    queryFn: () => readLimitStatus(call, bookId),
    enabled: setVersion !== undefined,
    staleTime: Infinity,
    placeholderData: keepPreviousData,
  })
  const statuses = summary?.limits.map((limit) => `${limit.limit_id}:${limit.status}`).join(',') ?? ''
  const { refetch } = evaluations
  useEffect(() => {
    if (setVersion !== undefined) void refetch({ cancelRefetch: false })
  }, [statuses, setVersion, refetch])
  const evaluated = useMemo(
    () => new Map((evaluations.data ?? []).map((evaluation) => [evaluation.limit_id, evaluation])),
    [evaluations.data],
  )
  if (summary === undefined || summary.limits.length === 0) {
    return <p className="flex-1 text-xs text-slate-500">No limits.</p>
  }
  return (
    <section className="min-w-[28rem] flex-1">
      <h2 className="mb-1 text-xs font-medium text-slate-400">Limits</h2>
      <table className="w-full border-collapse text-xs">
        <TableHead columns={LIMIT_COLUMNS} />
        <tbody>
          {summary.limits.map((limit) => {
            const evaluation = evaluated.get(limit.limit_id)
            const reason = limit.status === 'unknown' ? evaluation?.reason : undefined
            return (
              <tr key={limit.limit_id} className="border-t border-slate-800">
                <td className="px-2 py-0.5 font-mono text-slate-200">{limit.limit_id}</td>
                <td className="px-2 py-0.5 text-slate-300">
                  {evaluation ? measureLabel(evaluation.measure) : '…'}
                  {evaluation?.member && <span className="text-slate-500"> {evaluation.member}</span>}
                </td>
                <td className="px-2 py-0.5 text-right font-mono tabular-nums text-slate-400">
                  {evaluation ? boundsLabel(evaluation) : ''}
                </td>
                <td className="px-2 py-0.5 text-right font-mono tabular-nums text-slate-200">
                  {limit.value === undefined ? '—' : evaluation ? formatLimitValue(evaluation.measure, limit.value) : limit.value}
                </td>
                <td className="px-2 py-0.5">
                  {limit.utilisation !== undefined && <UtilisationBar utilisation={limit.utilisation} status={limit.status} />}
                </td>
                <td className={`px-2 py-0.5 ${LIMIT_TONE[limit.status]}`}>
                  {limit.status}
                  {reason && <span className="ml-2 text-slate-500">{unknownReasonText(reason)}</span>}
                </td>
              </tr>
            )
          })}
        </tbody>
      </table>
      {evaluations.error && <p className="text-xs text-red-400">{failureNotice(evaluations.error)}</p>}
    </section>
  )
}

/** A limit's use as a bar, coloured by its status, and in percent. */
function UtilisationBar({ utilisation, status }: { utilisation: string; status: LimitStatus }) {
  const width = Math.min(100, Math.max(0, Number(utilisation) * 100))
  return (
    <div className="flex items-center gap-2">
      <div className="h-1.5 w-24 rounded bg-slate-800">
        <div className={`h-1.5 rounded ${LIMIT_FILL[status]}`} style={{ width: `${width}%` }} />
      </div>
      <span className="font-mono tabular-nums text-slate-400">{formatPercent(utilisation)}</span>
    </div>
  )
}

/**
 * P&L since the book's latest close, by position: read once a close is measured, and again
 * at most every 2 seconds while the book's summaries arrive, the last of a burst included.
 */
function PnlSection({ bookId, summary }: { bookId: string; summary: BookSummary | undefined }) {
  const { call } = useSession()
  const closeVersion = summary?.pnl.status === 'measured' ? summary.pnl.close_version : undefined
  const pnl = useQuery({
    queryKey: ['book-pnl', bookId, closeVersion],
    queryFn: () => readBookPnl(call, bookId),
    enabled: closeVersion !== undefined,
    staleTime: Infinity,
    placeholderData: keepPreviousData,
  })
  const { refetch } = pnl
  const readAt = useRef(0)
  const evaluatedAt = summary?.evaluated_at
  useEffect(() => {
    if (closeVersion === undefined || evaluatedAt === undefined) return
    const read = () => {
      readAt.current = Date.now()
      void refetch({ cancelRefetch: false })
    }
    const wait = readAt.current + PNL_READ_MS - Date.now()
    if (wait <= 0) {
      read()
      return
    }
    const timer = setTimeout(read, wait)
    return () => clearTimeout(timer)
  }, [evaluatedAt, closeVersion, refetch])
  return (
    <section>
      <h2 className="mb-1 text-xs font-medium text-slate-400">
        P&amp;L since the close
        {pnl.data && ` of ${pnl.data.close.close_date} (version ${pnl.data.close.version}), ${pnl.data.currency}`}
      </h2>
      {summary?.pnl.status === 'unknown' && <p className="text-xs text-slate-500">{noBookPnlText(summary.pnl.reason)}.</p>}
      {pnl.error && <p className="text-xs text-red-400">{failureNotice(pnl.error)}</p>}
      {pnl.data && <PositionTable detail={pnl.data} />}
    </section>
  )
}

const POSITION_COLUMNS: Column[] = [
  { heading: 'Instrument' },
  { heading: 'Close face', isNumeric: true },
  { heading: 'Close dirty', isNumeric: true },
  { heading: 'Face', isNumeric: true },
  { heading: 'Dirty', isNumeric: true },
  { heading: 'Held through', isNumeric: true },
  { heading: 'Cash', isNumeric: true },
  { heading: 'P&L', isNumeric: true },
]

/** Each position held at the close or now, with its P&L or why it has none, and the total. */
const PositionTable = memo(function PositionTable({ detail }: { detail: BookPnlDetail }) {
  const cell = 'px-2 py-0.5 text-right font-mono tabular-nums'
  return (
    <table className="w-full border-collapse text-xs">
      <TableHead columns={POSITION_COLUMNS} />
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
              <td className="px-2 py-0.5 text-right text-slate-500">{position.no_pnl ? codeWords(position.no_pnl) : '—'}</td>
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
})
