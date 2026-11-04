// Hosts the Convex WASM engine off the main thread. The module is fetched with
// byte progress, instantiated once, and then serves `library()` requests in
// arrival order. A trap (wasm32 panics abort) is reported as `fatal` so the
// client can discard this worker and start a fresh instance.

import init, { library, library_schema } from '../../pkg/convex_wasm.js';

const wasmUrl = new URL('../../pkg/convex_wasm_bg.wasm', import.meta.url);

async function fetchWithProgress(url) {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`Engine download failed: HTTP ${res.status}`);
  const encoded = res.headers.get('content-encoding');
  const length = Number(res.headers.get('content-length')) || 0;
  // A compressed transfer reports the compressed length; progress is then bytes only.
  const total = encoded && encoded !== 'identity' ? 0 : length;
  if (!res.body) return new Uint8Array(await res.arrayBuffer());
  const reader = res.body.getReader();
  const chunks = [];
  let received = 0;
  let last = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    chunks.push(value);
    received += value.length;
    if (received - last > 262144) {
      last = received;
      postMessage({ type: 'progress', received, total });
    }
  }
  const bytes = new Uint8Array(received);
  let offset = 0;
  for (const c of chunks) {
    bytes.set(c, offset);
    offset += c.length;
  }
  postMessage({ type: 'progress', received, total: total || received });
  return bytes;
}

const ready = (async () => {
  const t0 = performance.now();
  const bytes = await fetchWithProgress(wasmUrl);
  await init({ module_or_path: bytes });
  return { bytes: bytes.length, ms: performance.now() - t0 };
})();

ready.then(
  (info) => postMessage({ type: 'ready', ...info }),
  (error) => postMessage({ type: 'failed', message: String(error && error.message ? error.message : error) }),
);

function execute(method, payload) {
  switch (method) {
    case 'library':
      return library(payload);
    case 'schema':
      return library_schema();
    default:
      throw new Error(`Unknown engine method ${method}`);
  }
}

self.onmessage = async ({ data }) => {
  const { id, method, payload, batch } = data;
  try {
    await ready;
  } catch (error) {
    postMessage({ type: 'result', id, error: String(error.message || error) });
    return;
  }
  try {
    if (batch) {
      const out = [];
      const times = [];
      for (const p of payload) {
        const t = performance.now();
        out.push(execute(method, p));
        times.push(performance.now() - t);
      }
      postMessage({ type: 'result', id, value: out, times });
    } else {
      const t = performance.now();
      const value = execute(method, payload);
      postMessage({ type: 'result', id, value, times: [performance.now() - t] });
    }
  } catch (error) {
    // Anything thrown out of wasm here is a trap; the instance is unusable.
    postMessage({ type: 'fatal', id, message: String(error && error.message ? error.message : error) });
  }
};
