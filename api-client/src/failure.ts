/** One importer finding: where a refused file went wrong, never its values. */
export interface Diagnostic {
  section: string
  row: number | null
  field: string
  code: string
  line: number | null
  column: number | null
}

/** One event of a quote batch the server refused, by its position in the batch. */
export interface RowRefusal {
  index: number
  code: string
  reason: string
}

/** A refused request, decoded from any of the server's error bodies. */
export class ApiFailure extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    readonly reason: string,
    readonly diagnostics: Diagnostic[] = [],
    readonly rows: RowRefusal[] = [],
  ) {
    super(`${status} ${code}: ${reason}`)
  }
}

type Json = Record<string, unknown>

function isObject(value: unknown): value is Json {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

/**
 * The server answers refusals in three forms: configuration routes send
 * `{error: {code, reason}}`, reference and access refusals send `{code, error}`
 * (reference ones may add `diagnostics`, the quote routes `rows`), and framework
 * rejections send text.
 */
export function decodeFailure(status: number, body: unknown, text: string): ApiFailure {
  if (isObject(body)) {
    const error = body.error
    if (isObject(error)) {
      return new ApiFailure(status, String(error.code ?? status), String(error.reason ?? ''))
    }
    const diagnostics = Array.isArray(body.diagnostics) ? (body.diagnostics as Diagnostic[]) : []
    const rows = Array.isArray(body.rows) ? (body.rows as RowRefusal[]) : []
    return new ApiFailure(
      status,
      String(body.code ?? status),
      String(error ?? body.message ?? ''),
      diagnostics,
      rows,
    )
  }
  return new ApiFailure(status, String(status), text.trim().slice(0, 500))
}
