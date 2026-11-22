import { useMutation } from '@tanstack/react-query'
import { ApiFailure, useSession } from '@convex/api-client'
import { useState, type ReactNode } from 'react'
import { isDecimal } from '../lib/decimal'
import { formatMeasure, measureUnit } from '../lib/format'
import type { MethodKind, PricingMethod } from '../lib/sheet'

// "Given this level, what are the others": one level of a stored bond, by any method,
// at a settlement date, through `POST /api/v1/bonds/:instrument_id/calculate`. The
// level is sent as typed, an exact decimal in the method's unit.

const METHODS: { kind: MethodKind; label: string; unit: string }[] = [
  { kind: 'price', label: 'Price', unit: 'clean, per 100' },
  { kind: 'yield', label: 'Yield', unit: 'decimal, 0.0525 for 5.25%' },
  { kind: 'benchmark_spread', label: 'Benchmark spread', unit: 'bp over the tenor' },
  { kind: 'z_spread', label: 'Z-spread', unit: 'bp' },
  { kind: 'discount_margin', label: 'Discount margin', unit: 'bp' },
  { kind: 'oas', label: 'OAS', unit: 'bp' },
]

interface Workout {
  date: string
  yield: number | string
  call_price?: number | string
  kind?: string
}

/** The reply: the bond's measures as flat members, beside what was read. */
interface CalculationReply {
  [member: string]: unknown
  input?: { benchmark?: { tenor?: string; ytm?: number | string } }
  workouts?: Workout[]
  worst?: Workout & { years?: number | string }
  measure_errors?: { measure?: string; message?: string }[]
  option_model?: Record<string, unknown>
}

const inputClass =
  'rounded border border-slate-700 bg-slate-950 px-2 py-1 font-mono text-sm text-slate-100 focus:border-sky-600 focus:outline-none'

function today(): string {
  return new Date().toISOString().slice(0, 10)
}

export function Calculator() {
  const { call } = useSession()
  const [instrumentId, setInstrumentId] = useState('')
  const [kind, setKind] = useState<MethodKind>('price')
  const [tenor, setTenor] = useState('10Y')
  const [level, setLevel] = useState('')
  const [settlement, setSettlement] = useState(today)
  const calculation = useMutation({
    mutationFn: async () => {
      const method: PricingMethod = kind === 'benchmark_spread' ? { kind, tenor } : { kind }
      const body = { method, level, settlement_date: settlement }
      const path = `/api/v1/bonds/${encodeURIComponent(instrumentId)}/calculate`
      return (await call<CalculationReply>(path, { method: 'POST', body })).data
    },
  })
  const ready = instrumentId.trim() !== '' && isDecimal(level) && /^\d{4}-\d{2}-\d{2}$/.test(settlement)
  const unit = METHODS.find((method) => method.kind === kind)?.unit ?? ''

  return (
    <div className="grid gap-6 lg:grid-cols-[22rem_1fr]">
      <section className="flex flex-col gap-3 rounded border border-slate-800 p-4">
        <Field label="Instrument id">
          <input className={inputClass} value={instrumentId} onChange={(e) => setInstrumentId(e.target.value.trim())} />
        </Field>
        <Field label="Quoted by">
          <select className={inputClass} value={kind} onChange={(e) => setKind(e.target.value as MethodKind)}>
            {METHODS.map((method) => (
              <option key={method.kind} value={method.kind}>
                {method.label}
              </option>
            ))}
          </select>
        </Field>
        {kind === 'benchmark_spread' && (
          <Field label="Benchmark tenor">
            <input className={inputClass} value={tenor} onChange={(e) => setTenor(e.target.value.trim())} />
          </Field>
        )}
        <Field label={`Level (${unit})`}>
          <input
            className={`${inputClass} ${level === '' || isDecimal(level) ? '' : 'border-red-700'}`}
            value={level}
            onChange={(e) => setLevel(e.target.value.trim())}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && ready) calculation.mutate()
            }}
          />
        </Field>
        <Field label="Settlement date">
          <input className={inputClass} type="date" value={settlement} onChange={(e) => setSettlement(e.target.value)} />
        </Field>
        <button
          type="button"
          disabled={!ready || calculation.isPending}
          onClick={() => calculation.mutate()}
          className="rounded bg-sky-700 px-3 py-1.5 text-sm font-medium text-white hover:bg-sky-600 disabled:opacity-40"
        >
          {calculation.isPending ? 'Calculating…' : 'Calculate'}
        </button>
      </section>
      <section className="flex flex-col gap-4">
        {calculation.error && <Refusal error={calculation.error} />}
        {calculation.data && <Reply reply={calculation.data} />}
      </section>
    </div>
  )
}

