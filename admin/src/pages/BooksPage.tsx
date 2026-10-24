import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'
import { ErrorNotice, JsonBlock, Pager, Panel, Table } from '../components/ui'
import { useSession } from '../lib/session'
import type { BookPage } from '../lib/types'

const PAGE = 50

/** Books and their ingest history, read-only (D41): the BMS ingest API writes them. */
export function BooksPage() {
  const { call } = useSession()
  const [after, setAfter] = useState<string | null>(null)
  const [selected, setSelected] = useState<string | null>(null)
  const page = useQuery({
    queryKey: ['books', after],
    queryFn: async () => {
      const cursor = after ? `&after=${encodeURIComponent(after)}` : ''
      return (await call<BookPage>(`/api/v1/books?limit=${PAGE}${cursor}`)).data
    },
  })
  const book = useQuery({
    queryKey: ['book', selected],
    enabled: selected !== null,
    queryFn: async () => (await call<unknown>(`/api/v1/books/${encodeURIComponent(selected ?? '')}`)).data,
  })
  const history = useQuery({
    queryKey: ['book-history', selected],
    enabled: selected !== null,
    queryFn: async () =>
      (await call<unknown>(`/api/v1/books/${encodeURIComponent(selected ?? '')}/history?limit=${PAGE}`)).data,
  })
  const books = page.data?.books ?? []
  return (
    <Panel title="Books">
      <div className="flex flex-col gap-3">
        <ErrorNotice error={page.error} />
        <Table
          columns={['Book', 'Name', 'Owning system', 'Positions', 'Version', 'Recorded']}
          selected={books.findIndex((b) => b.book_id === selected)}
          onSelect={(index) => setSelected(books[index]?.book_id ?? null)}
          rows={books.map((b) => [b.book_id, b.name, b.owning_system, b.position_count, b.version, b.recorded_at])}
        />
        <Pager
          onFirst={() => setAfter(null)}
          onNext={() => setAfter(page.data?.next_after ?? null)}
          hasNext={Boolean(page.data?.next_after)}
        />
        <ErrorNotice error={book.error ?? history.error} />
        {selected && (
          <div className="grid gap-4 lg:grid-cols-2">
            <div className="flex flex-col gap-1">
              <h3 className="text-xs font-semibold uppercase text-slate-500">Current version</h3>
              {book.data !== undefined && <JsonBlock value={book.data} />}
            </div>
            <div className="flex flex-col gap-1">
              <h3 className="text-xs font-semibold uppercase text-slate-500">Ingest history</h3>
              {history.data !== undefined && <JsonBlock value={history.data} />}
            </div>
          </div>
        )}
      </div>
    </Panel>
  )
}
