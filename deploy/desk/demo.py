#!/usr/bin/env python3
"""The desk demo in one command (stdlib only), on this machine.

    python3 deploy/desk/demo.py [--port 8080] [--work target/desk-demo] [--debug]
                                [--no-build] [--rfq-rate 0.2]

It builds the server, its tools and the desk app, then on a fresh database:

1. writes the synthetic seed for now (`seed.py`) and imports it;
2. compiles, stages and activates the `DESK-USD` configuration under the demo profile,
   its fixture asserting each bond's opening quote;
3. starts the server with the desk app, the feed, and the desk's opening marks;
4. once every bond is valued, sets each book's limits and takes a close of each, so
   P&L starts at zero;
5. starts the RFQ and BMS simulators.

Open the printed URL. Ctrl-C stops everything; so does any process stopping. Logs are in
the work directory's `logs/`. Every output is labelled demo, and nothing is published:
the server listens on loopback only, with no authentication.
"""
from __future__ import annotations

import argparse
import json
import shutil
import signal
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.request
import uuid
from datetime import datetime, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import seed  # noqa: E402

ROOT = seed.ROOT
VALUED_WITHIN_S = 120
LISTENING_WITHIN_S = 30


def step(message: str) -> None:
    print(f'desk demo: {message}', flush=True)


def run(command: list[str]) -> str:
    """Run `command` from the repository root, its output; on failure print it and stop."""
    done = subprocess.run(command, cwd=ROOT, capture_output=True, text=True)
    if done.returncode != 0:
        sys.stderr.write(done.stdout + done.stderr)
        raise SystemExit(f'desk demo: failed: {" ".join(command)}')
    return done.stdout


class Server:
    """The server's HTTP API on loopback, past any proxy the environment names."""

    def __init__(self, port: int):
        self.base = f'http://127.0.0.1:{port}'
        self.opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

    def call(self, method: str, path: str, body=None, headers: dict | None = None):
        """The status and JSON reply, or the error's text."""
        data = None if body is None else json.dumps(body).encode()
        request = urllib.request.Request(self.base + path, data=data, method=method,
                                         headers={'content-type': 'application/json', **(headers or {})})
        try:
            with self.opener.open(request, timeout=10) as response:
                text = response.read()
                return response.status, json.loads(text) if text else None
        except urllib.error.HTTPError as error:
            return error.code, error.read().decode(errors='replace')[:400]

    def require(self, what: str, method: str, path: str, body=None, headers: dict | None = None):
        """`call`, stopping the demo unless it succeeds."""
        status, reply = self.call(method, path, body, headers)
        if not 200 <= status < 300:
            raise SystemExit(f'desk demo: {what} refused ({status}): {reply}')
        return reply


class Processes:
    """The demo's processes, each logging to its own file."""

    def __init__(self, logs: Path):
        self.logs = logs
        self.running: dict[str, subprocess.Popen] = {}

    def start(self, name: str, command: list[str]) -> subprocess.Popen:
        log = open(self.logs / f'{name}.log', 'w')
        self.running[name] = subprocess.Popen(command, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT)
        return self.running[name]

    def stopped(self) -> list[str]:
        return [name for name, process in self.running.items() if process.poll() is not None]

    def stop(self) -> None:
        for process in self.running.values():
            if process.poll() is None:
                process.terminate()
        for process in self.running.values():
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()


def build(debug: bool) -> None:
    step('building the server, its tools and the simulators')
    run(['cargo', 'build', '--locked', '-q', *([] if debug else ['--release']), '-p', 'convex-server', '--bins'])
    if not (ROOT / 'node_modules').exists():
        step('installing the web apps\' packages')
        run(['npm', 'ci', '--no-audit', '--no-fund'])
    step('building the desk app')
    run(['npm', 'run', 'build', '-w', 'desk'])


def activate(bin_dir: Path, work: Path, database: Path) -> None:
    """Import the seed, and compile, stage and activate its configuration."""
    stamp = lambda: seed.stamp(datetime.now(timezone.utc))  # noqa: E731
    step('importing the seed')
    run([str(bin_dir / 'convex-core-inputs'), str(database), str(work / 'seed.json')])
    config = [str(bin_dir / 'convex-config'), '--demo', str(database)]
    step(f'compiling, staging and activating {seed.CONFIG_ID}')
    document = json.loads(run([*config, 'compile', str(work / 'selection.json')]))
    fixtures, events = str(work / 'fixtures.json'), str(work / 'events.jsonl')
    (work / 'staged.json').write_text(json.dumps({
        'document': document, 'idempotency_key': 'desk-demo-stage', 'actor': 'desk-demo',
        'reason': 'the desk demo, compiled at launch', 'staged_at': stamp()}))
    run([*config, 'stage', str(work / 'staged.json'), fixtures, events])
    (work / 'activation.json').write_text(json.dumps({
        'id': seed.CONFIG_ID, 'configuration_version': 1, 'expected_revision': 0,
        'idempotency_key': 'desk-demo-activate', 'actor': 'desk-demo', 'reason': 'start the desk demo',
        'activated_at': stamp(), 'mode': 'restart_required', 'action': {'kind': 'activate'}}))
    run([*config, 'activate', str(work / 'activation.json'), fixtures, events])


