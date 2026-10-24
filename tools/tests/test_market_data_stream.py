"""Real HTTP stream transport and existing immutable publication boundary."""
import contextlib
import copy
import http.server
import json
from pathlib import Path
import sys
import tempfile
import threading
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import market_data_adapter as adapter
import market_data_stream as stream
from test_market_data_adapter import curve, receiver

UNIVERSE = ({('CURVE-A', 'provider-neutral')}, set())


def frame(sequence=1, kind='image', records=None, session='A'):
    return dict(schema_version=1, stream_id='test.stream', session=session,
                sequence=sequence, kind=kind, records=[curve()] if records is None else records)


def updated(version=2):
    value = curve()
    value['payload']['version'] = version
    return value


@contextlib.contextmanager
def feed(connections, *, redirect=None):
    state = dict(requests=[], ended=threading.Event())
    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_GET(self):
            n = len(state['requests'])
            state['requests'].append(dict(image=self.headers.get('X-Convex-Require-Image'),
                                          token=self.headers.get('Authorization')))
            self.send_response(307 if redirect else 200)
            self.send_header('Content-Type', 'application/x-ndjson')
            if redirect:
                self.send_header('Location', redirect)
            self.end_headers()
            if redirect:
                return
            try:
                for value in connections[min(n, len(connections) - 1)]:
                    if value is None:
                        state['ended'].wait(0.5)
                        continue
                    self.wfile.write(value if isinstance(value, bytes) else json.dumps(value).encode() + b'\n')
                    self.wfile.flush()
            except OSError:
                pass
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    task = threading.Thread(target=server.serve_forever, daemon=True)
    task.start()
    try:
        yield f'http://127.0.0.1:{server.server_port}/feed', state
    finally:
        state['ended'].set()
        server.shutdown()
        server.server_close()
        task.join(timeout=3)
        if task.is_alive():
            raise RuntimeError('Owned feed did not stop')


