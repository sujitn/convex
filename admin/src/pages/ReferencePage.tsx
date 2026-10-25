import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import { Button, ErrorNotice, Field, JsonBlock, JsonEditor, Pager, Panel, Table, inputClass, parseJson } from '../components/ui'
import {
  bondEditCommand,
  etagVersion,
  masterEditCommand,
  masterLabel,
  newIdempotencyKey,
  readExact,
  writeExact,
  type MasterRecord,
} from '../lib/admin'
import { useSession } from '../lib/session'
import type { BondPage } from '../lib/types'

const PAGE = 50
// Books keep their own screen and routes.
const KINDS = [
  'bond',
  'entity',
  'calendar',
  'convention',
  'rate_index',
  'identifier',
  'issuer_scope',
  'calculation',
  'futures',
  'basket',
  'cash_universe',
] as const

export function ReferencePage() {
  const [kind, setKind] = useState<(typeof KINDS)[number]>('bond')
  const [draft, setDraft] = useState<string | null>(null)
  return (
    <div className="flex flex-col gap-4">
      <Panel
        title="Reference data"
        actions={
          <select
            value={kind}
            onChange={(event) => {
              setKind(event.target.value as (typeof KINDS)[number])
              setDraft(null)
            }}
            className={inputClass}
          >
            {KINDS.map((k) => (
              <option key={k} value={k}>
                {k}
              </option>
            ))}
          </select>
        }
      >
        {kind === 'bond' ? <Bonds onEdit={setDraft} /> : <Masters key={kind} kind={kind} onEdit={setDraft} />}
      </Panel>
      {draft !== null && <TransactionEditor key={draft} initial={draft} onClose={() => setDraft(null)} />}
    </div>
  )
}

function Bonds({ onEdit }: { onEdit: (draft: string) => void }) {
  const { call } = useSession()
  const [offset, setOffset] = useState(0)
  const [selected, setSelected] = useState<string | null>(null)
  const page = useQuery({
    queryKey: ['reference', 'bond', offset],
    queryFn: async () => (await call<BondPage>(`/api/v1/bonds?offset=${offset}&limit=${PAGE}`)).data,
  })
  const record = useQuery({
    queryKey: ['reference', 'bond-record', selected],
    enabled: selected !== null,
    queryFn: async () => {
      const reply = await call<Record<string, unknown>>(`/api/v1/bonds/${encodeURIComponent(selected ?? '')}`)
      return { bond: reply.data, text: reply.text, version: etagVersion(reply.headers.get('ETag')) }
    },
  })
  const bonds = page.data?.bonds ?? []
  const current = record.data
  return (
    <div className="flex flex-col gap-3">
      <ErrorNotice error={page.error} />
      <Table
        columns={['Instrument', 'ISIN', 'Type', 'Currency', 'Maturity', 'Description']}
        onSelect={(index) => setSelected(String(bonds[index]?.instrument_id))}
        rows={bonds.map((b) => [
          String(b.instrument_id),
          b.isin as string | null,
          String(b.bond_type),
          String(b.currency),
          String(b.maturity_date),
          String(b.description),
        ])}
      />
      <Pager
        onFirst={() => setOffset(0)}
        onNext={() => setOffset(offset + PAGE)}
        hasNext={(page.data?.total ?? 0) > offset + PAGE}
      />
      <ErrorNotice error={record.error} />
      {current && (
        <div className="flex flex-col gap-2">
          <div className="flex items-center gap-3 text-sm">
            <span className="font-mono">
              {selected} v{current.version ?? '?'}
            </span>
            <Button
              tone="plain"
              disabled={current.version === null}
              onClick={() =>
                onEdit(
                  writeExact(
                    bondEditCommand(
                      readExact(current.text) as Record<string, unknown>,
                      current.version ?? 0,
                      newIdempotencyKey('bond'),
                    ),
                  ),
                )
              }
            >
              Edit as transaction
            </Button>
          </div>
          <JsonBlock value={current.bond} />
        </div>
      )}
    </div>
  )
}

interface MasterPage {
  items: MasterRecord[]
  next: string | null
}

function Masters({ kind, onEdit }: { kind: string; onEdit: (draft: string) => void }) {
  const { call } = useSession()
  const [after, setAfter] = useState<string | null>(null)
  const [selected, setSelected] = useState<number | null>(null)
  const page = useQuery({
    queryKey: ['reference', kind, after],
    queryFn: async () => {
      const cursor = after ? `&after=${encodeURIComponent(after)}` : ''
      const reply = await call<MasterPage>(`/api/v1/reference/masters/${kind}?limit=${PAGE}${cursor}`)
      return { ...reply.data, text: reply.text }
    },
  })
  const items = page.data?.items ?? []
  const chosen = selected === null ? undefined : items[selected]
  return (
    <div className="flex flex-col gap-3">
      <ErrorNotice error={page.error} />
      <Table
        columns={['Identity', 'Version', 'Recorded']}
        selected={selected}
        onSelect={setSelected}
        rows={items.map((item) => [masterLabel(item), item.version, item.recorded_at])}
      />
      <Pager
        onFirst={() => {
          setAfter(null)
          setSelected(null)
        }}
        onNext={() => {
          setAfter(page.data?.next ?? null)
          setSelected(null)
        }}
        hasNext={Boolean(page.data?.next)}
      />
      {chosen && (
        <div className="flex flex-col gap-2">
          <div>
            <Button
              tone="plain"
              onClick={() => {
                const exact = (readExact(page.data?.text ?? '') as MasterPage).items[selected ?? 0]
                if (exact) onEdit(writeExact(masterEditCommand(exact, newIdempotencyKey(kind))))
              }}
            >
              Edit as transaction
            </Button>
          </div>
          <JsonBlock value={chosen} />
        </div>
      )}
    </div>
  )
}

/**
 * Edit a whole reference command and submit it as one atomic transaction. The
 * header key is the body's own idempotency key, so a retry returns the retained
 * receipt and a changed body under the same key conflicts.
 */
function TransactionEditor({ initial, onClose }: { initial: string; onClose: () => void }) {
  const { call } = useSession()
  const client = useQueryClient()
  const [text, setText] = useState(initial)
  const submit = useMutation({
    mutationFn: async () => {
      const parsed = parseJson(text)
      if (parsed.error) throw parsed.error
      const key = (parsed.value as { idempotency_key?: unknown })?.idempotency_key
      return (
        await call<unknown>('/api/v1/reference/transactions', {
          method: 'POST',
          raw: text,
          headers: { 'Idempotency-Key': String(key ?? '') },
        })
      ).data
    },
    onSuccess: () => client.invalidateQueries({ queryKey: ['reference'] }),
  })
  return (
    <Panel
      title="Reference transaction"
      actions={
        <Button tone="plain" onClick={onClose}>
          Close
        </Button>
      }
    >
      <div className="flex flex-col gap-3">
        <p className="text-sm text-slate-600">
          The definition replaces the current version whole; omitted fields are not merged. The server checks every
          field and the expected version.
        </p>
        <Field label="Command">
          <JsonEditor value={text} onChange={setText} rows={24} />
        </Field>
        <div className="flex gap-2">
          <Button onClick={() => submit.mutate()} disabled={submit.isPending}>
            Commit transaction
          </Button>
        </div>
        <ErrorNotice error={submit.error} />
        {submit.data !== undefined && (
          <div className="flex flex-col gap-1">
            <p className="text-sm font-medium text-emerald-800">Committed.</p>
            <JsonBlock value={submit.data} />
          </div>
        )}
      </div>
    </Panel>
  )
}
