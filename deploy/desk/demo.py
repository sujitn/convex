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


def stamp() -> str:
    return seed.stamp(datetime.now(timezone.utc))


def step(message: str) -> None:
    print(f'desk demo: {message}', flush=True)


def run(command: list[str], *, cwd: Path = ROOT, capture: bool = False) -> str:
    """Run `command`; on failure print its output and stop."""
    done = subprocess.run(command, cwd=cwd, capture_output=True, text=True)
    if done.returncode != 0:
        sys.stderr.write(done.stdout + done.stderr)
        raise SystemExit(f'desk demo: failed: {" ".join(command)}')
    return done.stdout if capture else ''


class Server:
    def __init__(self, port: int):
        self.base = f'http://127.0.0.1:{port}'

    def call(self, method: str, path: str, body=None, headers: dict | None = None):
        data = None if body is None else json.dumps(body).encode()
        request = urllib.request.Request(self.base + path, data=data, method=method,
                                         headers={'content-type': 'application/json', **(headers or {})})
        try:
            with urllib.request.urlopen(request, timeout=10) as response:
                text = response.read()
                return response.status, json.loads(text) if text else None
        except urllib.error.HTTPError as error:
            return error.code, error.read().decode(errors='replace')[:400]


def build(debug: bool) -> Path:
    profile = [] if debug else ['--release']
    step('building the server, its tools and the simulators')
    run(['cargo', 'build', '--locked', '-q', *profile, '-p', 'convex-server', '--bins'])
    if not (ROOT / 'node_modules').exists():
        step('installing the web apps\' packages')
        run(['npm', 'ci', '--no-audit', '--no-fund'])
    step('building the desk app')
    run(['npm', 'run', 'build', '-w', 'desk'])
    return ROOT / 'target' / ('debug' if debug else 'release')


def activate(bin_dir: Path, work: Path, database: Path) -> int:
    """Import the seed and activate its configuration; how many bonds it values."""
    step('importing the seed')
    run([str(bin_dir / 'convex-core-inputs'), str(database), str(work / 'seed.json')])
    config = [str(bin_dir / 'convex-config'), '--demo', str(database)]
    step('compiling, staging and activating DESK-USD')
    document = json.loads(run([*config, 'compile', str(work / 'selection.json')], capture=True))
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
    return len(document['assembly']['instruments'])


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


def wait_until_valued(server: Server, instruments: int) -> None:
    """Until every bond has a complete Live result, or report those without one."""
    deadline = time.monotonic() + VALUED_WITHIN_S
    incomplete: list[str] = []
    while time.monotonic() < deadline:
        status, page = server.call('GET', '/api/v1/runtime/results?limit=500')
        if status == 200:
            results = page['results']
            incomplete = [r['result']['provenance']['subject']['instrument_id'] for r in results
                          if r['result']['status'] != 'Complete']
            if len(results) == instruments and not incomplete:
                return
        time.sleep(1)
    raise SystemExit(f'desk demo: bonds not valued within {VALUED_WITHIN_S} s: {incomplete or "no results"}')


def set_up_books(server: Server, desk: dict) -> None:
    for book_id, limits in desk['limits'].items():
        status, body = server.call('PUT', f'/api/v1/books/{book_id}/limits', {'limits': limits},
                                   {'Idempotency-Key': f'desk-demo-limits-{book_id}'})
        if status not in (200, 201):
            raise SystemExit(f'desk demo: limits for {book_id} refused ({status}): {body}')
    for book_id in desk['books']:
        status, body = server.call('POST', f'/api/v1/books/{book_id}/close', None,
                                   {'Idempotency-Key': f'desk-demo-close-{book_id}-{uuid.uuid4()}'})
        if status not in (200, 201):
            raise SystemExit(f'desk demo: the close of {book_id} was refused ({status}): {body}')


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('--port', type=int, default=8080)
    parser.add_argument('--work', type=Path, default=ROOT / 'target' / 'desk-demo')
    parser.add_argument('--debug', action='store_true', help='use a debug build')
    parser.add_argument('--no-build', action='store_true', help='use the binaries and desk app already built')
    parser.add_argument('--rfq-rate', type=float, default=0.2, help='client requests a second')
    options = parser.parse_args()

    bin_dir = ROOT / 'target' / ('debug' if options.debug else 'release')
    if not options.no_build:
        bin_dir = build(options.debug)
    work = options.work.resolve()
    shutil.rmtree(work, ignore_errors=True)
    logs = work / 'logs'
    logs.mkdir(parents=True)
    database = work / 'convex.redb'
    facts = seed.generate(work)
    step(f'seeded {facts["bonds"]} bonds settling {facts["settlement"]}')
    instruments = activate(bin_dir, work, database)
    desk = json.loads((work / 'desk.json').read_text())

    processes: dict[str, subprocess.Popen] = {}

    def start(name: str, command: list[str]) -> None:
        log = open(logs / f'{name}.log', 'w')
        processes[name] = subprocess.Popen(command, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT)

    def stop(*_) -> None:
        for process in processes.values():
            if process.poll() is None:
                process.terminate()
        for process in processes.values():
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()

    signal.signal(signal.SIGTERM, lambda *_: (stop(), sys.exit(0)))
    server = Server(options.port)
    try:
        step('starting the server')
        start('server', [str(bin_dir / 'convex-server'), str(server_settings(work, database, options.port))])
        for _ in range(60):
            if processes['server'].poll() is not None:
                raise SystemExit(f'desk demo: the server stopped; see {logs / "server.log"}')
            try:
                if server.call('GET', '/health')[0] == 200:
                    break
            except OSError:
                pass
            time.sleep(0.5)
        start('feed', [str(bin_dir / 'convex-feed-sim'), '--server', server.base, '--feed', str(work / 'feed.json')])
        status, body = server.call('POST', '/api/v1/market/levels', {'source': seed.LEVEL_SOURCE, 'levels': desk['marks']})
        if status != 202:
            raise SystemExit(f'desk demo: the opening marks were refused ({status}): {body}')
        step(f'waiting for {instruments} bonds to be valued')
        wait_until_valued(server, instruments)
        step('setting limits and taking a close of each book')
        set_up_books(server, desk)
        books = ','.join(desk['books'])
        start('rfq-sim', [str(bin_dir / 'convex-rfq-sim'), '--server', server.base, '--source-id', 'venue-sim',
                          '--books', books, '--bonds', ','.join(desk['rfq_bonds']), '--tiers', ','.join(desk['tiers']),
                          '--rate', str(options.rfq_rate)])
        start('bms-sim', [str(bin_dir / 'convex-bms-sim'), '--server', server.base, '--books', books])
        print(f'\nThe desk demo is running: {server.base}/desk/\n'
              f'  pricing sheet {server.base}/desk/sheet, RFQs {server.base}/desk/rfqs, books {server.base}/desk/books\n'
              f'  logs in {logs}; Ctrl-C stops it.\n', flush=True)
        while all(process.poll() is None for process in processes.values()):
            time.sleep(1)
        stopped = [name for name, process in processes.items() if process.poll() is not None]
        raise SystemExit(f'desk demo: {", ".join(stopped)} stopped; see {logs}')
    except KeyboardInterrupt:
        step('stopping')
    finally:
        stop()


if __name__ == '__main__':
    main()
