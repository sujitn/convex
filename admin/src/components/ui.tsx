import type { ReactNode } from 'react'
import { ApiFailure } from '../lib/admin'

// Small presentational pieces. Writes are buttons, never <form> submissions: the
// server's page policy sets form-action 'none'.

export function Panel({ title, actions, children }: { title: string; actions?: ReactNode; children: ReactNode }) {
  return (
    <section className="rounded-lg border border-slate-200 bg-white shadow-sm">
      <header className="flex items-center justify-between gap-4 border-b border-slate-200 px-4 py-2">
        <h2 className="text-sm font-semibold uppercase tracking-wide text-slate-600">{title}</h2>
        {actions && <div className="flex items-center gap-2">{actions}</div>}
      </header>
      <div className="p-4">{children}</div>
    </section>
  )
}

export function Button({
  children,
  onClick,
  disabled,
  tone = 'primary',
}: {
  children: ReactNode
  onClick: () => void
  disabled?: boolean
  tone?: 'primary' | 'plain' | 'danger'
}) {
  const tones = {
    primary: 'bg-slate-800 text-white hover:bg-slate-700',
    plain: 'border border-slate-300 bg-white text-slate-700 hover:bg-slate-100',
    danger: 'bg-red-700 text-white hover:bg-red-600',
  }
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className={`rounded px-3 py-1.5 text-sm font-medium disabled:cursor-not-allowed disabled:opacity-40 ${tones[tone]}`}
    >
      {children}
    </button>
  )
}

export function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <label className="flex flex-col gap-1 text-xs font-medium text-slate-600">
      {label}
      {children}
    </label>
  )
}

export const inputClass =
  'rounded border border-slate-300 px-2 py-1 font-mono text-sm text-slate-900 focus:border-slate-500 focus:outline-none'

/** A refusal with its code, reason and any importer diagnostics. */
export function ErrorNotice({ error }: { error: unknown }) {
  if (!error) return null
  const failure =
    error instanceof ApiFailure ? error : new ApiFailure(0, 'CLIENT', error instanceof Error ? error.message : String(error))
  return (
    <div role="alert" className="rounded border border-red-200 bg-red-50 p-3 text-sm text-red-900">
      <p>
        <span className="font-mono font-semibold">{failure.code}</span>
        {failure.status > 0 && <span className="text-red-700"> (HTTP {failure.status})</span>}
        {failure.reason && <span>: {failure.reason}</span>}
      </p>
      {failure.status === 401 && <p className="mt-1">Paste a bearer token above.</p>}
      {failure.status === 403 && <p className="mt-1">The token's identity lacks the role or grant for this.</p>}
      {failure.diagnostics.length > 0 && (
        <Table
          className="mt-2"
          columns={['Section', 'Row', 'Field', 'Code', 'Line', 'Column']}
          rows={failure.diagnostics.map((d) => [d.section, d.row, d.field, d.code, d.line, d.column])}
        />
      )}
    </div>
  )
}

export function Table({
  columns,
  rows,
  onSelect,
  selected,
  className = '',
}: {
  columns: string[]
  rows: ReactNode[][]
  onSelect?: (index: number) => void
  selected?: number | null
  className?: string
}) {
  if (rows.length === 0) return <p className={`text-sm text-slate-500 ${className}`}>Nothing to show.</p>
  return (
    <div className={`overflow-x-auto ${className}`}>
      <table className="min-w-full text-left text-sm">
        <thead>
          <tr className="border-b border-slate-200 text-xs uppercase text-slate-500">
            {columns.map((column) => (
              <th key={column} className="px-2 py-1 font-medium">
                {column}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((row, index) => (
            <tr
              key={index}
              onClick={onSelect ? () => onSelect(index) : undefined}
              className={`border-b border-slate-100 ${onSelect ? 'cursor-pointer hover:bg-slate-50' : ''} ${
                selected === index ? 'bg-sky-50' : ''
              }`}
            >
              {row.map((cell, column) => (
                <td key={column} className="whitespace-nowrap px-2 py-1 font-mono text-xs">
                  {cell ?? '—'}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}

export function JsonBlock({ value }: { value: unknown }) {
  return (
    <pre className="max-h-[28rem] overflow-auto rounded bg-slate-900 p-3 text-xs leading-relaxed text-slate-100">
      {JSON.stringify(value, null, 2)}
    </pre>
  )
}

export function JsonEditor({ value, onChange, rows = 18 }: { value: string; onChange: (value: string) => void; rows?: number }) {
  return (
    <textarea
      value={value}
      onChange={(event) => onChange(event.target.value)}
      rows={rows}
      spellCheck={false}
      className={`${inputClass} w-full leading-relaxed`}
    />
  )
}

/** Parse editor text, reporting a syntax error as a client-side refusal. */
export function parseJson(text: string): { value?: unknown; error?: ApiFailure } {
  try {
    return { value: JSON.parse(text) }
  } catch (error) {
    return { error: new ApiFailure(0, 'INVALID_JSON', error instanceof Error ? error.message : 'invalid JSON') }
  }
}

export function Badge({ children, tone }: { children: ReactNode; tone: 'good' | 'warn' | 'bad' | 'neutral' }) {
  const tones = {
    good: 'bg-emerald-100 text-emerald-800',
    warn: 'bg-amber-100 text-amber-900',
    bad: 'bg-red-100 text-red-800',
    neutral: 'bg-slate-100 text-slate-700',
  }
  return <span className={`rounded px-2 py-0.5 font-mono text-xs font-semibold ${tones[tone]}`}>{children}</span>
}

export function Pager({ onFirst, onNext, hasNext }: { onFirst: () => void; onNext: () => void; hasNext: boolean }) {
  return (
    <div className="mt-3 flex gap-2">
      <Button tone="plain" onClick={onFirst}>
        First page
      </Button>
      <Button tone="plain" onClick={onNext} disabled={!hasNext}>
        Next page
      </Button>
    </div>
  )
}
