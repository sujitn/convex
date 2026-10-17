#!/usr/bin/env python3
"""Automatic six-trade core analytics from a local synthetic replay stream.

Runs the existing server/native library, verifies stale-data gating, stream
recovery and interrupted-work/server restart, then stops all owned processes.
"""
import argparse
import contextlib
import copy
import hashlib
import http.server
import json
from pathlib import Path
import queue
import subprocess
import sys
import tempfile
import threading
import urllib.request

import core_snapshot_workflow as core
import core_streaming_workflow as transport
import streaming_analytics as analytics

ROOT, require = core.ROOT, core.require


def configuration(server, capture, plan, factors):
    """Retain the existing full synthetic policy; replace only vector bindings."""
    book = core.supplied.snapshot()
    book['operation']['input']['before'] = copy.deepcopy(book['operation']['input']['steps'][0]['after'])
    requests, _ = core.supplied.risk_requests(book)
    for request in requests:
        request['operation']['input']['factors'] = copy.deepcopy(factors)
    # Setup calls supply the exact existing paper policy's native credit/FX
    # identities. The worker subsequently computes/persists all current results.
    results = [server.call(r)['result']['data'] for r in requests]
    hedge = core.supplied.hedge_request(book, results, factors)
    problem = hedge['operation']['problem']
    for row in [*problem['holdings'], *problem['candidates']]:
        row['fx'] = row['risk']['fx']
        row['risk'] = row['risk']['id']
    selected = copy.deepcopy(plan)
    selected['definition']['version'] = 2
    bindings = []
    for key, pins in selected['curve_sets'].items():
        for pin in pins:
            record = next(r for r in capture['curves'] if r['curve_id'] == pin['id'] and r['version'] == pin['version'])
            bindings.append(dict(set=key, curve_id=pin['id'], source=record['source'],
                recipe_key=record['recipe']['key'], follow=key == 'market.1', max_age_ms=5000))
    return dict(schema_version=1, id='core.synthetic.automatic', plan=selected, bindings=bindings,
                risks={r['vector']['id']: 'risk.' + str(n) for n, r in enumerate(results)}, pnl='pnl', hedge=hedge)


