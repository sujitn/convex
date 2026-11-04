import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useState, type ChangeEvent } from 'react'
import { PendingRestart } from '../components/PendingRestart'
import { Badge, Button, ErrorNotice, Field, JsonBlock, JsonEditor, Panel, Table, inputClass, parseJson } from '../components/ui'
import { ApiFailure, configurationWriteHeaders, newIdempotencyKey, rawObject } from '../lib/admin'
import { useRecentActivations, useRuntimeHealth } from '../lib/queries'
import { useSession } from '../lib/session'
import type { ActivationOutcome, StagedVersion, VersionDiff } from '../lib/types'

export function ConfigurationPage() {
  const health = useRuntimeHealth()
  const [chosen, setChosen] = useState<string | null>(null)
  const configurationId = chosen ?? health.data?.configuration_id ?? null
  return (
    <div className="flex flex-col gap-4">
      <Panel
        title="Configuration"
        actions={
          <input
            aria-label="Configuration id"
            value={configurationId ?? ''}
            placeholder="configuration id"
            onChange={(event) => setChosen(event.target.value.trim() || null)}
            className={inputClass}
          />
        }
      >
        {configurationId ? (
          <Activations configurationId={configurationId} />
        ) : (
          <p className="text-sm text-slate-600">The runtime names no configuration; enter one.</p>
        )}
      </Panel>
      {configurationId && (
        <>
          <Versions configurationId={configurationId} />
          <Changes configurationId={configurationId} />
        </>
      )}
    </div>
  )
}

function Activations({ configurationId }: { configurationId: string }) {
  const activations = useRecentActivations(configurationId)
  const items = activations.data?.items ?? []
  return (
    <div className="flex flex-col gap-3">
      <PendingRestart configurationId={configurationId} />
      <ErrorNotice error={activations.error} />
      <Table
        columns={['Revision', 'Action', 'Version', 'Mode', 'Actor', 'Reason', 'Activated', 'Fixtures']}
        rows={items.map((a, index) => [
          index === 0 ? (
            <Badge key="active" tone="good">
              {a.revision} active
            </Badge>
          ) : (
            a.revision
          ),
          a.command.action.kind + (a.command.action.to_revision ? ` to ${a.command.action.to_revision}` : ''),
          a.command.configuration_version,
          a.command.mode,
          a.command.actor,
          a.command.reason,
          a.command.activated_at,
          a.command.fixture_evidence ?? 'replay_checked',
        ])}
      />
      {activations.data && (
        <p className="text-xs text-slate-500">
          Newest {items.length} of {activations.data.total} activations.
        </p>
      )}
    </div>
  )
}

