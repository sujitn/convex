import { useQuery } from '@tanstack/react-query'
import { ApiFailure, useSession, type PublishedValuation } from '@convex/api-client'
import { memo, useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent } from 'react'
import { isDecimal } from '../lib/decimal'
import { formatAge, formatCoupon, formatMeasure, formatThirtySeconds, formatYieldPercent, formatBasisPoints, formatPrice } from '../lib/format'
import { readBonds, readDeskLevels, readPricingRules } from '../lib/reads'
import {
  instrumentOf,
  isQuotedInThirtySeconds,
  measure,
  methodLabel,
  nudgedLevel,
  rowState,
  sheetRows,
  shownLevel,
  withValuation,
  type DeskMark,
  type SheetRow,
} from '../lib/sheet'
import { followValuations, type StreamStatus } from '../lib/stream'

// Every bond with a pricing rule, repriced as the Live runtime publishes. The keyboard
// moves the selection and nudges the selected bond's level when the desk quotes it:
// a mark is posted to the server, and the bond's next result prices it.

/** How long a moved price keeps its tick colour. */
const TICK_MS = 1500

interface SheetData {
  rows: SheetRow[]
  /** The desk's level source; none when this server takes no desk levels. */
  deskSource: string | null
}

export function PricingSheet() {
  const { call, token } = useSession()
  const sheet = useQuery({
    queryKey: ['pricing-sheet'],
    queryFn: async (): Promise<SheetData> => {
      const [rules, bonds, desk] = await Promise.all([readPricingRules(call), readBonds(call), readDeskLevels(call)])
      return { rows: sheetRows(rules, bonds, desk), deskSource: desk?.source ?? null }
    },
  })
  const [rows, setRows] = useState<Map<string, SheetRow>>(new Map())
  const [order, setOrder] = useState<string[]>([])
  const [status, setStatus] = useState<StreamStatus | 'idle'>('idle')
  const [notice, setNotice] = useState<string | null>(null)
  const [selected, setSelected] = useState(0)
  const [editing, setEditing] = useState<string | null>(null)
  const [now, setNow] = useState(() => Date.now())
  const pending = useRef<PublishedValuation[]>([])
  const frame = useRef<number | null>(null)

  useEffect(() => {
    if (!sheet.data) return
    setRows(new Map(sheet.data.rows.map((row) => [row.instrumentId, row])))
    setOrder(sheet.data.rows.map((row) => row.instrumentId))
  }, [sheet.data])

  // Results arrive in bursts; they are merged once a frame.
  const flush = useCallback(() => {
    frame.current = null
    const arrived = pending.current
    pending.current = []
    const receivedAt = Date.now()
    setRows((current) => {
      const next = new Map(current)
      for (const valuation of arrived) {
        const id = instrumentOf(valuation)
        const row = id === undefined ? undefined : next.get(id)
        if (row && id !== undefined) next.set(id, withValuation(row, valuation, receivedAt))
      }
      return next
    })
  }, [])

  useEffect(() => {
    if (!sheet.data) return
    return followValuations(
      token,
      sheet.data.rows.map((row) => row.instrumentId),
      {
        onValuation: (valuation) => {
          pending.current.push(valuation)
          if (frame.current === null) frame.current = requestAnimationFrame(flush)
        },
        onStatus: setStatus,
        onRefusal: (code, message) => setNotice(`${code}: ${message}`),
      },
    )
  }, [sheet.data, token, flush])

  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 1000)
    return () => clearInterval(timer)
  }, [])

  const deskSource = sheet.data?.deskSource ?? null

  const mark = useCallback(
    async (row: SheetRow, level: string) => {
      if (!deskSource || !row.deskField) return
      const previous = row.mark
      const optimistic: DeskMark = { level, marked_at: new Date().toISOString() }
      setRows((current) => new Map(current).set(row.instrumentId, { ...row, mark: optimistic }))
      try {
        const reply = await call<{ receive_time_ms: number }>('/api/v1/market/levels', {
          method: 'POST',
          body: { source: deskSource, levels: [{ instrument_id: row.instrumentId, field: row.deskField, level }] },
        })
        const marked: DeskMark = { level, marked_at: new Date(reply.data.receive_time_ms).toISOString() }
        setRows((current) => {
          const latest = current.get(row.instrumentId)
          return latest ? new Map(current).set(row.instrumentId, { ...latest, mark: marked }) : current
        })
        setNotice(null)
      } catch (error) {
        setRows((current) => {
          const latest = current.get(row.instrumentId)
          return latest ? new Map(current).set(row.instrumentId, { ...latest, mark: previous }) : current
        })
        setNotice(error instanceof ApiFailure ? `${error.code}: ${error.reason}` : String(error))
      }
    },
    [call, deskSource],
  )

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (editing !== null) return
    const row = rows.get(order[selected] ?? '')
    const fine = event.altKey
    const nudge = (steps: number) => {
      if (!row?.deskField || !row.mark) return
      event.preventDefault()
      void mark(row, nudgedLevel(row.mark.level, row.deskField, steps, fine))
    }
    if (event.key === 'ArrowDown' && !event.shiftKey) {
      event.preventDefault()
      setSelected((index) => Math.min(order.length - 1, index + 1))
    } else if (event.key === 'ArrowUp' && !event.shiftKey) {
      event.preventDefault()
      setSelected((index) => Math.max(0, index - 1))
    } else if (event.key === '+' || event.key === '=' || (event.shiftKey && event.key === 'ArrowUp')) {
      nudge(1)
    } else if (event.key === '-' || event.key === '_' || (event.shiftKey && event.key === 'ArrowDown')) {
      nudge(-1)
    } else if (event.key === 'Enter' && row?.deskField) {
      event.preventDefault()
      setEditing(row.instrumentId)
    }
  }

  const ordered = useMemo(() => order.map((id) => rows.get(id)).filter((row): row is SheetRow => row !== undefined), [order, rows])

  if (sheet.isPending) return <p className="text-sm text-slate-400">Reading pricing rules and bonds…</p>
  if (sheet.error) return <p className="text-sm text-red-400">{String(sheet.error)}</p>

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-4 text-xs text-slate-400">
        <span>
          {ordered.length} bonds · stream <StreamBadge status={status} />
        </span>
        <span>
          {deskSource
            ? `Desk levels on ${deskSource}: ↑↓ select · + / − nudge 1 bp (Alt: ¼ bp) · Enter types a level`
            : 'This server takes no desk levels: levels are read-only.'}
        </span>
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
              {['Bond', 'CUSIP', 'Cpn', 'Maturity', 'Method', 'Level', 'Clean', 'Yield', 'Z', 'G', 'DV01', 'State', 'Mark'].map(
                (heading, index) => (
                  <th key={heading} className={`px-2 py-1 font-medium ${index >= 5 && index <= 10 ? 'text-right' : 'text-left'}`}>
                    {heading}
                  </th>
                ),
              )}
            </tr>
          </thead>
          <tbody>
            {ordered.map((row, index) => (
              <Row
                key={row.instrumentId}
                row={row}
                selected={index === selected}
                editing={editing === row.instrumentId}
                now={now}
                onSelect={() => setSelected(index)}
                onMark={(level) => {
                  setEditing(null)
                  if (level !== null) void mark(row, level)
                }}
              />
            ))}
          </tbody>
        </table>
      </div>
    </div>
  )
}

