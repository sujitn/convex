// Loads the real engine (wasm-pack --target nodejs build) for builder tests.
// Build it with `npm run build:wasm:node`, or point CONVEX_ENGINE_NODE at a
// directory containing convex_wasm.js (relative paths are from the current
// directory).
import { createRequire } from 'node:module';
import path from 'node:path';
import fs from 'node:fs';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const configured = process.env.CONVEX_ENGINE_NODE;
const dir = configured ? path.resolve(process.cwd(), configured) : path.resolve(here, '../../.engine-node');
const entry = path.join(dir, 'convex_wasm.js');
if (!fs.existsSync(entry)) {
  throw new Error(`Engine not built: ${entry} is missing. Run \`npm run build:wasm:node\`.`);
}
const engine = createRequire(import.meta.url)(entry);

/** "code: message (field x)" for a native error, without printing absent parts. */
export function describeError(error) {
  const { field: detailField, ...rest } = error.details || {};
  const field = error.field ?? detailField;
  const extra = Object.keys(rest).length ? ` ${JSON.stringify(rest)}` : '';
  return `${error.code}: ${error.message}${field ? ` (field ${field})` : ''}${extra}`;
}

/** Run one request; return `result.data` or throw with the native error. */
export function run(req) {
  const env = JSON.parse(engine.library(JSON.stringify(req)));
  if (env.ok !== 'true') {
    const e = new Error(describeError(env.error));
    e.envelope = env;
    throw e;
  }
  return env.result.data;
}

export function envelope(req) {
  return JSON.parse(engine.library(JSON.stringify(req)));
}

export { engine };