class WorkerProcess:
    def __init__(self, feed, url, folder, replay_at):
        self.events = queue.Queue(maxsize=128)
        self.command = [sys.executable, str(ROOT / 'tools/streaming_analytics.py'), '--feed-url', feed.url,
            '--url', url, '--stream-id', 'core.synthetic', '--subscription', str(folder / 'subscription.json'),
            '--workflow', str(folder / 'workflow.json'), '--state', str(folder / 'workflow-state.json'),
            '--replay-at', replay_at, '--timeout', '3', '--retry-delay', '0.02']
        self.history = []

    def __enter__(self):
        self.process = subprocess.Popen(self.command, cwd=ROOT, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        def read():
            for line in self.process.stdout:
                self.events.put(json.loads(line), timeout=5)
            self.events.put(None, timeout=5)
        self.reader = threading.Thread(target=read, daemon=True)
        self.reader.start()
        return self

    def wait(self, state, reason=None):
        for _ in range(128):
            try:
                event = self.events.get(timeout=30)
            except queue.Empty:
                raise RuntimeError('Automatic workflow state timeout: ' + state) from None
            require(event is not None, 'Automatic worker stopped before ' + state)
            self.history.append(event)
            if event['state'] == state and (reason is None or event.get('reason') == reason):
                return event
        raise RuntimeError('Automatic workflow event bound exceeded')

    def __exit__(self, *_):
        if self.process.poll() is None:
            self.process.terminate()
        self.process.wait(timeout=10)
        self.reader.join(timeout=3)
        error = self.process.stderr.read()
        self.process.stdout.close()
        self.process.stderr.close()
        require(not self.reader.is_alive() and self.process.returncode in (0, -9),
                'Automatic worker failed: ' + error + json.dumps(self.history[-5:]))


@contextlib.contextmanager
def lost_result_ack(url):
    """Forward one actual committed native result, but withhold its HTTP receipt."""
    blocked, release = threading.Event(), threading.Event()
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    state = dict(dropped=False)
    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def forward(self):
            body = self.rfile.read(int(self.headers['Content-Length'])) if 'Content-Length' in self.headers else None
            headers = {'Content-Type': 'application/json'}
            if 'Idempotency-Key' in self.headers:
                headers['Idempotency-Key'] = self.headers['Idempotency-Key']
            request = urllib.request.Request(url + self.path.removeprefix('/api/v1'), body, headers, method=self.command)
            with opener.open(request, timeout=15) as response:
                raw = response.read()
                profile = response.headers['x-convex-profile']
                status = response.status
            if '/run/' in self.path and not state['dropped']:
                state['dropped'] = True
                blocked.set()
                release.wait(15)
            try:
                self.send_response(status)
                self.send_header('x-convex-profile', profile)
                self.end_headers()
                self.wfile.write(raw)
            except OSError:
                pass
        do_GET = forward
        do_POST = forward
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield f'http://127.0.0.1:{server.server_port}/api/v1', blocked
    finally:
        release.set()
        server.shutdown()
        server.server_close()
        thread.join(timeout=3)
        require(not thread.is_alive(), 'Owned acknowledgement proxy did not stop')


def compare(client, event):
    saved, native = [], {}
    for calc, receipt in event['receipts'].items():
        output = client.result(receipt)
        snapshot = client.snapshot(receipt['snapshot_id'])
        require(client.server.call(core.native_request(snapshot, calc)) == output, 'Automatic native parity failed')
        saved.append((receipt, client.result(receipt, raw=True)))
        native[calc] = output
    pnl = native['pnl']['result']['data']['report']['reporting_totals']
    for row in pnl:
        residual = row['attribution']['residual']
        require(all(v['value'] == 0 for v in [residual['cash'], residual['current_model_pnl'], *residual['marking'].values()]),
                'Automatic causal P&L did not reconcile')
    solved = native['hedge']['result']['data']
    require(solved['status'] == 'feasible' and all(c['satisfied'] for c in solved['proposal']['constraints']), 'Automatic hedge constraint failed')
    return saved, native


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary-dir', type=Path)
    args = parser.parse_args()
    directory = args.binary_dir
    if directory is None:
        subprocess.run(['cargo', 'build', '--locked', '-p', 'convex-server', '--bins'], cwd=ROOT, check=True)
        metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--locked', '--no-deps', '--format-version', '1'], cwd=ROOT))
        directory = Path(metadata['target_directory']) / 'debug'
    binary = (directory / 'convex-server').resolve(strict=True)
    importer = (directory / 'convex-core-inputs').resolve(strict=True)
    capture, plan, factors = core.prepare()
    records = [transport.item('curve', r) for r in capture['curves']] + [transport.item('fixings', capture['fixings'])]
    with tempfile.TemporaryDirectory(prefix='convex-core-auto-') as name, transport.LocalFeed(records) as feed:
        folder = Path(name)
        (folder / 'subscription.json').write_text(json.dumps(transport.universe(records)))
        source = folder / 'references.json'
        source.write_text(json.dumps(dict(schema_version=1, references=capture['references'], curves=[], fixings=[])))
        subprocess.run([str(importer), str(folder / 'state.redb'), str(source)], cwd=ROOT, check=True, stdout=subprocess.PIPE)
        with core.supplied.ServerProcess(binary, folder) as server:
            client = core.Client(server)
            config = configuration(server, capture, plan, factors)
            (folder / 'workflow.json').write_text(json.dumps(config))
            # Crash while the first native result is durable but unacknowledged.
            with lost_result_ack(client.root) as (proxy, blocked), WorkerProcess(feed, proxy, folder, plan['known_at']) as worker:
                require(blocked.wait(30), 'No actual committed native result reached acknowledgement gate')
                worker.process.kill()
                worker.process.wait(timeout=5)
            with WorkerProcess(feed, client.root, folder, plan['known_at']) as worker:
                initial = worker.wait('ready')
                require(any(r['reused'] for r in initial['receipts'].values()), 'Interrupted durable result was not recovered')
                saved, baseline = compare(client, initial)
                update = copy.deepcopy(next(r for r in capture['curves'] if r['curve_id'] == 'D.OIS' and r['version'] == 2))
                update['version'] = 3
                # Reuse the complete original alternative curve, preserving all
                # node economics under the configured current native key.
                update['recipe']['source'] = copy.deepcopy(next(r for r in capture['curves'] if r['curve_id'] == 'D.OIS' and r['version'] == 1)['recipe']['source'])
                feed.send(transport.item('curve', update))
                revised = worker.wait('ready')
                require(revised['snapshot_id'] != initial['snapshot_id'], 'Confirmed source revision did not trigger a snapshot')
                more, curve_results = compare(client, revised)
                require(core.measure(curve_results['risk.0']) != core.measure(baseline['risk.0']), 'Automatic price did not consume changed curve')
                saved.extend(more)
                correction = copy.deepcopy(next(f for f in capture['fixings'] if f['key']['index_id'] == 'USD.SOFR' and f['key']['observation_date'] == '2026-09-16'))
                correction.update(version=2, rate=dict(value='0.039', unit='decimal_rate'), published_at=plan['known_at'], recorded_at=plan['known_at'])
                feed.send(transport.item('fixings', [correction]))
                fixed = worker.wait('ready')
                more, corrected = compare(client, fixed)
                saved.extend(more)
                by_id = lambda values: {v['result']['data']['vector']['id']: v for k, v in values.items() if k.startswith('risk.')}
                before, after = by_id(baseline), by_id(corrected)
                require(core.measure(before['seasoned-OIS']) != core.measure(after['seasoned-OIS']), 'Automatic OIS correction missing')
                require(core.measure(before['mixed.overnight']) == core.measure(after['mixed.overnight']), 'FirstPublished future restated')
                last_plan = analytics.load(folder / 'workflow-state.json')['plan']
                stale = copy.deepcopy(update)
                stale.update(version=4, max_age_ms=1, source_time='2026-09-16T11:59:00Z')
                feed.send(transport.item('curve', stale))
                worker.wait('unavailable', 'stale_market_data')
                require(analytics.load(folder / 'workflow-state.json')['plan'] == last_plan, 'Stale input advanced accepted selection')
                fresh = copy.deepcopy(update)
                fresh['version'] = 5
                feed.send(transport.item('curve', fresh))
                current = worker.wait('ready')
                more, _ = compare(client, current)
                saved.extend(more)
                # A gap must not publish the skipped fixing or reuse readiness.
                leaked = copy.deepcopy(correction)
                leaked['version'] = 99
                feed.send(transport.item('fixings', [leaked]), gap=True)
                worker.wait('unavailable', 'sequence_gap')
                recovered = worker.wait('ready')
                require(recovered['snapshot_id'] == current['snapshot_id'], 'Gap leaked into current analytics')
                require(all(r['reused'] for r in recovered['receipts'].values()), 'Reimage did not reuse saved calculations')
                feed.disconnect()
                worker.wait('unavailable', 'disconnected')
                require(worker.wait('ready')['snapshot_id'] == current['snapshot_id'], 'Reconnect changed exact inputs')
            source.unlink()
        with core.supplied.ServerProcess(binary, folder) as server:
            client = core.Client(server)
            with WorkerProcess(feed, client.root, folder, plan['known_at']) as worker:
                require(worker.wait('unavailable')['ready'] is False, 'Restart trusted a previous ready receipt')
                recovered = worker.wait('ready')
                require(recovered['snapshot_id'] == current['snapshot_id'], 'Server restart changed current snapshot')
                require(all(r['reused'] for r in recovered['receipts'].values()), 'Server restart lost cached work')
                for receipt, raw in saved:
                    require(client.result(receipt, raw=True) == raw, 'Restart changed saved native result')
                for key in ['risk.0', 'pnl', 'hedge']:
                    receipt = recovered['receipts'][key]
                    raw = client.result(receipt, raw=True)
                    replay = client.run(receipt['snapshot_id'], receipt['calculation'], recompute=True)
                    require(client.result(replay, raw=True) == raw, 'Saved-input recomputation changed after restart')
    print(json.dumps(dict(status='passed', data_quality='synthetic', clock='explicit historical replay', trades=6, factors=5,
        automatically_triggered=['pricing', 'risk', 'hedge_proposal', 'causal_pnl'], persisted_results=len(saved),
        interrupted_commit_recovered=True, native_parity=True, stale_input_blocked=True,
        gap_and_reconnect_recovered=True, latest_known_fixing_consumed=True, first_published_future_preserved=True,
        restart_results_identical=True, restart_cache_recovered=True, saved_recomputation_identical=True,
        snapshots=[initial['snapshot_id'], revised['snapshot_id'], fixed['snapshot_id'], current['snapshot_id']],
        result_digests={receipt['result_id']: hashlib.sha256(raw).hexdigest() for receipt, raw in saved},
        execution_authority='none', processes_stopped=True), indent=2))


if __name__ == '__main__':
    try:
        main()
    except (RuntimeError, OSError, ValueError, subprocess.SubprocessError, KeyError, StopIteration) as error:
        print(str(error), file=sys.stderr)
        raise SystemExit(1)
