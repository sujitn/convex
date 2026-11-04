import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'
import { ErrorNotice, JsonBlock, Pager, Panel, Table } from '../components/ui'
import { useSession } from '../lib/session'
import type { CurvePage } from '../lib/types'

const PAGE = 50

interface CurveNode {
  pillar: unknown
  quote: { unit: string; value: number }
}

/** Stored curve recipes, read-only (D41): recipes are written by the save route or the CLI. */
export function CurvesPage() {
  const { call } = useSession()
  const [after, setAfter] = useState<string | null>(null)
  const [selected, setSelected] = useState<string | null>(null)
  const page = useQuery({
    queryKey: ['curves', after],
    queryFn: async () => {
      const cursor = after ? `&after=${encodeURIComponent(after)}` : ''
      return (await call<CurvePage>(`/api/v1/curves?limit=${PAGE}${cursor}`)).data
    },
  })
  const recipe = useQuery({
    queryKey: ['curve', selected],
    enabled: selected !== null,
    queryFn: async () => (await call<Record<string, unknown>>(`/api/v1/curves/${encodeURIComponent(selected ?? '')}`)).data,
  })
  const curves = page.data?.curves ?? []
  const nodes = curveNodes(recipe.data)
  return (
    <Panel title="Curve recipes">
      <div className="flex flex-col gap-3">
        <ErrorNotice error={page.error} />
        <Table
          columns={['Curve', 'Latest version', 'Recorded']}
          selected={curves.findIndex((c) => c.curve_id === selected)}
          onSelect={(index) => setSelected(curves[index]?.curve_id ?? null)}
          rows={curves.map((c) => [c.curve_id, c.version, c.recorded_at])}
        />
        <Pager onFirst={() => setAfter(null)} onNext={() => setAfter(page.data?.next ?? null)} hasNext={Boolean(page.data?.next)} />
        <ErrorNotice error={recipe.error} />
        {recipe.data && (
          <div className="grid gap-4 lg:grid-cols-2">
            <Table
              columns={['Pillar', 'Quote', 'Unit']}
              rows={nodes.map((n) => [JSON.stringify(n.pillar), n.quote.value, n.quote.unit])}
            />
            <JsonBlock value={recipe.data} />
          </div>
        )}
      </div>
    </Panel>
  )
}

/** Direct recipes list their nodes; other calibrations show as JSON only. */
function curveNodes(record: Record<string, unknown> | undefined): CurveNode[] {
  const recipe = record?.recipe as { source?: { Nodes?: CurveNode[] } } | undefined
  return recipe?.source?.Nodes ?? []
}
