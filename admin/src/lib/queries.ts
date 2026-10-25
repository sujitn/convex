import { useQuery } from '@tanstack/react-query'
import { useSession } from './session'
import type { HistoryPage, RuntimeHealth } from './types'

const RECENT_ACTIVATIONS = 20

export function useRuntimeHealth() {
  const { call } = useSession()
  return useQuery({
    queryKey: ['runtime'],
    queryFn: async () => (await call<RuntimeHealth>('/api/v1/runtime')).data,
    refetchInterval: 5000,
  })
}

/**
 * The most recent activations, newest first. History pages count from the oldest
 * record, so the first read learns the total and the second reads the tail.
 */
export function useRecentActivations(configurationId: string | null) {
  const { call } = useSession()
  return useQuery({
    queryKey: ['activations', configurationId],
    enabled: configurationId !== null,
    queryFn: async () => {
      const base = `/api/v1/configuration/${encodeURIComponent(configurationId ?? '')}/history`
      const head = (await call<HistoryPage>(`${base}?offset=0&limit=1`)).data
      const offset = Math.max(0, head.total - RECENT_ACTIVATIONS)
      const tail = (await call<HistoryPage>(`${base}?offset=${offset}&limit=${RECENT_ACTIVATIONS}`)).data
      return { total: tail.total, items: [...tail.items].reverse() }
    },
  })
}
