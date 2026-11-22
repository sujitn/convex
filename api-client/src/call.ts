import { ApiFailure, decodeFailure } from './admin'

// Same-origin calls only: the server's policy allows connections to itself alone.

export interface CallOptions {
  method?: 'GET' | 'POST'
  /** A value to encode; use only for bodies whose numbers carry no written scale. */
  body?: unknown
  /** JSON text or a file sent exactly as given. */
  raw?: string | Blob
  headers?: Record<string, string>
}

export interface Reply<T> {
  data: T
  /** The response text, for exact re-reading with `readExact`. */
  text: string
  headers: Headers
}

export async function call<T>(token: string | null, path: string, options: CallOptions = {}): Promise<Reply<T>> {
  const headers: Record<string, string> = { Accept: 'application/json', ...options.headers }
  if (token) headers.Authorization = `Bearer ${token}`
  let body: BodyInit | undefined
  if (options.raw !== undefined) {
    body = options.raw
    headers['Content-Type'] = 'application/json'
  } else if (options.body !== undefined) {
    body = JSON.stringify(options.body)
    headers['Content-Type'] = 'application/json'
  }
  const response = await fetch(path, {
    method: options.method ?? 'GET',
    headers,
    body,
    cache: 'no-store',
    credentials: 'omit',
  })
  const text = await response.text()
  let parsed: unknown = null
  try {
    parsed = text ? JSON.parse(text) : null
  } catch {
    parsed = null
  }
  if (!response.ok) throw decodeFailure(response.status, parsed, text)
  return { data: parsed as T, text, headers: response.headers }
}

export { ApiFailure }
