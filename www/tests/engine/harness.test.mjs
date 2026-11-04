// The test harness itself: where it finds the engine and how it reports errors.
import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import fs from 'node:fs';
import os from 'node:os';
import { describeError } from './harness.mjs';

const www = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');

const harness = new URL('./harness.mjs', import.meta.url).href;
const load = (cwd, dir) => spawnSync(process.execPath, ['--input-type=module', '-e',
  `const m = await import(${JSON.stringify(harness)}); console.log(m.engine.library.name);`], {
  cwd,
  env: { ...process.env, CONVEX_ENGINE_NODE: dir },
  encoding: 'utf8',
});

test('CONVEX_ENGINE_NODE is honoured, relative to the current directory', () => {
  // A stand-in engine in a temporary directory: only the override can load it.
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'convex-engine-'));
  try {
    fs.mkdirSync(path.join(root, 'alt'));
    fs.writeFileSync(path.join(root, 'alt', 'convex_wasm.js'), 'exports.library = function standInEngine() {};');
    const res = load(root, './alt');
    assert.equal(res.status, 0, res.stderr);
    assert.equal(res.stdout.trim(), 'standInEngine');
    const missing = load(root, './nowhere');
    assert.notEqual(missing.status, 0);
    assert.ok(missing.stderr.includes(path.join(root, 'nowhere', 'convex_wasm.js')), missing.stderr);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('the default engine directory loads without the override', () => {
  const res = load(path.join(www, 'tests'), '');
  assert.equal(res.status, 0, res.stderr);
  assert.equal(res.stdout.trim(), 'library');
});

test('native errors read "code: message (field …)" and never print undefined', () => {
  assert.equal(describeError({ code: 'invalid_input', message: 'bad', details: { field: 'cusip' } }),
    'invalid_input: bad (field cusip)');
  assert.equal(describeError({ code: 'invalid_input', message: 'bad', field: 'pillar' }), 'invalid_input: bad (field pillar)');
  assert.equal(describeError({ code: 'unsupported', message: 'no' }), 'unsupported: no');
  assert.equal(describeError({ code: 'x', message: 'y', details: { native_kind: 'K' } }), 'x: y {"native_kind":"K"}');
});