function StreamBadge({ status }: { status: StreamStatus | 'idle' }) {
  const tone = status === 'open' ? 'text-emerald-400' : status === 'idle' ? 'text-slate-500' : 'text-amber-300'
  return <span className={tone}>{status}</span>
}

const STATE_TONE: Record<string, string> = {
  valid: 'text-emerald-400',
  partial: 'text-amber-300',
  stale: 'text-amber-300',
  expired: 'text-amber-300',
  failed: 'text-red-400',
  awaiting: 'text-slate-500',
}

const Row = memo(function Row({
  row,
  selected,
  editing,
  now,
  onSelect,
  onMark,
}: {
  row: SheetRow
  selected: boolean
  editing: boolean
  now: number
  onSelect: () => void
  onMark: (level: string | null) => void
}) {
  const thirtySeconds = isQuotedInThirtySeconds(row.bond)
  const state = rowState(row.valuation, now)
  const ticking = row.move !== null && row.movedAt !== null && now - row.movedAt < TICK_MS
  const cell = (name: string) => {
    const value = measure(row.valuation, name)
    return value ? formatMeasure(value.value.value, value.value.unit, thirtySeconds) : ''
  }
  const level = shownLevel(row)
  const levelText = level === null ? '' : levelDisplay(level.value, level.field, thirtySeconds)
  const firstError = row.valuation?.result.errors[0] as { message?: string } | undefined
  return (
    <tr
      onClick={onSelect}
      className={`border-t border-slate-900 ${selected ? 'bg-slate-800' : 'hover:bg-slate-900'}`}
      title={firstError?.message}
    >
      <td className="px-2 py-0.5 text-slate-200">{row.bond?.description ?? row.instrumentId}</td>
      <td className="px-2 py-0.5 font-mono text-slate-400">{row.bond?.cusip ?? row.bond?.isin ?? ''}</td>
      <td className="px-2 py-0.5 font-mono tabular-nums">{formatCoupon(row.bond?.coupon_rate)}</td>
      <td className="px-2 py-0.5 font-mono tabular-nums text-slate-400">{row.bond?.maturity_date ?? ''}</td>
      <td className="px-2 py-0.5 text-slate-400">{methodLabel(row.rule.method)}</td>
      <td className={`px-2 py-0.5 text-right font-mono tabular-nums ${row.deskField ? 'text-sky-300' : ''}`}>
        {editing ? <LevelInput initial={row.mark?.level ?? ''} onDone={onMark} /> : levelText}
      </td>
      <td
        className={`px-2 py-0.5 text-right font-mono tabular-nums ${
          ticking ? (row.move === 'up' ? 'bg-emerald-900 text-emerald-200' : 'bg-red-900 text-red-200') : ''
        }`}
      >
        {cell('clean_price_mid')}
      </td>
      <td className="px-2 py-0.5 text-right font-mono tabular-nums">{cell('ytm_mid')}</td>
      <td className="px-2 py-0.5 text-right font-mono tabular-nums">{cell('z_spread_mid')}</td>
      <td className="px-2 py-0.5 text-right font-mono tabular-nums">{cell('g_spread_mid')}</td>
      <td className="px-2 py-0.5 text-right font-mono tabular-nums">{cell('dv01_mid')}</td>
      <td className={`px-2 py-0.5 ${STATE_TONE[state] ?? ''}`}>{state}</td>
      <td className="px-2 py-0.5 font-mono text-slate-500">{row.mark ? formatAge(row.mark.marked_at, now) : ''}</td>
    </tr>
  )
})

function levelDisplay(value: string | number, field: 'mid_yield' | 'mid_spread' | 'price', thirtySeconds: boolean): string {
  if (field === 'mid_yield') return formatYieldPercent(value)
  if (field === 'mid_spread') return formatBasisPoints(value)
  return thirtySeconds ? formatThirtySeconds(value) : formatPrice(value)
}

/** A typed level, sent as written: a decimal yield or basis points, as the field reads. */
function LevelInput({ initial, onDone }: { initial: string; onDone: (level: string | null) => void }) {
  const [text, setText] = useState(initial)
  return (
    <input
      autoFocus
      value={text}
      onChange={(event) => setText(event.target.value.trim())}
      onKeyDown={(event) => {
        if (event.key === 'Enter' && isDecimal(text)) onDone(text)
        if (event.key === 'Escape') onDone(null)
        event.stopPropagation()
      }}
      onBlur={() => onDone(null)}
      className={`w-24 rounded border bg-slate-950 px-1 text-right font-mono ${isDecimal(text) ? 'border-sky-700' : 'border-red-700'}`}
    />
  )
}
