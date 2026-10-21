// Promise API over the engine worker, plus the request log the inspector shows.
// Every library call made by the app goes through `call`/`callMany`.

const LOG_LIMIT = 80;

function operationLabel(request) {
  const op = request && request.operation;
  if (!op) return 'request';
  const action = op.action && op.action.kind;
  return action ? `${op.kind}.${action}` : op.kind;
}

function parseEnvelope(text) {
  const env = JSON.parse(text);
  if (env.ok === 'true') return { ok: true, data: env.result.data, calibration: env.result.market_calibration };
  return { ok: false, error: env.error };
}

export function createEngine({ workerFactory } = {}) {
  const makeWorker =
    workerFactory || (() => new Worker(new URL('./worker.js', import.meta.url), { type: 'module' }));
  let worker = null;
  let seq = 0;
  let restarts = 0;
  let logSeq = 0;
  const pending = new Map();
  const statusListeners = new Set();
  const logListeners = new Set();
  let status = { state: 'loading', received: 0, total: 0 };
  let log = [];
  let resolveReady;
  let rejectReady;
  const ready = new Promise((res, rej) => {
    resolveReady = res;
    rejectReady = rej;
  });

  const setStatus = (next) => {
    status = { ...status, ...next };
    statusListeners.forEach((fn) => fn(status));
  };

  const pushLog = (entry) => {
    log = [entry, ...log].slice(0, LOG_LIMIT);
    logListeners.forEach((fn) => fn(log));
  };

  function spawn() {
    worker = makeWorker();
    worker.onmessage = ({ data }) => {
      if (data.type === 'progress') setStatus({ received: data.received, total: data.total });
      else if (data.type === 'ready') {
        setStatus({ state: 'ready', bytes: data.bytes, loadMs: data.ms, restarts });
        resolveReady(status);
      } else if (data.type === 'failed') {
        setStatus({ state: 'failed', message: data.message });
        rejectReady(new Error(data.message));
      } else if (data.type === 'result' || data.type === 'fatal') {
        const p = pending.get(data.id);
        if (!p) return;
        pending.delete(data.id);
        if (data.type === 'fatal') {
          p.reject(new Error(`Engine stopped: ${data.message}`));
          restart(data.message);
        } else if (data.error) p.reject(new Error(data.error));
        else p.resolve(data);
      }
    };
    worker.onerror = (e) => {
      const message = e.message || 'Engine worker error';
      if (status.state === 'loading') {
        setStatus({ state: 'failed', message });
        rejectReady(new Error(message));
      }
    };
  }

  function restart(reason) {
    restarts += 1;
    worker.terminate();
    for (const [, p] of pending) p.reject(new Error('Engine restarted'));
    pending.clear();
    setStatus({ state: 'loading', received: 0, total: 0, lastTrap: reason, restarts });
    // The replacement worker queues requests until its own instance is ready.
    spawn();
  }

  function post(method, payload, batch) {
    return new Promise((resolve, reject) => {
      const id = ++seq;
      pending.set(id, { resolve, reject });
      worker.postMessage({ id, method, payload, batch });
    });
  }

  spawn();

  async function call(request, { label } = {}) {
    await ready;
    const text = JSON.stringify(request);
    const started = Date.now();
    let out;
    try {
      const res = await post('library', text, false);
      out = { ...parseEnvelope(res.value), ms: res.times[0] };
      pushLog({ id: ++logSeq, at: started, label: label || operationLabel(request), op: operationLabel(request),
        ms: out.ms, count: 1, ok: out.ok, error: out.error, request, response: res.value });
    } catch (error) {
      out = { ok: false, error: { code: 'engine', message: error.message }, ms: 0 };
      pushLog({ id: ++logSeq, at: started, label: label || operationLabel(request), op: operationLabel(request),
        ms: 0, count: 1, ok: false, error: out.error, request, response: null });
    }
    return out;
  }

  // Same contract as `call`: a worker failure (trap, restart, failed reload)
  // resolves every member with an `engine` error envelope instead of rejecting.
  async function callMany(requests, { label } = {}) {
    if (!requests.length) return [];
    await ready;
    const started = Date.now();
    let results;
    let responses;
    try {
      const res = await post('library', requests.map((r) => JSON.stringify(r)), true);
      results = res.value.map((text, i) => ({ ...parseEnvelope(text), ms: res.times[i] }));
      responses = res.value;
    } catch (error) {
      results = requests.map(() => ({ ok: false, error: { code: 'engine', message: error.message }, ms: 0 }));
      responses = requests.map(() => null);
    }
    const failed = results.filter((r) => !r.ok);
    pushLog({ id: ++logSeq, at: started, label: label || `${operationLabel(requests[0])} ×${requests.length}`,
      op: operationLabel(requests[0]), ms: results.reduce((a, r) => a + r.ms, 0), count: requests.length,
      ok: failed.length === 0, error: failed[0] && failed[0].error, failures: failed.length,
      request: requests[0], response: responses[0], requests, responses });
    return results;
  }

  async function schema() {
    await ready;
    const res = await post('schema', null, false);
    return res.value;
  }

  return {
    ready,
    call,
    callMany,
    schema,
    getStatus: () => status,
    getLog: () => log,
    onStatus: (fn) => (statusListeners.add(fn), () => statusListeners.delete(fn)),
    onLog: (fn) => (logListeners.add(fn), () => logListeners.delete(fn)),
    clearLog: () => {
      log = [];
      logListeners.forEach((fn) => fn(log));
    },
  };
}
