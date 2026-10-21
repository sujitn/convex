import React from 'react';
import { createRoot } from 'react-dom/client';
import App from './App';
import Boot from './ui/Boot';
import { createEngine } from './engine/client';
import './styles/app.css';

// The analytics engine must load before anything is shown: there is no mock
// or fallback calculator. Until it is ready the boot screen shows download
// progress; if it fails, an error is rendered instead of the application.
export async function main({ engineFactory = createEngine, container = document.getElementById('root') } = {}) {
  const root = createRoot(container);
  let engine;
  try {
    engine = engineFactory();
    root.render(<Boot engine={engine} />);
    await engine.ready;
  } catch (error) {
    root.render(
      <div className="boot-error" role="alert">
        <p>Unable to load the analytics engine.</p>
        <p className="boot-error__detail">{String((error && error.message) || error)}</p>
      </div>,
    );
    return;
  }
  root.render(<App engine={engine} />);
}

if (typeof window !== 'undefined' && !window.__CONVEX_NO_AUTOSTART__) main();
