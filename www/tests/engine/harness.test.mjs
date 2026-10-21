// The test harness itself: where it finds the engine and how it reports errors.
import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { describeError } from './harness.mjs';

const www = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');

test('a relative CONVEX_ENGINE_NODE is taken from the current directory', () => {
  const harness = new URL('./harness.mjs', import.meta.url).href;
  const res = spawnSync(process.execPath, ['--input-type=module', '-e',
    `const m = await import(${JSON.stringify(harness)}); console.log(typeof m.engine.library);`], {
    cwd: path.join(www, 'tests'),
    env: { ...process.env, CONVEX_ENGINE_NODE: '../.engine-node' },
    encoding: 'utf8',
  });
  assert.equal(res.status, 0, res.stderr);
  assert.equal(res.stdout.trim(), 'function');
});

test('native errors read "code: message (field …)" and never print undefined', () => {
  assert.equal(describeError({ code: 'invalid_input', message: 'bad', details: { field: 'cusip' } }),
    'invalid_input: bad (field cusip)');
  assert.equal(describeError({ code: 'invalid_input', message: 'bad', field: 'pillar' }), 'invalid_input: bad (field pillar)');
  assert.equal(describeError({ code: 'unsupported', message: 'no' }), 'unsupported: no');
  assert.equal(describeError({ code: 'x', message: 'y', details: { native_kind: 'K' } }), 'x: y {"native_kind":"K"}');
});
