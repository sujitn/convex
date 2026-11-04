#!/usr/bin/env python3
"""Run the existing core book with a real sequenced NDJSON stream and recovery."""
import argparse
import copy
import http.server
import json
from pathlib import Path
import queue
import subprocess
import sys
import tempfile
import threading

import core_snapshot_workflow as core

ROOT = core.ROOT
require = core.require


def item(kind, payload):
    return dict(schema_version=1, kind=kind, payload=payload)


def universe(records):
    curves, fixings = {}, {}
    for r in records:
        if r['kind'] == 'curve':
            p = r['payload']
            curves[(p['curve_id'], p['source'])] = dict(curve_id=p['curve_id'], source=p['source'])
        else:
            for p in r['payload']:
                k = p['key']
                fixings[(k['index_id'], k['observation_date'], k['source'])] = k
    return dict(schema_version=1, curves=list(curves.values()), fixings=list(fixings.values()))


class LocalFeed:
    """Owned synthetic network producer used only by this runnable example/tests."""
    def __init__(self, records):
        self.records = copy.deepcopy(records)
        self.lock = threading.Lock()
        self.active = None
        self.connections = 0
        self.closed = False
        self.requests = []
        feed = self

        class Handler(http.server.BaseHTTPRequestHandler):
            def log_message(self, *_):
                pass

            def do_GET(self):
                self.connection.settimeout(2)
                with feed.lock:
                    if feed.active is not None:
                        try:
                            feed.active.put_nowait(None)
                        except queue.Full:
                            pass
                    feed.connections += 1
                    feed.requests.append(dict(image_required=self.headers.get('X-Convex-Require-Image'),
                                              stream=self.headers.get('X-Convex-Stream')))
                    channel = queue.Queue(maxsize=8)
                    feed.active = channel
                    feed.sequence = 1
                    feed.session = 'connection-' + str(feed.connections)
                    feed.last = feed.frame('image', feed.records)
                    image = copy.deepcopy(feed.last)
                self.send_response(200)
                self.send_header('Content-Type', 'application/x-ndjson')
                self.end_headers()
                try:
                    self.wfile.write(json.dumps(image).encode() + b'\n')
                    self.wfile.flush()
                    while not feed.closed:
                        try:
                            frame = channel.get(timeout=0.1)
                        except queue.Empty:
                            continue
                        if frame is None:
                            break
                        self.wfile.write(json.dumps(frame).encode() + b'\n')
                        self.wfile.flush()
                except OSError:
                    pass

        self.server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.url = f'http://127.0.0.1:{self.server.server_port}/feed'

    def frame(self, kind, records):
        return dict(schema_version=1, stream_id='core.synthetic', session=self.session,
                    sequence=self.sequence, kind=kind, records=copy.deepcopy(records))

    def send(self, record, *, gap=False):
        with self.lock:
            self.sequence += 2 if gap else 1
            if not gap:
                self.records.append(copy.deepcopy(record))
            self.last = self.frame('update', [record])
            self.active.put_nowait(copy.deepcopy(self.last))

    def duplicate(self):
        with self.lock:
            self.active.put_nowait(copy.deepcopy(self.last))

    def heartbeat(self):
        with self.lock:
            self.active.put_nowait(self.frame('heartbeat', []))

    def disconnect(self):
        with self.lock:
            self.active.put_nowait(None)

    def __enter__(self):
        self.thread.start()
        return self

    def __exit__(self, *_):
        self.closed = True
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=3)
        require(not self.thread.is_alive(), 'Owned synthetic feed did not stop')


