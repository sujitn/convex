import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'
import { PendingRestart } from '../components/PendingRestart'
import { Badge, ErrorNotice, JsonBlock, Panel, Table } from '../components/ui'
import { formatMeasure } from '../lib/admin'
import { useRuntimeHealth } from '../lib/queries'
import { useSession } from '../lib/session'
import type { Dependencies, PublishedValuation } from '../lib/types'

export function RuntimePage() {
  const { call } = useSession()
  const health = useRuntimeHealth()
  const dependencies = useQuery({
    queryKey: ['dependencies'],
    queryFn: async () => (await call<Dependencies>('/api/v1/dependencies')).data,
    refetchInterval: 5000,
  })
  const results = useQuery({
    queryKey: ['runtime-results'],
    queryFn: async () => (await call<PublishedValuation[]>('/api/v1/runtime/results')).data,
    refetchInterval: 5000,
  })
  const [selected, setSelected] = useState<number | null>(null)
  const runtime = health.data
  const valuations = results.data ?? []
  const chosen = selected === null ? undefined : valuations[selected]
  return (
    <div className="flex flex-col gap-4">
      <Panel title="Runtime">
        <ErrorNotice error={health.error} />
        {runtime && (
          <div className="flex flex-col gap-3">
            <div className="flex flex-wrap items-center gap-2">
              <Badge tone={runtime.ready ? 'good' : 'bad'}>{runtime.phase}</Badge>
              <Badge tone="neutral">mode {runtime.mode ?? 'none'}</Badge>
              <Badge tone={runtime.quality === 'Valid' ? 'good' : 'warn'}>quality {runtime.quality}</Badge>
              {runtime.failed_instruments > 0 && (
                <Badge tone="warn">{runtime.failed_instruments} failed instruments</Badge>
              )}
              {runtime.failure && <Badge tone="bad">{runtime.failure}</Badge>}
            </div>
            <Table
              columns={['Configuration', 'Version', 'Accepted revision', 'Generation', 'Result revision', 'Undelivered', 'Snapshot']}
              rows={[
                [
                  runtime.configuration_id,
                  runtime.configuration_version,
                  runtime.activation_revision,
                  runtime.generation,
                  runtime.revision,
                  runtime.pending_publications,
                  runtime.snapshot_id?.slice(0, 19),
                ],
              ]}
            />
            <PendingRestart configurationId={runtime.configuration_id} />
          </div>
        )}
      </Panel>
      <Panel title="Dependencies">
        <ErrorNotice error={dependencies.error} />
        <Table
          columns={['Dependency', 'Required', 'State']}
          rows={(dependencies.data?.dependencies ?? []).map((d) => [
            d.name,
            d.required ? 'yes' : 'no',
            <Badge key={d.name} tone={d.state === 'ready' ? 'good' : d.required ? 'bad' : 'warn'}>
              {d.state}
            </Badge>,
          ])}
        />
      </Panel>
      <Panel title={`Results (${valuations.length})`}>
        <ErrorNotice error={results.error} />
        <Table
          columns={['Instrument', 'Status', 'Quality', 'Measures', 'Errors', 'Revision', 'Valid until']}
          selected={selected}
          onSelect={setSelected}
          rows={valuations.map((v) => [
            v.result.provenance.subject.instrument_id ?? v.runtime_id,
            v.result.status,
            v.result.quality,
            Object.keys(v.result.results).length,
            v.result.errors.length,
            v.revision,
            v.live_valid_until,
          ])}
        />
        {chosen && (
          <div className="mt-4 grid gap-4 lg:grid-cols-2">
            <Table
              columns={['Measure', 'Value', 'Quality', 'Method']}
              rows={Object.entries(chosen.result.results).map(([name, m]) => [
                name,
                formatMeasure(m.value),
                m.quality,
                m.method,
              ])}
            />
            <JsonBlock value={chosen} />
          </div>
        )}
      </Panel>
    </div>
  )
}
