import { ApiFailure } from '@convex/api-client'

/**
 * A failure as the desk words it: the server's code and reason for the row it refused,
 * else the request's, else the error's own message.
 */
export function failureNotice(error: unknown): string {
  if (error instanceof ApiFailure) {
    const [row] = error.rows
    return row ? `${row.code}: ${row.reason}` : error.message
  }
  return error instanceof Error ? error.message : String(error)
}