function Versions({ configurationId }: { configurationId: string }) {
  const { call } = useSession()
  const base = `/api/v1/configuration/${encodeURIComponent(configurationId)}`
  const [version, setVersion] = useState('1')
  const [range, setRange] = useState({ from: '1', to: '2' })
  const staged = useMutation({
    mutationFn: async () => (await call<StagedVersion>(`${base}/versions/${Number(version)}`)).data,
  })
  const diff = useMutation({
    mutationFn: async () =>
      (await call<VersionDiff>(`${base}/diff?from=${Number(range.from)}&to=${Number(range.to)}`)).data,
  })
  return (
    <div className="grid gap-4 lg:grid-cols-2">
      <Panel title="Staged version">
        <div className="flex flex-col gap-3">
          <div className="flex items-end gap-2">
            <Field label="Version">
              <input value={version} onChange={(e) => setVersion(e.target.value)} className={`${inputClass} w-24`} />
            </Field>
            <Button tone="plain" onClick={() => staged.mutate()}>
              Show
            </Button>
          </div>
          <ErrorNotice error={staged.error} />
          {staged.data && (
            <>
              <Table
                columns={['Actor', 'Reason', 'Staged', 'Fixtures', 'Digest']}
                rows={[
                  [
                    staged.data.actor,
                    staged.data.reason,
                    staged.data.staged_at,
                    staged.data.fixture_evidence,
                    staged.data.digest.slice(0, 16),
                  ],
                ]}
              />
              <JsonBlock value={staged.data.configuration} />
            </>
          )}
        </div>
      </Panel>
      <Panel title="Compare versions">
        <div className="flex flex-col gap-3">
          <div className="flex items-end gap-2">
            <Field label="From">
              <input value={range.from} onChange={(e) => setRange({ ...range, from: e.target.value })} className={`${inputClass} w-24`} />
            </Field>
            <Field label="To">
              <input value={range.to} onChange={(e) => setRange({ ...range, to: e.target.value })} className={`${inputClass} w-24`} />
            </Field>
            <Button tone="plain" onClick={() => diff.mutate()}>
              Compare
            </Button>
          </div>
          <ErrorNotice error={diff.error} />
          {diff.data && (
            <Table
              columns={['Changed field', 'Applies']}
              rows={diff.data.changes.map((c) => [
                c.field,
                <Badge key={c.field} tone={c.reload === 'hot' ? 'good' : 'warn'}>
                  {c.reload}
                </Badge>,
              ])}
            />
          )}
        </div>
      </Panel>
    </div>
  )
}

async function readFile(event: ChangeEvent<HTMLInputElement>, set: (text: string) => void) {
  const file = event.target.files?.[0]
  if (file) set(await file.text())
}

