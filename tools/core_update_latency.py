#!/usr/bin/env python3
"""Measure synthetic 36/60-trade stream updates through existing core ports.

The timer ends at durable worker readiness. Native comparisons and recovery
checks run outside update samples. This is a local sequential measurement,
not a production latency target. No financial calculation is implemented here.
"""
import argparse
import contextlib
import copy
import hashlib
import http.server
import json
from pathlib import Path
import statistics
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.request

import core_capacity_workflow as capacity

core, transport, books, automatic = capacity.core, capacity.transport, capacity.books, capacity.automatic
ROOT, require = core.ROOT, core.require


def summary(values):
    return dict(samples_ms=values, median_ms=statistics.median(values), min_ms=min(values), max_ms=max(values))


def category(method, path):
    if '/run/' in path:
        name = path.rsplit('/', 1)[1]
        return 'risk_run' if name.startswith('risk.') else name + '_run'
    if '/analytics/results/' in path:
        return 'result_read'
    if '/analytics/snapshots/' in path:
        return 'snapshot_read'
    if path.endswith('/analytics/snapshots'):
        return 'assemble'
    return method.lower() + ':' + path.split('/api/v1', 1)[-1]


@contextlib.contextmanager
def observed_http(url):
    """Identical local measurement proxy for both candidates; no service changes."""
    rows = []
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def forward(self):
            body = self.rfile.read(int(self.headers.get('Content-Length', '0')))
            headers = {'Content-Type': 'application/json'}
            if 'Idempotency-Key' in self.headers:
                headers['Idempotency-Key'] = self.headers['Idempotency-Key']
            request = urllib.request.Request(url + self.path.removeprefix('/api/v1'),
                body if self.command == 'POST' else None, headers, method=self.command)
            start = time.perf_counter_ns()
            try:
                response = opener.open(request, timeout=120)
            except urllib.error.HTTPError as error:
                response = error
            with response:
                raw = response.read(32 * 1024 * 1024 + 1)
                status, profile = response.status, response.headers.get('x-convex-profile')
            elapsed = (time.perf_counter_ns() - start) / 1e6
            require(len(raw) <= 32 * 1024 * 1024, 'Measured response exceeded existing transport bound')
            rows.append(dict(method=self.command, path=self.path, category=category(self.command, self.path),
                elapsed_ms=elapsed, request_bytes=len(body), response_bytes=len(raw), status=status))
            self.send_response(status)
            if profile:
                self.send_header('x-convex-profile', profile)
            self.send_header('Content-Length', str(len(raw)))
            self.end_headers()
            self.wfile.write(raw)

        do_POST = forward
        do_GET = forward

    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield f'http://127.0.0.1:{server.server_port}/api/v1', rows
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=3)
        require(not thread.is_alive(), 'Measurement proxy did not stop')


def measured_worker(feed, client, folder, cutoff):
    process = capacity.worker(feed, client, folder, cutoff)
    process.command[process.command.index('--timeout') + 1] = '120'
    return process