function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <label className="flex flex-col gap-1 text-xs text-slate-400">
      {label}
      {children}
    </label>
  )
}

function Refusal({ error }: { error: unknown }) {
  const text = error instanceof ApiFailure ? `${error.status} ${error.code}: ${error.reason}` : String(error)
  return <p className="rounded border border-red-900 bg-red-950 px-3 py-2 text-sm text-red-300">{text}</p>
}

/** Members of the reply that are not measures. */
const READ_MEMBERS = new Set(['input', 'pricing_rule', 'workouts', 'worst', 'snapshot', 'option_model', 'measure_errors'])

function Reply({ reply }: { reply: CalculationReply }) {
  const measures = Object.entries(reply).filter(
    (entry): entry is [string, number | string] =>
      !READ_MEMBERS.has(entry[0]) && (typeof entry[1] === 'number' || typeof entry[1] === 'string'),
  )
  return (
    <>
      <table className="w-full max-w-xl text-sm">
        <tbody>
          {measures.map(([name, value]) => {
            const unit = measureUnit(name)
            return (
              <tr key={name} className="border-t border-slate-900">
                <td className="py-0.5 pr-4 text-slate-400">{name}</td>
                <td className="py-0.5 text-right font-mono tabular-nums">{formatMeasure(value, unit ?? '')}</td>
                <td className="py-0.5 pl-2 text-xs text-slate-500">{unit === 'decimal_rate' ? '%' : (unit ?? '')}</td>
              </tr>
            )
          })}
        </tbody>
      </table>
      {reply.input?.benchmark && (
        <p className="text-xs text-slate-400">
          Quoted over the {reply.input.benchmark.tenor} benchmark at a yield of{' '}
          {reply.input.benchmark.ytm === undefined ? '—' : formatMeasure(reply.input.benchmark.ytm, 'decimal_rate')}%.
        </p>
      )}
      {reply.workouts && reply.workouts.length > 0 && (
        <table className="w-full max-w-xl text-sm">
          <thead className="text-xs text-slate-400">
            <tr>
              <th className="text-left font-medium">Workout</th>
              <th className="text-right font-medium">Call price</th>
              <th className="text-right font-medium">Yield %</th>
            </tr>
          </thead>
          <tbody>
            {reply.workouts.map((workout) => {
              const worst = reply.worst?.date === workout.date
              return (
                <tr key={workout.date} className={`border-t border-slate-900 ${worst ? 'text-sky-300' : ''}`}>
                  <td className="py-0.5 font-mono">
                    {workout.date}
                    {worst ? ' (worst)' : ''}
                  </td>
                  <td className="py-0.5 text-right font-mono tabular-nums">
                    {workout.call_price === undefined ? 'maturity' : formatMeasure(workout.call_price, 'price_per_100')}
                  </td>
                  <td className="py-0.5 text-right font-mono tabular-nums">{formatMeasure(workout.yield, 'decimal_rate')}</td>
                </tr>
              )
            })}
          </tbody>
        </table>
      )}
      {reply.measure_errors && reply.measure_errors.length > 0 && (
        <ul className="text-xs text-amber-300">
          {reply.measure_errors.map((error, index) => (
            <li key={index}>
              {error.measure}: {error.message}
            </li>
          ))}
        </ul>
      )}
      {reply.option_model && (
        <p className="text-xs text-slate-400">Valued on HW1F trees: {JSON.stringify(reply.option_model)}</p>
      )}
    </>
  )
}
