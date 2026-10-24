"""Engineering orchestration controls; native finance stays separately pinned."""
import copy
from datetime import datetime, timedelta, timezone
import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import market_data_adapter as adapter
import market_data_stream as stream
import streaming_analytics as analytics
from test_market_data_adapter import curve, receiver
from test_market_data_stream import feed, frame as wire_frame, UNIVERSE

AT = '2026-10-17T00:00:01Z'


def config():
    return dict(schema_version=1, id='test.workflow',
        plan=dict(definition=dict(id='book', version=1), as_of=AT, known_at=AT,
            curve_sets={'current': [dict(id='CURVE-A', version=1)]}, fixings=[]),
        bindings=[dict(set='current', curve_id='CURVE-A', source='provider-neutral',
                       follow=True, max_age_ms=5000, recipe_key=dict(id='native.A', version='1'))],
        risks={'trade.A': 'risk.0'}, pnl='pnl',
        hedge=dict(schema_version=1, inputs=copy.deepcopy(analytics.EMPTY), operation=dict(kind='hedge',
            problem=dict(snapshot_id='placeholder', expected_holdings=['trade.A'],
                holdings=[dict(risk='trade.A', fx=dict(snapshot_id='placeholder'))],
                candidates=[dict(risk='trade.A', fx=dict(snapshot_id='placeholder'))]), action=dict(kind='solve'))))


def market(version=1):
    value = curve()
    value['payload'].update(version=version, max_age_ms=10000, recipe=dict(key=dict(id='native.A', version='1')))
    return value


def frame(version=1, kind='image'):
    return dict(kind=kind, sequence=version, session='A', records=[market(version)])


class PortClient:
    """Opaque canned port envelopes; deliberately contains no financial math."""
    def __init__(self):
        self.calls, self.results, self.saved, self.commits = [], {}, {}, {}
        self.fail_after = None
        self.after_run = None
        self.historical = False

    def assemble(self, plan):
        self.calls.append(('assemble', copy.deepcopy(plan)))
        identity = analytics.digest(plan)
        self.saved[identity] = copy.deepcopy(plan)
        return identity

    def call(self, method, path, value=None):
        if path == '/reference/transactions':
            key = value['idempotency_key']
            if key in self.commits:
                if self.commits[key] != value:
                    raise AssertionError('Non-idempotent reference command')
            self.commits[key] = copy.deepcopy(value)
            return dict(recorded_at=AT)
        plan = self.saved[path.rsplit('/', 1)[-1]]
        sets = {'historical' if self.historical else 'current': [dict(curve_id='CURVE-A')]}
        return dict(content=dict(curves=sets, definition=dict(definition=dict(definition=dict(calculations={
            'risk.0': dict(curve_sets=list(sets), request=dict(operation=dict(kind='graph_hedge_risk'))),
            'pnl': dict(curve_sets=[], request=dict(operation=dict(kind='book_pnl')))}))), plan=plan))

    def run(self, snapshot, calc):
        self.calls.append(('run', snapshot, calc))
        key = (snapshot, calc)
        reused = key in self.results
        if calc.startswith('risk.'):
            data = dict(basis='opaque native basis', input=dict(snapshot_id=snapshot), vector=dict(id='trade.A',
                reference_position={'quantity': '1'}, sensitivities=[dict(snapshot_id=snapshot, base_pv='opaque native price')]))
        elif calc == 'hedge':
            data = dict(status='feasible')
        else:
            data = dict(report='opaque native causal report')
        self.results[key] = data
        if self.after_run:
            self.after_run(calc)
        if self.fail_after == calc:
            self.fail_after = None
            raise adapter.IngestionError('Lost native result acknowledgement')
        return dict(snapshot_id=snapshot, calculation=calc, result_id=analytics.digest(key),
                    reused=reused, build_id='same.native.build'), copy.deepcopy(data)


