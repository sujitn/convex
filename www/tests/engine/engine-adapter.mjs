// The browser engine client's call/callMany contract, backed by the Node build.
import { engine } from './harness.mjs';

function parse(text) {
  const env = JSON.parse(text);
  return env.ok === 'true' ? { ok: true, data: env.result.data } : { ok: false, error: env.error };
}

export const nodeEngine = {
  async call(req) {
    const t = performance.now();
    const out = parse(engine.library(JSON.stringify(req)));
    return { ...out, ms: performance.now() - t };
  },
  async callMany(reqs) {
    return reqs.map((r) => ({ ...parse(engine.library(JSON.stringify(r))), ms: 0 }));
  },
};
