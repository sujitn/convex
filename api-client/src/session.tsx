import { useQueryClient } from '@tanstack/react-query'
import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from 'react'
import { call, callQueue, type CallOptions, type Reply } from './call'

// The pasted bearer token lives in this component's state only: never in storage,
// the URL or a cookie, so closing or reloading the tab forgets it. A token's calls share
// one queue, so a page's burst of reads stays inside the server's concurrent requests for
// one subject.

interface Session {
  token: string | null
  setToken: (token: string | null) => void
  call: <T>(path: string, options?: CallOptions) => Promise<Reply<T>>
}

const SessionContext = createContext<Session | null>(null)

export function SessionProvider({ children }: { children: ReactNode }) {
  const [token, setTokenState] = useState<string | null>(null)
  // One queue a token: a call still waiting when the token is changed or cleared is
  // refused, not sent under the old token or in the new token's turn.
  const [queue, setQueue] = useState(callQueue)
  const client = useQueryClient()
  const setToken = useCallback(
    (next: string | null) => {
      queue.close()
      setQueue(callQueue())
      setTokenState(next)
      client.removeQueries()
    },
    [client, queue],
  )
  const value = useMemo<Session>(
    () => ({ token, setToken, call: (path, options) => queue.run(() => call(token, path, options)) }),
    [token, setToken, queue],
  )
  return <SessionContext.Provider value={value}>{children}</SessionContext.Provider>
}

export function useSession(): Session {
  const session = useContext(SessionContext)
  if (!session) throw new Error('useSession outside SessionProvider')
  return session
}