class MarketDataStreamTests(unittest.TestCase):
    def consume(self, source, sink, events, **kwargs):
        return stream.consume(source, sink, 'test.stream', UNIVERSE,
                              retry_delay=0.01, timeout=1, emit=events.append, **kwargs)

    def test_sequence_gap_discards_update_then_requires_full_image(self):
        events = []
        with receiver() as (sink, saved), feed([
                [frame(), frame(3, 'update', [updated(99)])],
                [frame(10, records=[updated(2)], session='B')]]) as (source, requests):
            self.consume(source, sink, events, max_frames=2)
            self.assertEqual(set(saved['saved']), {('CURVE-A', 1), ('CURVE-A', 2)})
            self.assertEqual(len(requests['requests']), 2)
            self.assertTrue(all(r['image'] == 'true' for r in requests['requests']))
        gaps = [v for v in events if v.get('reason') == 'sequence_gap']
        self.assertEqual(len(gaps), 1)
        self.assertFalse(gaps[0]['image_confirmed'])
        self.assertEqual(events[-1]['state'], 'stopped')
        self.assertFalse(events[-1]['image_confirmed'])

    def test_duplicate_and_heartbeat_do_not_write_or_refresh_data(self):
        events = []
        image = frame()
        with receiver() as (sink, saved), feed([[image, image,
                frame(1, 'heartbeat', []), frame(2, 'update', [updated()])]]) as (source, _):
            self.consume(source, sink, events, max_frames=2)
            self.assertEqual(len(saved['calls']), 2)
            self.assertEqual(saved['calls'][0][1], curve()['payload'])
        self.assertEqual([e['accepted_frames'] for e in events if e['state'] in ('heartbeat', 'duplicate')], [1, 1])

    def test_session_change_cannot_apply_delta_before_new_image(self):
        events = []
        with receiver() as (sink, saved), feed([
                [frame(), frame(2, 'update', [updated(99)], session='B')],
                [frame(records=[updated()], session='B')]]) as (source, _):
            self.consume(source, sink, events, max_frames=2)
            self.assertNotIn(('CURVE-A', 99), saved['saved'])
            self.assertTrue(any(e.get('reason') == 'session_changed' for e in events))

    def test_eof_truncation_and_delta_only_reconnect_are_not_continuity(self):
        events = []
        with receiver() as (sink, saved), feed([
                [b'{"schema_version":'], [frame(2, 'update', [updated(99)])],
                [frame()]]) as (source, requests):
            self.consume(source, sink, events, max_frames=1, reconnects=2)
            self.assertEqual(len(saved['calls']), 1)
            self.assertEqual(len(requests['requests']), 3)
        self.assertEqual([e['reason'] for e in events if e['state'] == 'recovering'],
                         ['truncated_frame', 'image_required'])

    def test_lost_publication_ack_reimages_without_advancing_cursor(self):
        events = []
        image = frame(records=[curve(), updated()])
        retry = copy.deepcopy(image)
        retry['session'] = 'B'
        with receiver('lost_ack') as (sink, saved), feed([[image], [retry]]) as (source, _):
            self.consume(source, sink, events, max_frames=1)
            self.assertEqual(len(saved['saved']), 2)
            self.assertEqual(len(saved['calls']), 3)
        recovery = next(e for e in events if e.get('reason') == 'publication_unconfirmed')
        self.assertEqual(recovery['accepted_frames'], 0)
        self.assertFalse(recovery['image_confirmed'])

    def test_idle_read_timeout_reconnects_without_confirming_old_delivery(self):
        events = []
        with receiver() as (sink, saved), feed([[None], [frame()]]) as (source, requests):
            stream.consume(source, sink, 'test.stream', UNIVERSE, timeout=0.1,
                           retry_delay=0.01, reconnects=1, max_frames=1, emit=events.append)
            self.assertEqual(len(requests['requests']), 2)
            self.assertEqual(len(saved['calls']), 1)
        self.assertTrue(any(e.get('reason') == 'transport_unavailable' and not e['image_confirmed'] for e in events))

    def test_conflicting_duplicate_fails_before_another_publication(self):
        events = []
        with receiver() as (sink, saved), feed([[frame(), frame(records=[updated()])]]) as (source, _):
            with self.assertRaisesRegex(adapter.IngestionError, 'Conflicting stream sequence'):
                self.consume(source, sink, events)
            self.assertEqual(len(saved['calls']), 1)
        self.assertFalse(events[-1]['image_confirmed'])

    def test_permissions_and_separate_feed_sink_credentials_are_preserved(self):
        events = []
        secret = frame(records=[curve('private-provider-data')])
        with receiver('deny') as (sink, saved), feed([[secret]]) as (source, requests):
            with self.assertRaises(adapter.IngestionError) as failure:
                self.consume(source, sink, events, feed_token='feed-secret', server_token='sink-secret')
            self.assertEqual(failure.exception.http_status, 403)
            self.assertEqual(requests['requests'][0]['token'], 'Bearer feed-secret')
            self.assertEqual(saved['calls'][0][2], 'Bearer sink-secret')
        output = json.dumps(events) + json.dumps(failure.exception.diagnostic())
        for value in ['feed-secret', 'sink-secret', 'private-provider-data']:
            self.assertNotIn(value, output)

    def test_source_redirect_cannot_forward_credentials(self):
        events = []
        with receiver() as (sink, saved), feed([[frame()]]) as (target, target_state), feed([[]], redirect=target) as (source, _):
            with self.assertRaises(adapter.IngestionError) as failure:
                self.consume(source, sink, events, feed_token='secret')
            self.assertEqual(failure.exception.http_status, 307)
            self.assertEqual(target_state['requests'], [])
            self.assertEqual(saved['calls'], [])

    def test_incomplete_unknown_or_malformed_frame_never_publishes(self):
        invalid = []
        value = frame()
        value['records'][0]['payload']['source'] = 'unsubscribed'
        invalid.append(json.dumps(value).encode() + b'\n')
        value = frame(records=[])
        invalid.append(json.dumps(value).encode() + b'\n')
        value = frame()
        value['ignored'] = True
        invalid.append(json.dumps(value).encode() + b'\n')
        invalid += [b'{"schema_version":1,"schema_version":1}\n', b'{"value":NaN}\n']
        for raw in invalid:
            with self.subTest(raw=raw), self.assertRaises(adapter.IngestionError):
                stream.decode(raw, 'test.stream', UNIVERSE)
        needed_fixing = ('USD.SOFR', '2026-10-01', 'fixing-source')
        with self.assertRaises(adapter.IngestionError):
            stream.decode(json.dumps(frame()).encode() + b'\n', 'test.stream', (UNIVERSE[0], {needed_fixing}))

    def test_reconnect_and_configuration_budgets_fail_explicitly(self):
        events = []
        with receiver() as (sink, saved), feed([[]]) as (source, requests):
            with self.assertRaisesRegex(adapter.IngestionError, 'reconnect budget'):
                self.consume(source, sink, events, reconnects=1)
            self.assertEqual(len(requests['requests']), 2)
            self.assertEqual(saved['calls'], [])
        for setting in [dict(timeout=0), dict(reconnects=-1), dict(retry_delay=float('nan')), dict(max_frames=0)]:
            with self.subTest(setting=setting), self.assertRaises(adapter.IngestionError):
                stream.consume('http://127.0.0.1/feed', 'http://127.0.0.1/api/v1', 'test.stream', UNIVERSE, **setting)

    def test_subscription_bounds_and_exact_key_coverage(self):
        value = dict(schema_version=1, curves=[dict(curve_id='CURVE-A', source='provider-neutral')], fixings=[])
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'subscription.json'
            path.write_text(json.dumps(value))
            self.assertEqual(stream.subscription(path), UNIVERSE)
            value['curves'] *= 2
            path.write_text(json.dumps(value))
            with self.assertRaises(adapter.IngestionError):
                stream.subscription(path)
            path.write_bytes(b'x' * (256 * 1024 + 1))
            with self.assertRaises(adapter.IngestionError):
                stream.subscription(path)


if __name__ == '__main__':
    unittest.main()