class StreamProcess:
    def __init__(self, feed, url, subscription):
        self.events = queue.Queue(maxsize=64)
        self.accepted = 0
        self.command = [sys.executable, str(ROOT / 'tools/market_data_stream.py'),
                        '--feed-url', feed.url, '--url', url, '--stream-id', 'core.synthetic',
                        '--subscription', str(subscription), '--timeout', '3', '--retry-delay', '0.02']

    def __enter__(self):
        self.process = subprocess.Popen(self.command, cwd=ROOT, text=True,
                                        stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        def read():
            try:
                for line in self.process.stdout:
                    self.events.put(json.loads(line), timeout=5)
            finally:
                self.events.put(None, timeout=5)
        self.reader = threading.Thread(target=read, daemon=True)
        self.reader.start()
        try:
            self.wait('streaming', 1)
        except BaseException:
            self.__exit__()
            raise
        return self

    def wait(self, state, accepted=None):
        for _ in range(64):
            try:
                event = self.events.get(timeout=15)
            except queue.Empty:
                raise RuntimeError('Timed out waiting for streaming state: ' + state) from None
            require(event is not None, 'Streaming adapter ended before expected state')
            self.accepted = event['accepted_frames']
            if event['state'] == state and (accepted is None or self.accepted >= accepted):
                return event
        raise RuntimeError('Streaming state budget exhausted')

    def __exit__(self, *_):
        if self.process.poll() is None:
            self.process.terminate()
        self.process.wait(timeout=10)
        self.reader.join(timeout=3)
        error = self.process.stderr.read()
        self.process.stdout.close()
        self.process.stderr.close()
        require(not self.reader.is_alive() and self.process.returncode == 0,
                'Owned streaming adapter failed: ' + error)


class StreamingClient(core.Client):
    def __init__(self, server, feed, adapter):
        super().__init__(server)
        self.feed, self.adapter = feed, adapter

    def call(self, method, path, value=None, status=200, raw=False):
        kinds = {'/analytics/curves': 'curve', '/analytics/fixings': 'fixings'}
        if method == 'POST' and path in kinds and status == 204:
            wanted = self.adapter.accepted + 1
            self.feed.send(item(kinds[path], value))
            self.adapter.wait('streaming', wanted)
            return None
        return super().call(method, path, value, status, raw)


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
    records = [item('curve', c) for c in capture['curves']] + [item('fixings', capture['fixings'])]
    with tempfile.TemporaryDirectory(prefix='convex-core-stream-') as name, LocalFeed(records) as feed:
        folder = Path(name)
        selection = folder / 'subscription.json'
        selection.write_text(json.dumps(universe(records)))
        source = folder / 'references.json'
        source.write_text(json.dumps(dict(schema_version=1, references=capture['references'], curves=[], fixings=[])))
        subprocess.run([str(importer), str(folder / 'state.redb'), str(source)], cwd=ROOT, check=True, stdout=subprocess.PIPE)
        with core.supplied.ServerProcess(binary, folder) as server:
            with StreamProcess(feed, core.Client(server).root, selection) as adapter:
                client = StreamingClient(server, feed, adapter)
                summary, saved = core.exercise(client, capture, plan, factors)
                latest = copy.deepcopy(plan)
                latest['definition']['version'] = 2
                next(p for p in latest['curve_sets']['market.1'] if p['id'] == 'D.OIS')['version'] = 3
                before = adapter.accepted
                leaked = copy.deepcopy(capture['fixings'][0])
                leaked['version'] = 99
                feed.send(item('fixings', [leaked]), gap=True)
                gap = adapter.wait('recovering')
                require(gap['reason'] == 'sequence_gap' and not gap['image_confirmed'], 'Gap did not suspend continuity')
                adapter.wait('streaming', before + 1)
                require(client.assemble(latest) == summary['snapshots'][2], 'Gapped update leaked into storage')
                before = adapter.accepted
                feed.duplicate()
                require(adapter.wait('duplicate')['accepted_frames'] == before, 'Duplicate advanced stream sequence')
                feed.heartbeat()
                require(adapter.wait('heartbeat')['accepted_frames'] == before, 'Heartbeat advanced data')
                feed.disconnect()
                require(not adapter.wait('recovering')['image_confirmed'], 'Disconnect preserved continuity')
                adapter.wait('streaming', before + 1)
            with StreamProcess(feed, client.root, selection):
                require(client.assemble(latest) == summary['snapshots'][2], 'Adapter restart changed saved inputs')
        source.unlink()
        with core.supplied.ServerProcess(binary, folder) as server:
            client = core.Client(server)
            with StreamProcess(feed, client.root, selection):
                require(client.assemble(latest) == summary['snapshots'][2], 'Server restart changed image replay')
                for receipt, raw in saved:
                    reused = client.run(receipt['snapshot_id'], receipt['calculation'])
                    require(reused['reused'] and client.result(reused, raw=True) == raw, 'Restart changed durable results')
                for receipt, raw in [saved[0], saved[-2], saved[-1]]:
                    replay = client.run(receipt['snapshot_id'], receipt['calculation'], recompute=True)
                    require(not replay['reused'] and client.result(replay, raw=True) == raw, 'Saved-input recomputation changed')
        require(all(r == dict(image_required='true', stream='core.synthetic') for r in feed.requests),
                'Reconnect did not explicitly request a fresh image')
        summary.update(stream_connections=feed.connections, gap_update_not_published=True,
                       duplicate_and_heartbeat_preserve_data=True, reconnect_requires_complete_image=True,
                       adapter_restart_recovered=True, server_restart_recovered=True,
                       restart_results_identical=True, saved_snapshot_recomputation_identical=True,
                       all_owned_processes_stopped=True)
    print(json.dumps(summary, indent=2))


if __name__ == '__main__':
    try:
        main()
    except (RuntimeError, OSError, ValueError, subprocess.SubprocessError, StopIteration, KeyError, queue.Empty) as error:
        print(str(error), file=sys.stderr)
        raise SystemExit(1)
