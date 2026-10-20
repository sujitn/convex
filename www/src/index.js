import React from 'react';
import { createRoot } from 'react-dom/client';
import App from './App';
import './styles/bloomberg.css';
// WASM initialization fix

// Initialize WASM module
let wasmModule = null;

async function initWasm() {
  try {
    // Dynamic import of the WASM module (web target)
    const wasm = await import('../pkg');
    // With --target web, we need to call the default init function first
    if (wasm.default) {
      await wasm.default();
    }
    wasmModule = wasm;
    console.log('WASM module initialized successfully');
    return wasm;
  } catch (error) {
    console.error('Failed to initialize WASM module:', error);
    throw error;
  }
}

// Export for use in components
export function getWasmModule() {
  return wasmModule;
}

// Initialize and render
async function main() {
  const wasm = await initWasm();
  wasmModule = wasm;

  const container = document.getElementById('root');
  const root = createRoot(container);
  root.render(
    <React.StrictMode>
      <App wasmModule={wasm} />
    </React.StrictMode>
  );
}

main().catch(() => {
  createRoot(document.getElementById('root')).render(
    <div className="error-banner" role="alert">Unable to load the analytics engine.</div>
  );
});
