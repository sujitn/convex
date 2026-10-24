// The engine client's call/callMany contract against a scripted worker: a
// worker failure is an `engine` error envelope per request, never a rejection,
// and a batch is logged with every member.
import test from 'node:test';
import assert from 'node:assert/strict';
import { createEngine } from '../src/engine/client.js';

const OK = (n) => JSON.stringify({ ok: 'true', result: { data: { n } } });
const FAIL = JSON.stringify({ ok: 'false', error: { code: 'invalid_input', message: 'bad', details: { field: 'x' } } });

// A fake worker: announces ready, then answers each message with `respond(message)`.
function factory(respond) {
  const workers = [];
  const make = () => {
    const w = {
      onmessage: null,
      onerror: null,
      terminated: false,
      postMessage(msg) {
        setTimeout(() => !w.terminated && w.onmessage({ data: { id: msg.id, ...respond(msg) } }), 0);
      },
      terminate() {
        w.terminated = true;
      },
    };
    setTimeout(() => w.onmessage({ data: { type: 'ready', bytes: 1, ms: 1 } }), 0);
    workers.push(w);
    return w;
  };
  return { make, workers };
}

const req = (n) => ({ schema_version: 1, inputs: {}, operation: { kind: 'curve_query', n } });

test('a batch is logged whole: every request and every response', async () => {
  const { make } = factory((msg) => ({ type: 'result', value: msg.payload.map((p, i) => (i === 1 ? FAIL : OK(i))),
    times: msg.payload.map(() => 1) }));
  const engine = createEngine({ workerFactory: make });
  await engine.ready;
  const requests = [req(0), req(1), req(2)];
  const out = await engine.callMany(requests, { label: 'three' });
  assert.deepEqual(out.map((r) => r.ok), [true, false, true]);
  assert.equal(out[2].data.n, 2);
  const [entry] = engine.getLog();
  assert.equal(entry.count, 3);
  assert.equal(entry.ok, false);
  assert.equal(entry.failures, 1);
  assert.equal(entry.error.code, 'invalid_input');
  assert.deepEqual(entry.requests, requests);
  assert.equal(entry.responses.length, 3);
  assert.equal(entry.responses[1], FAIL);
  assert.equal(entry.request, requests[0]); // the single-entry fields stay for existing readers
  assert.equal(entry.response, entry.responses[0]);
});

test('a trap during a batch resolves every member with an engine error, logs it, and restarts', async () => {
  const { make, workers } = factory(() => ({ type: 'fatal', message: 'unreachable executed' }));
  const engine = createEngine({ workerFactory: make });
  await engine.ready;
  const out = await engine.callMany([req(0), req(1)], { label: 'trapped' });
  assert.equal(out.length, 2);
  for (const r of out) {
    assert.equal(r.ok, false);
    assert.equal(r.error.code, 'engine');
    assert.match(r.error.message, /Engine stopped: unreachable executed/);
  }
  const [entry] = engine.getLog();
  assert.equal(entry.label, 'trapped');
  assert.equal(entry.ok, false);
  assert.equal(entry.failures, 2);
  assert.deepEqual(entry.responses, [null, null]);
  assert.equal(workers.length, 2, 'a replacement worker was started');
  assert.equal(workers[0].terminated, true);
  // `call` has the same contract.
  const one = await engine.call(req(9));
  assert.equal(one.ok, false);
  assert.equal(one.error.code, 'engine');
});

test('a worker whose engine failed to load answers with errors, which become envelopes', async () => {
  const { make } = factory(() => ({ type: 'result', error: 'Engine download failed: HTTP 503' }));
  const engine = createEngine({ workerFactory: make });
  await engine.ready;
  const out = await engine.callMany([req(0), req(1), req(2)]);
  assert.deepEqual(out.map((r) => r.error.code), ['engine', 'engine', 'engine']);
  assert.match(out[0].error.message, /HTTP 503/);
  assert.equal(engine.getLog()[0].count, 3);
});

test('a replacement worker that dies before loading fails its requests instead of leaving them pending', async () => {
  const workers = [];
  const make = () => {
    const first = workers.length === 0;
    const w = {
      onmessage: null,
      onerror: null,
      terminated: false,
      // The first worker traps on any request; the replacement never answers.
      postMessage(msg) {
        if (first) setTimeout(() => !w.terminated && w.onmessage({ data: { id: msg.id, type: 'fatal', message: 'unreachable' } }), 0);
      },
      terminate() {
        w.terminated = true;
      },
    };
    if (first) setTimeout(() => w.onmessage({ data: { type: 'ready', bytes: 1, ms: 1 } }), 0);
    else setTimeout(() => w.onerror({ message: 'worker script failed to load' }), 5);
    workers.push(w);
    return w;
  };
  const engine = createEngine({ workerFactory: make });
  await engine.ready;
  assert.match((await engine.call(req(0))).error.message, /Engine stopped/);
  // Sent while the replacement is still loading.
  const out = await engine.callMany([req(1), req(2)]);
  assert.deepEqual(out.map((r) => r.error.code), ['engine', 'engine']);
  assert.match(out[0].error.message, /worker script failed to load/);
  assert.equal(engine.getStatus().state, 'failed');
  // Once failed, a request is answered at once.
  const later = await engine.call(req(3));
  assert.equal(later.error.code, 'engine');
  assert.equal(workers.length, 2);
});
