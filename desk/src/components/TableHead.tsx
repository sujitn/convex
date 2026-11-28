/** A table column: its heading, and whether it holds numbers, which are right-aligned. */
export interface Column {
  heading: string
  isNumeric?: boolean
}

/** A table's heading row. */
export function TableHead({ columns }: { columns: readonly Column[] }) {
  return (
    <thead className="sticky top-0 bg-slate-900 text-slate-400">
      <tr>
        {columns.map(({ heading, isNumeric }) => (
          <th key={heading} className={`px-2 py-1 font-medium ${isNumeric ? 'text-right' : 'text-left'}`}>
            {heading}
          </th>
        ))}
      </tr>
    </thead>
  )
}