class StreamingAnalyticsTests(unittest.TestCase):
    def setUp(self):
        self.folder = tempfile.TemporaryDirectory(prefix='convex-stream-analytics-')
        self.addCleanup(self.folder.cleanup)
        self.path = Path(self.folder.name) / 'receipt.json'
        self.now = analytics.timestamp(AT)
        self.events, self.client = [], PortClient()

    def worker(self, value=None, universe=UNIVERSE):
        return analytics.Workflow(value or config(), self.client, self.path, universe,
            clock=lambda: self.now, emit=self.events.append)

    def test_confirmed_frame_runs_all_stages_and_retains_only_native_values(self):
        worker = self.worker()
        self.assertFalse(self.events[-1]['ready'])
        worker.frame(frame())
        ready = self.events[-1]
        self.assertTrue(ready['ready'])
        self.assertEqual(set(ready['receipts']), {'risk.0', 'pnl', 'hedge'})
        self.assertEqual(ready['prices'][0]['reference_measure'], 'opaque native price')
        command = next(iter(self.client.commits.values()))
        request = command['master_changes'][0]['definition']['definition']['calculations']['hedge']['request']
        problem = request['operation']['problem']
        self.assertEqual(problem['snapshot_id'], ready['snapshot_id'])
        for binding in [*problem['holdings'], *problem['candidates']]:
            self.assertTrue(all(s['snapshot_id'] == ready['snapshot_id'] for s in binding['risk']['sensitivities']))
            self.assertEqual(binding['risk']['fx']['snapshot_id'], ready['snapshot_id'])
        self.assertEqual(analytics.load(self.path)['bundle']['snapshot_id'], ready['snapshot_id'])

    def test_stale_future_quality_and_native_rebind_do_not_start_calculation(self):
        invalid = []
        stale = frame()
        stale['records'][0]['payload']['source_time'] = '2026-10-16T00:00:01Z'
        invalid.append(stale)
        future = frame()
        future['records'][0]['payload']['recorded_at'] = '2026-10-18T00:00:01Z'
        invalid.append(future)
        bad = frame()
        bad['records'][0]['payload']['quality'] = 'Stale'
        invalid.append(bad)
        changed_key = frame()
        changed_key['records'][0]['payload']['recipe']['key']['version'] = '2'
        invalid.append(changed_key)
        for value in invalid:
            with self.subTest(value=value):
                self.worker().frame(value)
                self.assertFalse(self.events[-1]['ready'])
                self.assertEqual(self.client.calls, [])
                self.assertFalse(self.path.exists())

    def test_default_live_clock_accepts_current_data_and_emits_bounded_expiry(self):
        value = frame()
        now = datetime.now(timezone.utc)
        value['records'][0]['payload'].update(source_time=analytics.iso(now), recorded_at=analytics.iso(now))
        worker = analytics.Workflow(config(), self.client, self.path, UNIVERSE, emit=self.events.append)
        worker.frame(value)
        self.assertTrue(self.events[-1]['ready'])
        self.assertEqual(self.events[-1]['mode'], 'live')
        self.assertLessEqual(analytics.timestamp(self.events[-1]['valid_until']), now + timedelta(seconds=5))

    def test_future_plan_cutoff_cannot_admit_information_not_yet_available(self):
        value = config()
        value['plan']['known_at'] = '2026-10-18T00:00:01Z'
        self.worker(value).frame(frame())
        self.assertFalse(self.events[-1]['ready'])
        self.assertEqual(self.events[-1]['reason'], 'future_snapshot_cutoff')
        self.assertEqual(self.client.calls, [])

    def test_heartbeat_duplicate_and_clock_regression_cannot_refresh_market_age(self):
        for state in ['heartbeat', 'duplicate']:
            with self.subTest(state=state):
                self.path.unlink(missing_ok=True)
                self.now = analytics.timestamp(AT)
                worker = self.worker()
                worker.frame(frame())
                calls = copy.deepcopy(self.client.calls)
                self.now += timedelta(seconds=4)
                worker.status(dict(state=state))
                self.assertFalse(worker.current)  # source is one second older than receipt
                self.assertFalse(self.events[-1]['ready'])
                self.assertEqual(self.client.calls, calls)
        self.now = analytics.timestamp(AT)
        self.path.unlink()
        worker = self.worker()
        worker.frame(frame())
        self.now -= timedelta(microseconds=1)
        worker.status(dict(state='heartbeat'))
        self.assertFalse(worker.current)

    def test_expiry_during_native_calculation_suppresses_completion_and_hedge(self):
        worker = self.worker()
        self.client.after_run = lambda _: setattr(self, 'now', self.now + timedelta(seconds=5))
        worker.frame(frame())
        self.assertFalse(self.events[-1]['ready'])
        self.assertIsNone(analytics.load(self.path)['bundle'])
        self.assertEqual(len([c for c in self.client.calls if c[0] == 'run']), 1)
        self.assertEqual(self.client.commits, {})

    def test_unconfirmed_result_recovers_cached_prefix_after_fresh_image(self):
        worker = self.worker()
        self.client.fail_after = 'risk.0'
        with self.assertRaises(stream.Recover):
            worker.frame(frame())
        self.assertFalse(self.events[-1]['ready'])
        self.assertIsNone(analytics.load(self.path)['bundle'])
        restored = self.worker()
        self.assertFalse(restored.current)
        self.assertEqual(restored.records, {})
        restored.frame(frame())
        self.assertTrue(self.events[-1]['ready'])
        self.assertTrue(self.events[-1]['receipts']['risk.0']['reused'])
        self.assertFalse(self.events[-1]['receipts']['pnl']['reused'])

    def test_gap_stopped_and_reconnect_never_restore_a_previous_ready_flag(self):
        worker = self.worker()
        worker.frame(frame())
        first = self.events[-1]['snapshot_id']
        for state in ['recovering', 'awaiting_image', 'stopped']:
            worker.status(dict(state=state, reason='sequence_gap'))
            self.assertFalse(worker.current)
            self.assertFalse(self.events[-1]['ready'])
            self.assertEqual(worker.records, {})
        restored = self.worker()
        self.assertFalse(restored.current)
        restored.frame(frame())
        self.assertEqual(self.events[-1]['snapshot_id'], first)
        self.assertTrue(all(r['reused'] for r in self.events[-1]['receipts'].values()))
        self.assertEqual(len(self.client.commits), 1)

    def test_restart_refuses_old_image_below_persisted_source_version(self):
        self.worker().frame(frame(3))
        restored = self.worker()
        restored.frame(frame(2))
        self.assertFalse(self.events[-1]['ready'])
        self.assertEqual(analytics.load(self.path)['plan']['curve_sets']['current'][0]['version'], 3)

    def test_in_session_source_revision_rollback_requires_new_image(self):
        worker = self.worker()
        worker.frame(frame(3))
        with self.assertRaises(stream.Recover):
            worker.frame(frame(2, 'update'))
        self.assertFalse(worker.current)

    def test_future_fixing_and_native_failure_do_not_reach_hedge(self):
        original = self.client.run
        def failed(snapshot, calc):
            if calc == 'pnl':
                raise analytics.Blocked('native_calculation_failed')
            return original(snapshot, calc)
        self.client.run = failed
        self.worker().frame(frame())
        self.assertEqual(self.events[-1]['reason'], 'native_calculation_failed')
        self.assertFalse(self.client.commits)
        self.path.unlink()
        value = frame()
        value['records'].append(dict(kind='fixings', payload=[dict(published_at='2026-10-18T00:00:00Z', recorded_at=AT)]))
        self.worker().frame(value)
        self.assertEqual(self.events[-1]['reason'], 'future_fixing')

    def test_analytics_http_preserves_permissions_and_never_follows_redirect(self):
        with receiver('deny') as (sink, state):
            client = analytics.Client(sink, token='analytics-secret')
            with self.assertRaises(adapter.IngestionError) as error:
                client.call('POST', '/reference/transactions', {'idempotency_key': 'safe'})
            self.assertEqual(error.exception.http_status, 403)
            self.assertEqual(state['calls'][0][2], 'Bearer analytics-secret')
            self.assertNotIn('analytics-secret', json.dumps(error.exception.diagnostic()))
            self.assertNotIn('private', str(error.exception))
        with receiver() as (target, saved), receiver(target) as (sink, _):
            with self.assertRaises(adapter.IngestionError) as error:
                analytics.Client(sink, token='analytics-secret').call('POST', '/analytics/snapshots', {})
            self.assertEqual(error.exception.http_status, 307)
            self.assertEqual(saved['calls'], [])

    def test_historical_curves_cannot_be_used_for_current_risk(self):
        self.client.historical = True
        self.worker().frame(frame())
        self.assertFalse(self.events[-1]['ready'])
        self.assertFalse(any(c[0] == 'run' for c in self.client.calls))

    def test_wrong_native_risk_identity_does_not_reach_hedge(self):
        original = self.client.run
        def run(snapshot, calc):
            receipt, data = original(snapshot, calc)
            data['vector']['id'] = 'other.trade'
            return receipt, data
        self.client.run = run
        self.worker().frame(frame())
        self.assertFalse(self.events[-1]['ready'])
        self.assertFalse(self.client.commits)

    def test_receipt_mismatch_corruption_and_duplicate_worker_fail_closed(self):
        self.worker().frame(frame())
        changed = config()
        changed['id'] = 'other.workflow'
        with self.assertRaises(ValueError):
            self.worker(changed)
        self.path.write_text('{"schema_version":1,"schema_version":1}')
        with self.assertRaises(ValueError):
            self.worker()
        with analytics.owner(self.path):
            with self.assertRaises(adapter.IngestionError):
                with analytics.owner(self.path):
                    self.fail('Concurrent receipt writer admitted')

    def test_native_failure_or_build_change_cannot_complete_bundle(self):
        original = self.client.run
        def changed(snapshot, calc):
            receipt, data = original(snapshot, calc)
            if calc == 'hedge':
                receipt['build_id'] = 'other.build'
            return receipt, data
        self.client.run = changed
        self.worker().frame(frame())
        self.assertFalse(self.events[-1]['ready'])
        self.assertIsNone(analytics.load(self.path)['bundle'])

    def test_confirmed_hook_never_sees_gapped_or_unconfirmed_frames(self):
        observed, events = [], []
        image = wire_frame(records=[market()])
        skipped = wire_frame(3, kind='update', records=[market(99)])
        with receiver('lost_ack') as (sink, state), feed([[image], [image, skipped], [image]]) as (source, _):
            stream.consume(source, sink, 'test.stream', UNIVERSE, timeout=1, retry_delay=.01,
                reconnects=2, max_frames=2, emit=events.append, on_frame=observed.append)
            self.assertNotIn(('CURVE-A', 99), state['saved'])
        self.assertEqual([f['sequence'] for f in observed], [1, 1])
        self.assertTrue(any(e.get('reason') == 'publication_unconfirmed' for e in events))
        self.assertTrue(any(e.get('reason') == 'sequence_gap' for e in events))

    def test_config_requires_exact_subscriptions_and_explicit_hedge_vectors(self):
        for mutate in [lambda c: c['bindings'][0].update(follow=False),
                       lambda c: c['bindings'][0].update(max_age_ms=0),
                       lambda c: c['hedge']['operation']['problem']['holdings'][0].update(risk='missing.trade'),
                       lambda c: c['risks'].update({'second.trade': 'risk.0'})]:
            value = config()
            mutate(value)
            with self.assertRaises(ValueError):
                analytics.validate(value, UNIVERSE)
        self.path.write_bytes(b'x' * (analytics.MAX_JSON + 1))
        with self.assertRaises(ValueError):
            analytics.load(self.path)


if __name__ == '__main__':
    unittest.main()
