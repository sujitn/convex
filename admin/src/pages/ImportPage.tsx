import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import { Button, ErrorNotice, Field, JsonBlock, Panel } from '../components/ui'
import { ApiFailure, MAX_IMPORT_BYTES } from '../lib/admin'
import { useSession } from '../lib/session'

/** Upload one reference import file; it commits as one atomic transaction or not at all. */
export function ImportPage() {
  const { call } = useSession()
  const client = useQueryClient()
  const [file, setFile] = useState<File | null>(null)
  const upload = useMutation({
    mutationFn: async (chosen: File) => {
      if (chosen.size > MAX_IMPORT_BYTES) {
        throw new ApiFailure(0, 'FILE_TOO_LARGE', `the import is limited to ${MAX_IMPORT_BYTES} bytes`)
      }
      return (await call<unknown>('/api/v1/reference/import', { method: 'POST', raw: chosen })).data
    },
    onSuccess: () => client.invalidateQueries({ queryKey: ['reference'] }),
  })
  return (
    <Panel title="Reference import">
      <div className="flex flex-col gap-3">
        <p className="text-sm text-slate-600">
          A strict <span className="font-mono">{'{schema_version, command}'}</span> file, at most 8 MiB. Every row is
          checked before anything commits; a refused file names its section, row and field. Uploading the same file
          again returns the retained receipt.
        </p>
        <Field label="Import file">
          <input
            type="file"
            accept="application/json,.json"
            onChange={(event) => {
              setFile(event.target.files?.[0] ?? null)
              upload.reset()
            }}
            className="text-sm"
          />
        </Field>
        {file && (
          <p className="font-mono text-xs text-slate-600">
            {file.name}, {file.size} bytes
          </p>
        )}
        <div>
          <Button onClick={() => file && upload.mutate(file)} disabled={!file || upload.isPending}>
            Import
          </Button>
        </div>
        <ErrorNotice error={upload.error} />
        {upload.data !== undefined && (
          <div className="flex flex-col gap-1">
            <p className="text-sm font-medium text-emerald-800">Committed.</p>
            <JsonBlock value={upload.data} />
          </div>
        )}
      </div>
    </Panel>
  )
}
