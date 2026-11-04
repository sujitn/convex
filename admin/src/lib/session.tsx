import { useQueryClient } from '@tanstack/react-query'
import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from 'react'
import { call, type CallOptions, type Reply } from './api'

// The pasted bearer token lives in this component's state only: never in storage,
// the URL or a cookie, so closing or reloading the tab forgets it (D38).

interface Session {
  token: string | null
  setToken: (token: string | null) => void
  call: <T>(path: string, options?: CallOptions) => Promise<Reply<T>>
}

const SessionContext = createContext<Session | null>(null)

export function SessionProvider({ children }: { children: ReactNode }) {
  const [token, setTokenState] = useState<string | null>(null)
  const client = useQueryClient()
  const setToken = useCallback(
    (next: string | null) => {
      setTokenState(next)
      client.removeQueries()
    },
    [client],
  )
  const value = useMemo<Session>(
    () => ({ token, setToken, call: (path, options) => call(token, path, options) }),
    [token, setToken],
  )
  return <SessionContext.Provider value={value}>{children}</SessionContext.Provider>
}

export function useSession(): Session {
  const session = useContext(SessionContext)
  if (!session) throw new Error('useSession outside SessionProvider')
  return session
}
