import { QueryClient } from '@tanstack/react-query'

/**
 * The apps' query client. Refusals are answers, not transient faults: a 401 or 403 must
 * not be retried.
 */
export function createQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false }, mutations: { retry: false } },
  })
}
