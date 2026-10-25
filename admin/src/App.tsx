import { useState } from 'react'
import { NavLink, Navigate, Route, Routes } from 'react-router-dom'
import { Button, inputClass } from './components/ui'
import { BooksPage } from './pages/BooksPage'
import { ConfigurationPage } from './pages/ConfigurationPage'
import { CurvesPage } from './pages/CurvesPage'
import { ImportPage } from './pages/ImportPage'
import { ReferencePage } from './pages/ReferencePage'
import { RuntimePage } from './pages/RuntimePage'
import { useSession } from './lib/session'

const PAGES = [
  { path: 'runtime', label: 'Runtime', element: <RuntimePage /> },
  { path: 'reference', label: 'Reference', element: <ReferencePage /> },
  { path: 'import', label: 'Import', element: <ImportPage /> },
  { path: 'curves', label: 'Curves', element: <CurvesPage /> },
  { path: 'books', label: 'Books', element: <BooksPage /> },
  { path: 'configuration', label: 'Configuration', element: <ConfigurationPage /> },
]

export function App() {
  return (
    <div className="min-h-screen">
      <header className="border-b border-slate-200 bg-white">
        <div className="mx-auto flex max-w-7xl flex-wrap items-center justify-between gap-3 px-4 py-3">
          <div className="flex items-center gap-6">
            <span className="font-semibold">Convex admin</span>
            <nav className="flex gap-1">
              {PAGES.map((page) => (
                <NavLink
                  key={page.path}
                  to={`/${page.path}`}
                  className={({ isActive }) =>
                    `rounded px-3 py-1.5 text-sm ${isActive ? 'bg-slate-800 text-white' : 'text-slate-600 hover:bg-slate-100'}`
                  }
                >
                  {page.label}
                </NavLink>
              ))}
            </nav>
          </div>
          <TokenBar />
        </div>
      </header>
      <main className="mx-auto max-w-7xl px-4 py-6">
        <Routes>
          {PAGES.map((page) => (
            <Route key={page.path} path={`/${page.path}`} element={page.element} />
          ))}
          <Route path="*" element={<Navigate to="/runtime" replace />} />
        </Routes>
      </main>
    </div>
  )
}

/** Required access mode needs a bearer token; it is kept in memory only (D38). */
function TokenBar() {
  const { token, setToken } = useSession()
  const [draft, setDraft] = useState('')
  if (token) {
    return (
      <div className="flex items-center gap-2 text-sm text-slate-600">
        <span>Bearer token held in this tab only</span>
        <Button tone="plain" onClick={() => setToken(null)}>
          Forget
        </Button>
      </div>
    )
  }
  return (
    <div className="flex items-center gap-2">
      <input
        type="password"
        autoComplete="off"
        aria-label="Bearer token"
        placeholder="Bearer token (required access mode)"
        value={draft}
        onChange={(event) => setDraft(event.target.value)}
        className={`${inputClass} w-72`}
      />
      <Button
        tone="plain"
        disabled={!draft.trim()}
        onClick={() => {
          setToken(draft.trim())
          setDraft('')
        }}
      >
        Use token
      </Button>
    </div>
  )
}
