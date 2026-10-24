"""Actual HTTP delivery, immutable retry and fail-closed ingress checks."""
import contextlib
import copy
import http.server
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import threading
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import market_data_adapter as adapter


def curve(value='0.0312500000000000000000000000'):
    return dict(schema_version=1, kind='curve', payload=dict(
        curve_id='CURVE-A', version=1, source='provider-neutral',
        source_time='2026-10-17T00:00:00Z', recorded_at='2026-10-17T00:00:01Z',
        quality='Demo', recipe=dict(quote=value)))


@contextlib.contextmanager
def receiver(mode=None):
    state = dict(calls=[], saved={}, mode=mode)

    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_POST(self):
            body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            state['calls'].append((self.path, body, self.headers.get('Authorization')))
            if state['mode'] == 'deny':
                self.send_response(403)
                self.end_headers()
                self.wfile.write(b'provider/private terms and secrets must not escape')
                return
            if isinstance(state['mode'], str) and state['mode'].startswith('http'):
                self.send_response(307)
                self.send_header('Location', state['mode'])
                self.end_headers()
                return
            identity = (body['curve_id'], body['version']) if isinstance(body, dict) else 'fixings'
            if identity in state['saved'] and state['saved'][identity] != body:
                self.send_response(409)
                self.end_headers()
                return
            state['saved'][identity] = body
            if state['mode'] == 'lost_ack':
                state['mode'] = None
                self.close_connection = True
                self.connection.shutdown(socket.SHUT_RDWR)
                return
            self.send_response(204)
            self.end_headers()

    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield f'http://127.0.0.1:{server.server_port}/api/v1', state
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=3)
        if thread.is_alive():
            raise RuntimeError('Owned HTTP test server did not stop')


class MarketDataAdapterTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix='convex-ingestion-test-')
        self.addCleanup(self.directory.cleanup)
        self.path = Path(self.directory.name) / 'market.jsonl'

    def write(self, values):
        self.path.write_text(''.join(json.dumps(v) + '\n' for v in values))

    def test_exact_units_metadata_order_and_duplicate_replay(self):
        item = curve()
        fixing = dict(schema_version=1, kind='fixings', payload=[dict(
            key=dict(index_id='USD.SOFR', source='other-provider'), version=17,
            rate=dict(value='-0.0000000000000000000000000001', unit='decimal_rate'),
            is_preliminary=True, entitled=False)])
        self.write([item, item, fixing])
        with receiver() as (url, state):
            for _ in range(2):
                result = adapter.publish_file(self.path, url)
                self.assertEqual(result['acknowledged'], 3)
            self.assertEqual(len(state['saved']), 2)
            self.assertEqual([c[0] for c in state['calls'][:3]],
                             ['/api/v1/analytics/curves'] * 2 + ['/api/v1/analytics/fixings'])
            self.assertEqual(state['calls'][0][1], item['payload'])
            self.assertEqual(state['calls'][2][1], fixing['payload'])

    def test_commit_then_lost_ack_replay_is_idempotent(self):
        self.write([curve()])
        with receiver('lost_ack') as (url, state):
            with self.assertRaises(adapter.IngestionError) as failed:
                adapter.publish_file(self.path, url)
            self.assertEqual(failed.exception.acknowledged, 0)
            self.assertEqual(failed.exception.line, 1)
            self.assertEqual(len(state['saved']), 1)
            self.assertEqual(adapter.publish_file(self.path, url)['acknowledged'], 1)
            self.assertEqual(len(state['saved']), 1)
            self.assertEqual(len(state['calls']), 2)

    def test_conflict_stops_following_records_and_replays_committed_prefix(self):
        first = curve()
        conflict = curve('0.04')
        later = copy.deepcopy(first)
        later['payload']['version'] = 2
        self.write([first, conflict, later])
        with receiver() as (url, state):
            with self.assertRaises(adapter.IngestionError) as failed:
                adapter.publish_file(self.path, url)
            self.assertEqual((failed.exception.line, failed.exception.http_status,
                              failed.exception.acknowledged), (2, 409, 1))
            self.assertEqual(len(state['calls']), 2)
            self.assertEqual(len(state['saved']), 1)
            self.write([first, later])
            self.assertEqual(adapter.publish_file(self.path, url)['acknowledged'], 2)
            self.assertEqual(state['saved'][('CURVE-A', 1)], first['payload'])
            self.assertEqual(state['saved'][('CURVE-A', 2)], later['payload'])

    def test_entire_capture_is_preflighted_before_publication(self):
        invalid = [b'not JSON', b'{}',
                   b'{"schema_version":1,"kind":"curve","kind":"fixings","payload":[]}',
                   b'{"schema_version":1,"kind":"curve","payload":{"x":NaN}}',
                   b'{"schema_version":1,"kind":"curve","payload":{"x":1e9999}}',
                   b'{"schema_version":true,"kind":"curve","payload":{}}',
                   b'{"schema_version":1,"kind":"fixings","payload":[]}',
                   b'{"schema_version":1,"kind":"unknown","payload":{}}',
                   b'{"schema_version":1,"kind":"curve","payload":{},"ignored":true}',
                   b'{"schema_version":1,"kind":"curve","payload":{"x":1,"x":2}}',
                   b'\xff']
        with receiver() as (url, state):
            for content in invalid:
                with self.subTest(content=content):
                    self.path.write_bytes(json.dumps(curve()).encode() + b'\n' + content + b'\n')
                    with self.assertRaises(adapter.IngestionError) as failed:
                        adapter.publish_file(self.path, url)
                    self.assertEqual(failed.exception.line, 2)
                    self.assertEqual(state['calls'], [])

    def test_redirects_cannot_forward_credentials_or_payload(self):
        self.write([curve()])
        with receiver() as (target, target_state), receiver(target) as (url, _):
            with self.assertRaises(adapter.IngestionError) as failed:
                adapter.publish_file(self.path, url, token='secret-bearer')
            self.assertEqual(failed.exception.http_status, 307)
            self.assertEqual(target_state['calls'], [])
            self.assertNotIn('secret-bearer', json.dumps(failed.exception.diagnostic()))

    def test_permission_rejection_and_cli_output_are_redacted(self):
        self.write([curve('private-payload-value')])
        with receiver('deny') as (url, state):
            completed = subprocess.run([sys.executable, str(Path(adapter.__file__)), str(self.path),
                                        '--url', url, '--token-env', 'TEST_MARKET_TOKEN'],
                                       env={**os.environ, 'TEST_MARKET_TOKEN': 'private-token'},
                                       text=True, capture_output=True, check=False)
            self.assertEqual(completed.returncode, 1)
            self.assertEqual(completed.stdout, '')
            result = json.loads(completed.stderr)
            self.assertEqual((result['http_status'], result['acknowledged']), (403, 0))
            for secret in ['private-token', 'private-payload-value', 'provider/private']:
                self.assertNotIn(secret, completed.stderr)
            self.assertEqual(state['calls'][0][2], 'Bearer private-token')

    def test_destination_token_and_time_bounds_fail_before_io(self):
        self.write([curve()])
        invalid = ['http://remote.example/api/v1', 'ftp://localhost/api/v1',
                   'http://user:password@127.0.0.1/api/v1',
                   'http://127.0.0.1/api/v1?token=hidden', 'http://127.0.0.1/api/v1#hidden',
                   'http://127.0.0.1:999999/api/v1', 'http://127.0.0.1/api/v1\r\n']
        for url in invalid:
            with self.subTest(url=url), self.assertRaises(adapter.IngestionError):
                adapter.publish_file(self.path, url)
        for token in ['', 'line\r\ninjection', 'x' * 8193]:
            with self.assertRaises(adapter.IngestionError):
                adapter.publish_file(self.path, 'http://127.0.0.1/api/v1', token=token)
        for timeout in [0, 121, float('nan')]:
            with self.assertRaises(adapter.IngestionError):
                adapter.publish_file(self.path, 'http://127.0.0.1/api/v1', timeout=timeout)
        self.assertEqual(adapter.endpoint('https://example.test/api/v1/'), 'https://example.test/api/v1')

    def test_bounded_file_record_count_and_json_depth(self):
        self.path.write_bytes(b'x' * 17)
        with mock.patch.object(adapter, 'MAX_FILE_BYTES', 16), self.assertRaises(adapter.IngestionError):
            adapter.load_messages(self.path)
        self.write([curve(), curve()])
        with mock.patch.object(adapter, 'MAX_RECORDS', 1), self.assertRaises(adapter.IngestionError):
            adapter.load_messages(self.path)
        self.path.write_bytes(json.dumps(curve()).encode() + b'\r\n')
        with mock.patch.object(adapter, 'MAX_RECORDS', 1):
            self.assertEqual(len(adapter.load_messages(self.path)), 1)
        with mock.patch.object(adapter, 'MAX_RECORD_BYTES', 16), self.assertRaises(adapter.IngestionError):
            adapter.load_messages(self.path)
        self.path.write_text('{"schema_version":1,"kind":"curve","payload":{"x":' + '[' * 65 + '0' + ']' * 65 + '}}\n')
        with self.assertRaises(adapter.IngestionError):
            adapter.load_messages(self.path)


if __name__ == '__main__':
    unittest.main()