/** Validate and stage a document, then activate a version or roll back to a revision. */
function Changes({ configurationId }: { configurationId: string }) {
  const { call } = useSession()
  const client = useQueryClient()
  const activations = useRecentActivations(configurationId)
  const active = activations.data?.items[0]?.revision ?? 0
  const base = `/api/v1/configuration/${encodeURIComponent(configurationId)}`
  const [documentText, setDocumentText] = useState('')
  const [fixturesText, setFixturesText] = useState('')
  const [reason, setReason] = useState('')
  const [target, setTarget] = useState({ action: 'activate', number: '', mode: 'hot', expected: '' })

  // The document and fixtures are sent as the operator wrote them; parsing only
  // checks their syntax.
  const checked = (...texts: string[]) => {
    for (const text of texts) {
      const parsed = parseJson(text)
      if (parsed.error) throw parsed.error
    }
  }
  const validate = useMutation({
    mutationFn: async () => {
      checked(documentText, fixturesText)
      return (
        await call<unknown>('/api/v1/configuration/validate', {
          method: 'POST',
          raw: rawObject({ document: documentText, fixtures: fixturesText }),
        })
      ).data
    },
  })
  const stage = useMutation({
    mutationFn: async () => {
      checked(documentText, fixturesText)
      return (
        await call<unknown>(`${base}/stage`, {
          method: 'POST',
          headers: configurationWriteHeaders(newIdempotencyKey('stage')),
          raw: rawObject({
            document: documentText,
            fixtures: fixturesText,
            reason: JSON.stringify(reason),
            staged_at: JSON.stringify(new Date().toISOString()),
          }),
        })
      ).data
    },
  })
  const activate = useMutation({
    mutationFn: async () => {
      checked(fixturesText)
      const rollback = target.action === 'rollback'
      const number = target.number.trim()
      if (!/^[1-9][0-9]*$/.test(number)) {
        throw new ApiFailure(0, 'INVALID_INPUT', `${rollback ? 'revision' : 'version'} must be a positive integer`)
      }
      const expected = target.expected === '' ? active : Number(target.expected)
      return (
        await call<ActivationOutcome>(`${base}/${rollback ? 'rollback' : 'activate'}`, {
          method: 'POST',
          headers: configurationWriteHeaders(newIdempotencyKey(target.action), expected),
          raw: rawObject({
            [rollback ? 'to_revision' : 'configuration_version']: number,
            reason: JSON.stringify(reason),
            activated_at: JSON.stringify(new Date().toISOString()),
            mode: JSON.stringify(target.mode),
            fixtures: fixturesText,
          }),
        })
      ).data
    },
    onSuccess: () => client.invalidateQueries({ queryKey: ['activations', configurationId] }),
  })

  const outcome = activate.data
  return (
    <Panel title="Change">
      <div className="flex flex-col gap-4">
        <p className="text-sm text-slate-600">
          Every change carries one to eight fixtures. With a replay quotes file on the server, they must prefix it;
          otherwise the records say the fixtures were operator-supplied. A restart-required activation commits now
          and applies at the next start.
        </p>
        <div className="grid gap-4 lg:grid-cols-2">
          <Field label="Document (JSON)">
            <input type="file" accept=".json" onChange={(e) => void readFile(e, setDocumentText)} className="text-sm" />
            <JsonEditor value={documentText} onChange={setDocumentText} rows={14} />
          </Field>
          <Field label="Fixtures (JSON array)">
            <input type="file" accept=".json" onChange={(e) => void readFile(e, setFixturesText)} className="text-sm" />
            <JsonEditor value={fixturesText} onChange={setFixturesText} rows={14} />
          </Field>
        </div>
        <Field label="Reason (recorded with the change)">
          <input value={reason} onChange={(e) => setReason(e.target.value)} className={inputClass} />
        </Field>
        <div className="flex flex-wrap gap-2">
          <Button tone="plain" onClick={() => validate.mutate()} disabled={validate.isPending}>
            Validate document
          </Button>
          <Button onClick={() => stage.mutate()} disabled={stage.isPending || !reason}>
            Stage document
          </Button>
        </div>
        <ErrorNotice error={validate.error ?? stage.error} />
        {validate.data !== undefined && <JsonBlock value={validate.data} />}
        {stage.data !== undefined && <JsonBlock value={stage.data} />}

        <div className="flex flex-wrap items-end gap-3 border-t border-slate-200 pt-4">
          <Field label="Action">
            <select value={target.action} onChange={(e) => setTarget({ ...target, action: e.target.value })} className={inputClass}>
              <option value="activate">activate version</option>
              <option value="rollback">roll back to revision</option>
            </select>
          </Field>
          <Field label={target.action === 'rollback' ? 'Revision' : 'Version'}>
            <input value={target.number} onChange={(e) => setTarget({ ...target, number: e.target.value })} className={`${inputClass} w-24`} />
          </Field>
          <Field label="Mode">
            <select value={target.mode} onChange={(e) => setTarget({ ...target, mode: e.target.value })} className={inputClass}>
              <option value="hot">hot</option>
              <option value="restart_required">restart_required</option>
            </select>
          </Field>
          <Field label={`Expected active revision (If-Match, now ${active})`}>
            <input
              value={target.expected}
              placeholder={String(active)}
              onChange={(e) => setTarget({ ...target, expected: e.target.value })}
              className={`${inputClass} w-24`}
            />
          </Field>
          <Button tone="danger" onClick={() => activate.mutate()} disabled={activate.isPending || !reason || !target.number}>
            {target.action === 'rollback' ? 'Roll back' : 'Activate'}
          </Button>
        </div>
        <ErrorNotice error={activate.error} />
        {outcome && (
          <div className="flex flex-col gap-2">
            <p className="text-sm">
              {outcome.restart_required ? (
                <Badge tone="warn">restart required</Badge>
              ) : (
                <Badge tone="good">applied</Badge>
              )}{' '}
              Revision {outcome.activation.revision} is active.{' '}
              {outcome.restart_required
                ? 'The running runtime keeps its selection until the next start.'
                : `The runtime accepted it at generation ${outcome.accepted_generation ?? '?'}.`}
            </p>
            <JsonBlock value={outcome} />
          </div>
        )}
      </div>
    </Panel>
  )
}
