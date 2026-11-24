import { decodeFailure } from './failure'

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


/**
 * Calls a session keeps in flight at once, half the server's default concurrent requests
 * for one subject, past which it refuses a call (429); the rest wait their turn.
 */
export const MAX_CALLS_IN_FLIGHT = 4

/** Calls run in turn, at most so many at once. */
export interface CallQueue {
  /** Run `task` in its turn. */
  run<T>(task: () => Promise<T>): Promise<T>
  /** Refuse every call still waiting, and each asked from now on; those running finish. */
  close(): void
}

/** A queue that runs at most `most` calls at once, each in the order it was asked. */
export function callQueue(most: number = MAX_CALLS_IN_FLIGHT): CallQueue {
  let running = 0
  let isClosed = false
  const waiting: { start: () => void; refuse: (error: Error) => void }[] = []
  const closed = () => new Error('the token was changed or cleared before the call was sent')
  return {
    async run(task) {
      if (isClosed) throw closed()
      if (running >= most) await new Promise<void>((start, refuse) => waiting.push({ start, refuse }))
      else running += 1
      try {
        return await task()
      } finally {
        // A waiting call takes this one's turn, so the count stays as it is.
        const next = waiting.shift()
        if (next) next.start()
        else running -= 1
      }
    },
    close() {
      isClosed = true
      for (const entry of waiting.splice(0)) entry.refuse(closed())
    },
  }
}