def server_settings(work: Path, database: Path, port: int) -> Path:
    settings = work / 'server.toml'
    settings.write_text(f'''# The desk demo's server: loopback only, no authentication, every output labelled demo.
profile = "demo"
configuration_id = "{seed.CONFIG_ID}"
host = "127.0.0.1"
port = {port}
websocket_enabled = true
storage_path = "{database}"
desk_ui_dir = "{ROOT / 'desk' / 'dist'}"
desk_level_source = "{seed.LEVEL_SOURCE}"

[access]
mode = "local_development"
cors_origins = ["http://127.0.0.1:{port}"]
''')
    return settings


def refuse_a_taken_port(port: int) -> None:
    """Stop the demo if something already listens on `port`, such as an earlier demo: the
    launcher would otherwise set up whatever answers there."""
    with socket.socket() as probe:
        if probe.connect_ex(('127.0.0.1', port)) == 0:
            raise SystemExit(f'desk demo: 127.0.0.1:{port} is in use; stop what listens there or pass --port')


def wait_until_listening(server: Server, process: subprocess.Popen, log: Path) -> None:
    deadline = time.monotonic() + LISTENING_WITHIN_S
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise SystemExit(f'desk demo: the server stopped; see {log}')
        try:
            if server.call('GET', '/health')[0] == 200:
                return
        except OSError:
            pass
        time.sleep(0.5)
    raise SystemExit(f'desk demo: the server did not listen within {LISTENING_WITHIN_S} s; see {log}')


def wait_until_valued(server: Server, instrument_count: int) -> None:
    """Until every bond has a complete Live result, or report those without one."""
    deadline = time.monotonic() + VALUED_WITHIN_S
    incomplete: list[str] = []
    while time.monotonic() < deadline:
        status, page = server.call('GET', '/api/v1/runtime/results?limit=500')
        if status == 200:
            results = page['results']
            incomplete = [r['result']['provenance']['subject']['instrument_id'] for r in results
                          if r['result']['status'] != 'Complete']
            if len(results) == instrument_count and not incomplete:
                return
        time.sleep(1)
    raise SystemExit(f'desk demo: bonds not valued within {VALUED_WITHIN_S} s: {incomplete or "no results"}')


def set_up_books(server: Server) -> None:
    """Each book's limits, then a close of each, so its P&L starts at zero."""
    for book_id, limits in seed.LIMITS.items():
        server.require(f'the limits of {book_id}', 'PUT', f'/api/v1/books/{book_id}/limits', {'limits': limits},
                       {'Idempotency-Key': f'desk-demo-limits-{book_id}'})
    for book_id in seed.BOOKS:
        server.require(f'the close of {book_id}', 'POST', f'/api/v1/books/{book_id}/close', None,
                       {'Idempotency-Key': f'desk-demo-close-{book_id}-{uuid.uuid4()}'})


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('--port', type=int, default=8080)
    parser.add_argument('--work', type=Path, default=ROOT / 'target' / 'desk-demo')
    parser.add_argument('--debug', action='store_true', help='use a debug build')
    parser.add_argument('--no-build', action='store_true', help='use the binaries and desk app already built')
    parser.add_argument('--rfq-rate', type=float, default=0.2, help='client requests a second')
    options = parser.parse_args()

    refuse_a_taken_port(options.port)
    if not options.no_build:
        build(options.debug)
    bin_dir = ROOT / 'target' / ('debug' if options.debug else 'release')
    work = options.work.resolve()
    shutil.rmtree(work, ignore_errors=True)
    logs = work / 'logs'
    logs.mkdir(parents=True)
    database = work / 'convex.redb'
    launch = seed.generate(work)
    step(f'seeded {launch.instrument_count} bonds settling {launch.settlement}')
    activate(bin_dir, work, database)

    processes = Processes(logs)
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(0))
    server = Server(options.port)
    try:
        step('starting the server')
        listening = processes.start('server', [str(bin_dir / 'convex-server'), str(server_settings(work, database, options.port))])
        wait_until_listening(server, listening, logs / 'server.log')
        processes.start('feed', [str(bin_dir / 'convex-feed-sim'), '--server', server.base, '--feed', str(work / 'feed.json')])
        server.require('the opening marks', 'POST', '/api/v1/market/levels', {'source': seed.LEVEL_SOURCE, 'levels': launch.marks})
        step(f'waiting for {launch.instrument_count} bonds to be valued')
        wait_until_valued(server, launch.instrument_count)
        step('setting limits and taking a close of each book')
        set_up_books(server)
        books = ','.join(seed.BOOKS)
        processes.start('rfq-sim', [str(bin_dir / 'convex-rfq-sim'), '--server', server.base, '--source-id', 'venue-sim',
                                    '--books', books, '--bonds', ','.join(launch.rfq_bonds), '--tiers', ','.join(seed.TIERS),
                                    '--rate', str(options.rfq_rate)])
        processes.start('bms-sim', [str(bin_dir / 'convex-bms-sim'), '--server', server.base, '--books', books])
        print(f'\nThe desk demo is running: {server.base}/desk/\n'
              f'  pricing sheet {server.base}/desk/sheet, RFQs {server.base}/desk/rfqs, books {server.base}/desk/books\n'
              f'  logs in {logs}; Ctrl-C stops it.\n', flush=True)
        while not processes.stopped():
            time.sleep(1)
        raise SystemExit(f'desk demo: {", ".join(processes.stopped())} stopped; see {logs}')
    except KeyboardInterrupt:
        step('stopping')
    finally:
        processes.stop()


if __name__ == '__main__':
    main()
