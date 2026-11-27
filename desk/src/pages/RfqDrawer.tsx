import { useQuery } from '@tanstack/react-query'
import { useSession } from '@convex/api-client'
import { memo, type ReactNode } from 'react'
import { failureNotice } from '../lib/failure'
import { formatMeasure, formatPrice, formatQuoteValue } from '../lib/format'
import {
  boundsLabel,
  formatLimitValue,
  LIMIT_TONE,
  measureLabel,
  unknownReasonText,
  type LimitEvaluation,
} from '../lib/limits'
import { readRfqRecords } from '../lib/reads'
import {
  formatFace,
  formatQuotedPrice,
  stateLabel,
  type RecordedAutoPrice,
  type RfqRecord,
  type RfqRow,
  type RuleRead,
} from '../lib/rfqs'

// One request's entries, read again each time the request moves: what it was received as,
// what its auto price and check were made from, and who sent each event and when.

export function RfqDrawer({ row, thirtySeconds, onClose }: { row: RfqRow; thirtySeconds: boolean; onClose: () => void }) {
  const { call } = useSession()
  // Keyed by the latest entry, so the entries read never change.
  const records = useQuery({
    queryKey: ['rfq-records', row.rfq_key, row.record_id],
    queryFn: () => readRfqRecords(call, row.rfq_key),
    staleTime: Infinity,
  })
  return (
    <aside className="w-[28rem] shrink-0 overflow-auto rounded border border-slate-800 bg-slate-900 p-3 text-xs">
      <div className="mb-2 flex items-center justify-between">
        <span className="font-semibold text-slate-200">
          {row.request.client_id} {row.request.side} {formatFace(row.request.face)} {row.instrument_id}
        </span>
        <button type="button" className="rounded px-2 py-0.5 text-slate-400 hover:bg-slate-800" onClick={onClose}>
          Close
        </button>
      </div>
      <p className="mb-3 break-all font-mono text-slate-500">{row.rfq_key}</p>
      {records.isPending && <p className="text-slate-400">Reading the request's entries…</p>}
      {records.error && <p className="text-red-400">{failureNotice(records.error)}</p>}
      {records.data?.map((record) => <Entry key={record.record_id} record={record} thirtySeconds={thirtySeconds} />)}
    </aside>
  )
}

const Entry = memo(function Entry({ record, thirtySeconds }: { record: RfqRecord; thirtySeconds: boolean }) {
  return (
    <section className="mb-3 border-t border-slate-800 pt-2">
      <h3 className="mb-1 flex justify-between text-slate-200">
        <span className="font-semibold">{stateLabel(record.state)}</span>
        <span className="text-slate-500">
          {record.actor} · {record.recorded_at} · #{record.record_id}
        </span>
      </h3>
      {record.user && <Field name="Sent by">{record.user}</Field>}
      {record.state === 'received' && (
        <>
          <Field name="Source">
            {record.request.source_id} · {record.request.rfq_id}
          </Field>
          <Field name="Client">
            {record.request.client_id} · tier {record.request.client_tier}
          </Field>
          <Field name="Side, size">
            {record.request.side} {record.request.face}
          </Field>
          <Field name="Bond">
            {record.instrument_id} v{record.instrument_version}
            {record.request.identifier && ` (named by ${record.request.identifier.kind} ${record.request.identifier.value})`}
          </Field>
          <Field name="Settlement">{record.request.settlement_date}</Field>
          <Field name="Book">{record.request.book_id}</Field>
          <Field name="Deadline">{record.request.deadline}</Field>
          <Field name="Received">{record.received_at}</Field>
        </>
      )}
      {record.state === 'priced' && (
        <>
          <Field name="Configuration">
            {record.configuration.configuration_id} v{record.configuration.version} · activation{' '}
            {record.configuration.activation_revision}
          </Field>
          <Field name="Client tier">
            {record.configuration.client_tier} · width × {record.configuration.width_factor}
          </Field>
          <AutoPrice autoPrice={record.auto_price} thirtySeconds={thirtySeconds} />
          <Field name="Priced at">{record.priced_at}</Field>
          {record.check.kind === 'evaluated' ? (
            <Limits limits={record.check.status.limits} />
          ) : (
            <Field name="No check">{record.check.reason}</Field>
          )}
        </>
      )}
      {record.state === 'quoted' && (
        <>
          <Field name="Clean price">{formatQuotedPrice(record.clean_price, thirtySeconds)}</Field>
          <Field name="Quoted at">{record.quoted_at}</Field>
        </>
      )}
      {record.state === 'expired' && <Field name="Reason">{record.reason}</Field>}
      <details className="mt-1 text-slate-500">
        <summary className="cursor-pointer">Entry</summary>
        <pre className="mt-1 overflow-auto whitespace-pre-wrap break-all font-mono text-[11px]">
          {JSON.stringify(record, null, 2)}
        </pre>
      </details>
    </section>
  )
})

function Field({ name, children }: { name: string; children: ReactNode }) {
  return (
    <div className="flex gap-2 py-px">
      <span className="w-32 shrink-0 text-slate-500">{name}</span>
      <span className="min-w-0 break-words font-mono text-slate-300">{children}</span>
    </div>
  )
}

function AutoPrice({ autoPrice, thirtySeconds }: { autoPrice: RecordedAutoPrice; thirtySeconds: boolean }) {
  if (autoPrice.kind === 'unpriced') return <Field name="Unpriced">{autoPrice.reason}</Field>
  const price = (value: string) => formatMeasure(value, 'price_per_100', thirtySeconds)
  const quoted = autoPrice.quote_input
  return (
    <>
      <Field name="Desk side">{autoPrice.desk_side}</Field>
      {autoPrice.level && <Field name="Level">{formatQuoteValue(autoPrice.level)}</Field>}
      <Field name="Clean price">
        {price(autoPrice.clean_price)} <span className="text-slate-500">({autoPrice.clean_price})</span>
      </Field>
      <Field name="Clean mid">{price(autoPrice.clean_price_mid)}</Field>
      {autoPrice.mid_only && <Field name="Mid only">{autoPrice.mid_only}</Field>}
      {quoted && (
        <Field name="Live quote">
          {quoted.field} {formatQuoteValue(quoted.observation.value, thirtySeconds)} · seq {quoted.observation.sequence} ·
          received {new Date(quoted.observation.receive_time_ms).toISOString()}
        </Field>
      )}
      {autoPrice.pricing_rule && <Rule rule={autoPrice.pricing_rule} />}
      {autoPrice.curves.length > 0 && (
        <Field name="Curves">{autoPrice.curves.map((curve) => `${curve.curve_id} v${curve.version}`).join(', ')}</Field>
      )}
    </>
  )
}

function Rule({ rule }: { rule: RuleRead }) {
  const { benchmark_set: set, benchmark, two_way: twoWay } = rule
  return (
    <>
      <Field name="Rule">
        v{rule.version} · {rule.method.kind}
        {rule.method.tenor ? ` ${rule.method.tenor}` : ''} · {rule.model}
      </Field>
      {set && (
        <Field name="Benchmark set">
          {set.benchmark_set_id} v{set.version}
          {set.securities.map((security) => (
            <div key={security.tenor} className="text-slate-400">
              {security.tenor} {security.instrument_id} v{security.version} at {formatPrice(security.clean_price_mid)}
            </div>
          ))}
        </Field>
      )}
      {benchmark && (
        <Field name="Benchmark">
          {benchmark.tenor} {benchmark.instrument_id} ytm {String(benchmark.ytm)}
        </Field>
      )}
      {twoWay && (
        <Field name="Two-way">
          width {twoWay.width} {twoWay.unit} · bid {twoWay.bid} · ask {twoWay.ask}
        </Field>
      )}
      {rule.g_spread_refused && <Field name="No G-spread">{rule.g_spread_refused}</Field>}
    </>
  )
}

/** Each limit as the check evaluated it with the trade done. */
function Limits({ limits }: { limits: LimitEvaluation[] }) {
  return (
    <table className="mt-1 w-full text-left">
      <thead className="text-slate-500">
        <tr>
          <th className="font-normal">Limit</th>
          <th className="font-normal">Status</th>
          <th className="text-right font-normal">Value</th>
          <th className="text-right font-normal">Bounds</th>
        </tr>
      </thead>
      <tbody className="font-mono text-slate-300">
        {limits.map((limit) => (
          <tr key={limit.limit_id} title={limit.reason && unknownReasonText(limit.reason)}>
            <td>
              {limit.limit_id} <span className="text-slate-500">{measureLabel(limit.measure)}</span>
              {limit.member && <span className="text-slate-500"> {limit.member}</span>}
            </td>
            <td className={LIMIT_TONE[limit.status]}>{limit.status}</td>
            <td className="text-right">{limit.value === undefined ? '' : formatLimitValue(limit.measure, limit.value)}</td>
            <td className="text-right">{boundsLabel(limit)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  )
}
