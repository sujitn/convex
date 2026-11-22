import { useQuery } from '@tanstack/react-query'
import { ApiFailure, useSession, type PublishedValuation } from '@convex/api-client'
import { memo, useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore, type KeyboardEvent } from 'react'
import { isDecimal } from '../lib/decimal'
import { formatAge, formatCoupon, formatMeasure } from '../lib/format'
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
  withMarks,
  type DeskMark,
  type RowState,
  type SheetRow,
} from '../lib/sheet'
import { followValuations, type StreamStatus } from '../lib/stream'

// Every bond with a pricing rule, repriced as the Live runtime publishes. The keyboard
// moves the selection and nudges the selected bond's level when the desk quotes it:
// a mark is posted to the server, and the bond's next result prices it.

/** How often the desk's marks are read again, so another page's marks appear. */
const MARKS_REFRESH_MS = 5000

export function PricingSheet() {
  const { call, token } = useSession()
  const universe = useQuery({
    queryKey: ['pricing-sheet'],
    queryFn: async () => {
      const [rules, bonds] = await Promise.all([readPricingRules(call), readBonds(call)])
      return { rules, bonds }
    },
  })
  const desk = useQuery({
    queryKey: ['desk-levels'],
    queryFn: () => readDeskLevels(call),
    refetchInterval: MARKS_REFRESH_MS,
  })
  const [rows, setRows] = useState<SheetRow[]>([])
  const [status, setStatus] = useState<StreamStatus | 'idle'>('idle')
  const [notice, setNotice] = useState<string | null>(null)
  const [selected, setSelected] = useState(0)
  const [editing, setEditing] = useState<number | null>(null)
  const rowsRef = useRef(rows)
  rowsRef.current = rows
  const pending = useRef<PublishedValuation[]>([])
  const frame = useRef<number | null>(null)
  /** Bonds with a mark in flight, and the latest level waiting to follow it. */
  const inFlight = useRef(new Map<string, string | null>())
  const deskData = desk.data ?? null
  const deskSource = deskData?.source ?? null
  const deskRef = useRef(deskData)
  deskRef.current = deskData
  // A failed read of the marks leaves the sheet read-only, saying why.
  const isDeskRead = desk.isSuccess || desk.isError

  // Rows are built once the rules, the bonds and the desk's marks are read; the marks
  // read again later are merged into them.
  useEffect(() => {
    if (universe.data && isDeskRead) setRows(sheetRows(universe.data.rules, universe.data.bonds, deskRef.current))
  }, [universe.data, isDeskRead])

  useEffect(() => {
    if (deskData) setRows((current) => withMarks(current, deskData, new Set(inFlight.current.keys())))
  }, [deskData])

  // Rows keep the rules' order, so a bond's row is found by its rule's position.
  const instrumentIds = useMemo(() => universe.data?.rules.map((rule) => rule.instrument_id) ?? [], [universe.data])
  const indexById = useMemo(() => new Map(instrumentIds.map((id, index) => [id, index])), [instrumentIds])

  // Results arrive in bursts; they are merged once a frame.
  const flush = useCallback(() => {
    frame.current = null
    const arrived = pending.current
    pending.current = []
    const receivedAt = Date.now()
    setRows((current) => {
      let next = current
      for (const valuation of arrived) {
        const index = indexById.get(instrumentOf(valuation) ?? '')
        const row = index === undefined ? undefined : next[index]
        if (index === undefined || row === undefined) continue
        const merged = withValuation(row, valuation, receivedAt)
        if (merged === row) continue
        if (next === current) next = [...current]
        next[index] = merged
      }
      return next
    })
  }, [indexById])

  useEffect(() => {
    if (instrumentIds.length === 0) return
    return followValuations(token, instrumentIds, {
      onValuation: (valuation) => {
        pending.current.push(valuation)
        if (frame.current === null) frame.current = requestAnimationFrame(flush)
      },
      onStatus: setStatus,
      onRefusal: (code, message) => setNotice(`${code}: ${message}`),
    })
  }, [instrumentIds, token, flush])

  const setMark = useCallback((index: number, mark: DeskMark | null) => {
    setRows((current) => {
      const row = current[index]
      if (!row) return current
      const next = [...current]
      next[index] = { ...row, mark }
      return next
    })
  }, [])

  // One mark in flight a bond: a level set meanwhile waits, and only the latest is sent.
  const mark = useCallback(
    (index: number, level: string) => {
      const row = rowsRef.current[index]
      if (!deskSource || !row?.deskField) return
      const field = row.deskField
      const id = row.instrumentId
      setMark(index, { level, marked_at: new Date().toISOString() })
      if (inFlight.current.has(id)) {
        inFlight.current.set(id, level)
        return
      }
      const send = async (sent: string, previous: DeskMark | null) => {
        inFlight.current.set(id, null)
        try {
          const reply = await call<{ receive_time_ms: number }>('/api/v1/market/levels', {
            method: 'POST',
            body: { source: deskSource, levels: [{ instrument_id: id, field, level: sent }] },
          })
          // The server's receipt time orders this mark against the marks read back later.
          if (inFlight.current.get(id) === null) {
            setMark(index, { level: sent, marked_at: new Date(reply.data.receive_time_ms).toISOString() })
          }
          setNotice(null)
        } catch (error) {
          setMark(index, previous)
          setNotice(error instanceof ApiFailure ? error.message : String(error))
        }
        const next = inFlight.current.get(id)
        inFlight.current.delete(id)
        if (next) void send(next, rowsRef.current[index]?.mark ?? null)
      }
      void send(level, row.mark)
    },
    [call, deskSource, setMark],
  )

  const onMark = useCallback(
    (index: number, level: string | null) => {
      setEditing(null)
      if (level !== null) mark(index, level)
    },
    [mark],
  )

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (editing !== null) return
    const row = rows[selected]
    const nudge = (direction: 1 | -1) => {
      if (!row?.deskField || !row.mark) return
      event.preventDefault()
      mark(selected, nudgedLevel(row.mark.level, row.deskField, direction, event.altKey))
    }
    if (event.key === 'ArrowDown' && !event.shiftKey) {
      event.preventDefault()
      setSelected((index) => Math.min(rows.length - 1, index + 1))
    } else if (event.key === 'ArrowUp' && !event.shiftKey) {
      event.preventDefault()
      setSelected((index) => Math.max(0, index - 1))
    } else if (event.key === '+' || event.key === '=' || (event.shiftKey && event.key === 'ArrowUp')) {
      nudge(1)
    } else if (event.key === '-' || event.key === '_' || (event.shiftKey && event.key === 'ArrowDown')) {
      nudge(-1)
    } else if (event.key === 'Enter' && row?.deskField) {
      event.preventDefault()
      setEditing(selected)
    }
  }

  if (universe.isPending || desk.isPending) return <p className="text-sm text-slate-400">Reading pricing rules and bonds…</p>
  if (universe.error) return <p className="text-sm text-red-400">{String(universe.error)}</p>

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-4 text-xs text-slate-400">
        <span>
          {rows.length} bonds · stream <StreamBadge status={status} />
        </span>
        <span>
          {deskSource
            ? `Desk levels on ${deskSource}: ↑↓ select · + / − nudge 1 bp (Alt: ¼ bp) · Enter types a level`
            : desk.error
              ? `Desk levels could not be read (${desk.error instanceof ApiFailure ? desk.error.message : String(desk.error)}): levels are read-only.`
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
            {rows.map((row, index) => (
              <Row
                key={row.instrumentId}
                row={row}
                index={index}
                selected={index === selected}
                editing={editing === index}
                onSelect={setSelected}
                onMark={onMark}
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

// One clock for every cell that reads the time, so a second's tick re-renders those cells only.
const clock = (() => {
  let now = Date.now()
  const listeners = new Set<() => void>()
  let timer: ReturnType<typeof setInterval> | undefined
  return {
    subscribe(listener: () => void) {
      listeners.add(listener)
      timer ??= setInterval(() => {
        now = Date.now()
        listeners.forEach((notify) => notify())
      }, 1000)
      return () => {
        listeners.delete(listener)
        if (listeners.size === 0) {
          clearInterval(timer)
          timer = undefined
        }
      }
    },
    now: () => now,
  }
})()

function useNow(): number {
  return useSyncExternalStore(clock.subscribe, clock.now)
}

const STATE_TONE: Record<RowState, string> = {
  valid: 'text-emerald-400',
  partial: 'text-amber-300',
  stale: 'text-amber-300',
  expired: 'text-amber-300',
  failed: 'text-red-400',
  awaiting: 'text-slate-500',
}

function StateCell({ valuation }: { valuation: PublishedValuation | null }) {
  const state = rowState(valuation, useNow())
  return <td className={`px-2 py-0.5 ${STATE_TONE[state]}`}>{state}</td>
}

function AgeCell({ markedAt }: { markedAt: string | null }) {
  const now = useNow()
  return <td className="px-2 py-0.5 font-mono text-slate-500">{markedAt ? formatAge(markedAt, now) : ''}</td>
}

const Row = memo(function Row({
  row,
  index,
  selected,
  editing,
  onSelect,
  onMark,
}: {
  row: SheetRow
  index: number
  selected: boolean
  editing: boolean
  onSelect: (index: number) => void
  onMark: (index: number, level: string | null) => void
}) {
  const thirtySeconds = isQuotedInThirtySeconds(row.bond)
  const cell = (name: string) => {
    const value = measure(row.valuation, name)
    return value ? formatMeasure(value.value.value, value.value.unit, thirtySeconds) : ''
  }
  const level = shownLevel(row)
  const firstError = row.valuation?.result.errors[0] as { message?: string } | undefined
  return (
    <tr
      onClick={() => onSelect(index)}
      className={`border-t border-slate-900 ${selected ? 'bg-slate-800' : 'hover:bg-slate-900'}`}
      title={firstError?.message}
    >
      <td className="px-2 py-0.5 text-slate-200">{row.bond?.description ?? row.instrumentId}</td>
      <td className="px-2 py-0.5 font-mono text-slate-400">{row.bond?.cusip ?? row.bond?.isin ?? ''}</td>
      <td className="px-2 py-0.5 font-mono tabular-nums">{formatCoupon(row.bond?.coupon_rate)}</td>
      <td className="px-2 py-0.5 font-mono tabular-nums text-slate-400">{row.bond?.maturity_date ?? ''}</td>
      <td className="px-2 py-0.5 text-slate-400">{methodLabel(row.rule.method)}</td>
      <td className={`px-2 py-0.5 text-right font-mono tabular-nums ${row.deskField ? 'text-sky-300' : ''}`}>
        {editing ? (
          <LevelInput initial={row.mark?.level ?? ''} onDone={(text) => onMark(index, text)} />
        ) : level === null ? (
          ''
        ) : (
          formatMeasure(level.value, level.unit, thirtySeconds)
        )}
      </td>
      {/* Keyed by the tick, so each move restarts its colour's fade. */}
      <td
        key={row.tick?.at ?? 'still'}
        className={`px-2 py-0.5 text-right font-mono tabular-nums ${row.tick ? `tick-${row.tick.move}` : ''}`}
      >
        {cell('clean_price_mid')}
      </td>
      <td className="px-2 py-0.5 text-right font-mono tabular-nums">{cell('ytm_mid') || cell('ytw_mid')}</td>
      <td className="px-2 py-0.5 text-right font-mono tabular-nums">{cell('z_spread_mid')}</td>
      <td className="px-2 py-0.5 text-right font-mono tabular-nums">{cell('g_spread_mid')}</td>
      <td className="px-2 py-0.5 text-right font-mono tabular-nums">{cell('dv01_mid')}</td>
      <StateCell valuation={row.valuation} />
      <AgeCell markedAt={row.mark?.marked_at ?? null} />
    </tr>
  )
})

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
