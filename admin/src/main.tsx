import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { BrowserRouter } from 'react-router-dom'
import { App } from './App'
import { SessionProvider } from './lib/session'
import './index.css'

// Refusals are answers, not transient faults: a 401 or 403 must not be retried.
const client = new QueryClient({
  defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false }, mutations: { retry: false } },
})

const root = document.getElementById('root')
if (!root) throw new Error('missing #root')

createRoot(root).render(
  <StrictMode>
    <QueryClientProvider client={client}>
      <SessionProvider>
        <BrowserRouter basename="/admin">
          <App />
        </BrowserRouter>
      </SessionProvider>
    </QueryClientProvider>
  </StrictMode>,
)