def measure(directory, lots, receipts, samples):
    binary, importer = [(directory / n).resolve(strict=True) for n in ('convex-server', 'convex-core-inputs')]
    native = (directory / 'examples/book_capacity_native').resolve(strict=True)
    fixture = json.loads(capacity.benchmark.FIXTURE.read_bytes())
    value, plan, risks = capacity.workload(lots, receipts)
    records = [transport.item('curve', r) for r in fixture['curves']] + [transport.item('fixings', fixture['fixings'])]
    retained, sample_rows, probes = [], [], {}
    with tempfile.TemporaryDirectory(prefix='convex-core-update-') as name, books.BookFeed(records) as feed:
        folder = Path(name)
        command = dict(idempotency_key='capacity.initial', changes=[], master_changes=[dict(expected_version=None,
            definition=dict(kind='calculation', definition=value))])
        capture = folder / 'capture.json'
        capture.write_text(json.dumps(dict(schema_version=1,
            references=[dict(recorded_at=plan['known_at'], command=command)], curves=fixture['curves'], fixings=fixture['fixings'])))
        subprocess.run([str(importer), str(folder / 'state.redb'), str(capture)], cwd=ROOT, check=True, stdout=subprocess.PIPE)
        capture.unlink()
        with core.supplied.ServerProcess(binary, folder) as server:
            config = capacity.configuration(server, value, plan, risks)
        current = dict(id='core.capacity.book', version=1, reference_commit=command['idempotency_key'], workflow=config, position_step=0)
        feed.retain_book(transport.item('book', current))
        selection = transport.universe(records)
        selection['book'] = current['id']
        (folder / 'subscription.json').write_text(json.dumps(selection))
        (folder / 'workflow.json').write_text(json.dumps(config))
        with core.supplied.ServerProcess(binary, folder) as server:
            client = core.Client(server)
            with observed_http(client.root) as (url, calls):
                observed = copy.copy(client)
                observed.root = url
                with measured_worker(feed, observed, folder, plan['known_at']) as process:
                    initial = process.wait('ready')
                    rows, first = capacity.compare(client, initial, config, native)
                    retained.extend(rows)
                    previous = initial
                    for index in range(samples + 1):
                        update = copy.deepcopy(next(r for r in fixture['curves'] if r['curve_id'] == 'D.OIS' and r['version'] == 4))
                        update['version'] = 5 + index
                        source_version = 1 if index % 2 == 0 else 4
                        update['recipe']['source'] = copy.deepcopy(next(r for r in fixture['curves']
                            if r['curve_id'] == 'D.OIS' and r['version'] == source_version)['recipe']['source'])
                        begin = len(calls)
                        start = time.perf_counter_ns()
                        feed.send(transport.item('curve', update))
                        ready = process.wait('ready')
                        elapsed = (time.perf_counter_ns() - start) / 1e6
                        trace = copy.deepcopy(calls[begin:])
                        require(ready['snapshot_id'] != previous['snapshot_id'], 'Update reused previous captured inputs')
                        require(all(not r['reused'] for r in ready['receipts'].values()), 'Fresh update reused old results')
                        rows, revised = capacity.compare(client, ready, config, native)
                        retained.extend(rows)
                        require(first['risk.1']['vector']['sensitivities'][0]['base_pv'] != revised['risk.1']['vector']['sensitivities'][0]['base_pv'],
                            'Curve change did not reprice the live IRS')
                        sample_rows.append(dict(warmup=index == 0, elapsed_ms=elapsed, snapshot_id=ready['snapshot_id'],
                            calls=trace, result_digests={r['calculation']: hashlib.sha256(raw).hexdigest() for r, raw in rows}))
                        print(json.dumps(dict(trades=6 * lots, warmup=index == 0, update_ms=elapsed)), file=sys.stderr, flush=True)
                        first, previous = revised, ready
                    # The same native work is forced or recovered from durable results;
                    # these endpoint probes are deliberately outside update samples.
                    for label, call in [('snapshot_read', lambda: client.snapshot(ready['snapshot_id'])),
                            ('cached_risk', lambda: client.run(ready['snapshot_id'], 'risk.1')),
                            ('forced_risk', lambda: client.run(ready['snapshot_id'], 'risk.1', recompute=True))]:
                        times = []
                        for _ in range(samples):
                            start = time.perf_counter_ns()
                            result = call()
                            times.append((time.perf_counter_ns() - start) / 1e6)
                            if label != 'snapshot_read':
                                require(result['reused'] == (label == 'cached_risk'), 'Probe did not run the stated path')
                                expected = dict((r['calculation'], raw) for r, raw in rows)['risk.1']
                                require(client.result(result, raw=True) == expected, 'Probe changed native output')
                        probes[label] = summary(times)
            last_snapshot = client.snapshot(ready['snapshot_id'])
        # Reopen both processes. No selected-input/native-output memo can survive.
        with core.supplied.ServerProcess(binary, folder) as server:
            client = core.Client(server)
            start = time.perf_counter_ns()
            with measured_worker(feed, client, folder, plan['known_at']) as process:
                recovered = process.wait('ready')
                restart_ms = (time.perf_counter_ns() - start) / 1e6
                require(recovered['snapshot_id'] == ready['snapshot_id'], 'Restart changed selected inputs')
                require(all(r['reused'] for r in recovered['receipts'].values()), 'Restart did not recover durable results')
                for receipt, raw in retained:
                    require(client.result(receipt, raw=True) == raw, 'Restart changed retained output bytes')
                for receipt in recovered['receipts'].values():
                    before = client.result(receipt, raw=True)
                    replay = client.run(receipt['snapshot_id'], receipt['calculation'], recompute=True)
                    require(not replay['reused'] and client.result(replay, raw=True) == before, 'Restart recomputation changed saved-input output')
    measured = [s for s in sample_rows if not s['warmup']]
    require(all(200 <= c['status'] < 300 for s in sample_rows for c in s['calls']), 'Rejected work cannot count as update latency')
    return dict(status='passed', trades=6 * lots, receipts_per_bond=receipts,
        update=summary([s['elapsed_ms'] for s in measured]), samples=sample_rows, probes=probes,
        stage_median_ms={key: statistics.median(sum(c['elapsed_ms'] for c in s['calls'] if c['category'] == key)
            for s in measured) for key in sorted({c['category'] for s in measured for c in s['calls']})},
        restart_worker_to_ready_ms=restart_ms, snapshot_bytes=len(capacity.analytics.encoded(last_snapshot)),
        definition_sha256=hashlib.sha256(capacity.analytics.encoded(value)).hexdigest(),
        native_parity=True, retained_results=len(retained), restart_cached=True, restart_recomputed=True,
        old_bytes_unchanged=True, processes_stopped=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary-dir', type=Path, required=True)
    parser.add_argument('--samples', type=int, default=3)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if not 3 <= args.samples <= 20:
        parser.error('samples must be 3..20')
    cases = [measure(args.binary_dir, lots, receipts, args.samples) for lots, receipts in [(6, 12), (10, 0)]]
    result = dict(status='passed', clock='perf_counter_ns', mode='sequential loopback demo replay',
        warmup_updates=1, samples_per_case=args.samples, cases=cases,
        binaries={str((args.binary_dir / n).resolve()): hashlib.sha256((args.binary_dir / n).read_bytes()).hexdigest()
            for n in ['convex-server', 'convex-core-inputs', 'examples/book_capacity_native']},
        harness_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        execution_authority='none', latency_certification=False)
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(dict(status='passed', output=str(args.output), cases=[dict(trades=c['trades'],
        update_median_ms=c['update']['median_ms'], stages=c['stage_median_ms'], probes=c['probes']) for c in cases]), indent=2))


if __name__ == '__main__':
    main()
