import { useSession } from '@convex/api-client'
import { useState } from 'react'
import { NavLink, Navigate, Route, Routes } from 'react-router-dom'
import { Calculator } from './pages/Calculator'
import { PricingSheet } from './pages/PricingSheet'
import { RfqBlotter } from './pages/RfqBlotter'

const SCREENS = [
  { path: 'sheet', label: 'Pricing sheet', element: <PricingSheet /> },
  { path: 'rfqs', label: 'RFQ blotter', element: <RfqBlotter /> },
  { path: 'calculator', label: 'Calculator', element: <Calculator /> },
]

export function App() {
  return (
    <div className="min-h-screen">
      <header className="border-b border-slate-800 bg-slate-900">
        <div className="flex flex-wrap items-center justify-between gap-3 px-4 py-2">
          <div className="flex items-center gap-6">
            <span className="text-sm font-semibold tracking-wide text-slate-200">Convex desk</span>
            <nav className="flex gap-1">
              {SCREENS.map((screen) => (
                <NavLink
                  key={screen.path}
                  to={`/${screen.path}`}
                  className={({ isActive }) =>
                    `rounded px-3 py-1 text-xs ${isActive ? 'bg-sky-800 text-white' : 'text-slate-400 hover:bg-slate-800'}`
                  }
                >
                  {screen.label}
                </NavLink>
              ))}
            </nav>
          </div>
          <TokenBar />
        </div>
      </header>
      <main className="px-4 py-4">
        <Routes>
          {SCREENS.map((screen) => (
            <Route key={screen.path} path={`/${screen.path}`} element={screen.element} />
          ))}
          <Route path="*" element={<Navigate to="/sheet" replace />} />
        </Routes>
      </main>
    </div>
  )
}

/** The bearer token for required access, held in page memory only, as the admin app holds it. */
function TokenBar() {
  const { token, setToken } = useSession()
  const [draft, setDraft] = useState('')
  if (token) {
    return (
      <div className="flex items-center gap-2 text-xs text-slate-400">
        <span>Token set</span>
        <button type="button" className="rounded px-2 py-0.5 hover:bg-slate-800" onClick={() => setToken(null)}>
          Clear
        </button>
      </div>
    )
  }
  return (
    <div className="flex items-center gap-2">
      <input
        type="password"
        placeholder="Bearer token"
        value={draft}
        onChange={(event) => setDraft(event.target.value.trim())}
        className="w-56 rounded border border-slate-700 bg-slate-950 px-2 py-0.5 font-mono text-xs text-slate-100"
      />
      <button
        type="button"
        disabled={!draft}
        className="rounded bg-slate-800 px-2 py-0.5 text-xs text-slate-200 disabled:opacity-40"
        onClick={() => {
          setToken(draft)
          setDraft('')
        }}
      >
        Use
      </button>
    </div>
  )
}
