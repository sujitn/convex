// Loads the real engine (wasm-pack --target nodejs build) for builder tests.
// Build it with `npm run build:wasm:node`, or point CONVEX_ENGINE_NODE at a
// directory containing convex_wasm.js.
import { createRequire } from 'node:module';
import path from 'node:path';
import fs from 'node:fs';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const dir = process.env.CONVEX_ENGINE_NODE || path.resolve(here, '../../.engine-node');
const entry = path.join(dir, 'convex_wasm.js');
if (!fs.existsSync(entry)) {
  throw new Error(`Engine not built: ${entry} is missing. Run \`npm run build:wasm:node\`.`);
}
const engine = createRequire(import.meta.url)(entry);

/** Run one request; return `result.data` or throw with the native error. */
export function run(req) {
  const env = JSON.parse(engine.library(JSON.stringify(req)));
  if (env.ok !== 'true') {
    const e = new Error(`${env.error.code}: ${env.error.message} ${JSON.stringify(env.error.details)}`);
    e.envelope = env;
    throw e;
  }
  return env.result.data;
}

export function envelope(req) {
  return JSON.parse(engine.library(JSON.stringify(req)));
}

export { engine };
