"""Book release/position binding engineering; native finance is exercised by demo."""
import copy
import json
from pathlib import Path
import sys
import tempfile
import unittest
import urllib.parse

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import book_updates
import market_data_adapter as adapter
import market_data_stream as stream
import streaming_analytics as analytics
from test_market_data_stream import UNIVERSE, feed, frame as wire_frame
from test_market_data_adapter import receiver
from test_streaming_analytics import AT, PortClient, config, frame, market

UNIVERSE_BOOK = (*UNIVERSE, 'desk.book')


def book_config(version=2):
    value = config()
    value['plan']['definition']['version'] = version
    value['hedge']['operation']['problem'].update(entity='desk', reporting_currency='USD')
    return value


def definition(quantity='5', pending='-2'):
    identity = dict(id='trade.A', entity='desk', instrument={'id': 'security', 'version': 1})
    position = dict(identity=identity, current=dict(quantity=quantity, reference_quantity='1', currency='USD'), pending_quantity=pending)
    reference = copy.deepcopy(position['current'])
    reference['quantity'] = reference['reference_quantity']
    product = {'opaque_native_product': True}
    return dict(id='book', calculations={
        'risk.0': dict(curve_sets=['current'], request=dict(operation=dict(kind='graph_hedge_risk', product=product,
            input=dict(identity=copy.deepcopy(identity), reference_position=reference)))),
        'pnl': dict(curve_sets=['current'], request=dict(operation=dict(kind='book_pnl', input=dict(
            reporting_currency='USD', holdings=[dict(id='trade.A', product=dict(kind='product', product=product))],
            steps=[dict(after=dict(market=0, holdings=[0], point=dict(positions=[position],
                manifest=dict(id='positions.' + quantity, version=1), valuation_date='2026-10-17',
                fx=[dict(from_='USD', to='USD')])))]))))})


def record(value, version):
    # Native serializer uses the actual `from` key.
    fx = value['calculations']['pnl']['request']['operation']['input']['steps'][0]['after']['point']['fx'][0]
    fx['from'] = fx.pop('from_')
    return dict(schema_version=2, version=version, recorded_at=AT,
                definition=dict(kind='calculation', definition=value))


def release(version=1, reference=2):
    return dict(id='desk.book', version=version, reference_commit='commit.' + str(reference),
                workflow=book_config(reference), position_step=0)


def book_frame(payload, kind='image'):
    value = frame(kind=kind)
    value['records'].append(dict(schema_version=1, kind='book', payload=payload))
    return value


class BookClient(PortClient):
    def __init__(self):
        super().__init__()
        self.records = {2: record(definition(), 2), 3: record(definition('13', '-10'), 3)}
        self.missing, self.wrong_snapshot = False, False

    def call(self, method, path, value=None):
        if path.startswith('/reference/commits/'):
            if self.missing:
                raise adapter.IngestionError('not committed', http_status=404)
            key = urllib.parse.unquote(path.rsplit('/', 1)[-1])
            version = int(key.rsplit('.', 1)[-1])
            return dict(idempotency_key=key, recorded_at=self.records[version]['recorded_at'], master_records=[copy.deepcopy(self.records[version])])
        if path.startswith('/analytics/snapshots/'):
            plan = self.saved[path.rsplit('/', 1)[-1]]
            r = copy.deepcopy(self.records[plan['definition']['version']])
            if self.wrong_snapshot:
                r['version'] += 1
            return dict(content=dict(definition=r, curves={'current': [dict(curve_id='CURVE-A')]}))
        return super().call(method, path, value)


class BookUpdateTests(unittest.TestCase):
    def setUp(self):
        self.folder = tempfile.TemporaryDirectory(prefix='convex-book-updates-')
        self.addCleanup(self.folder.cleanup)
        self.path = Path(self.folder.name) / 'receipt.json'
        self.now, self.events, self.client = analytics.timestamp(AT), [], None

    def book_worker(self):
        if not isinstance(self.client, BookClient):
            self.client = BookClient()
        return analytics.Workflow(book_config(), self.client, self.path, UNIVERSE_BOOK,
                                  clock=lambda: self.now, emit=self.events.append)

    def test_book_quantities_fx_and_manifest_come_from_exact_pnl_close(self):
        worker = self.book_worker()
        worker.frame(book_frame(release()))
        self.assertTrue(self.events[-1]['ready'])
        self.assertEqual(self.events[-1]['book']['definition']['version'], 2)
        command = next(iter(self.client.commits.values()))
        problem = command['master_changes'][0]['definition']['definition']['calculations']['hedge']['request']['operation']['problem']
        self.assertEqual(problem['holdings'][0]['current_quantity'], '5')
        self.assertEqual(problem['holdings'][0]['pending_quantity'], '-2')
        self.assertEqual(problem['position_state'], {'id': 'positions.5', 'version': 1})
        self.assertEqual(problem['holdings'][0]['risk']['fx']['from'], 'USD')
        state = analytics.load(self.path)
        self.assertEqual(state['schema_version'], 2)
        self.assertEqual(state['book']['version'], 1)

    def test_updated_positions_resume_after_result_ack_loss_and_process_restart(self):
        worker = self.book_worker()
        worker.frame(book_frame(release()))
        self.client.fail_after = 'risk.0'
        with self.assertRaises(stream.Recover):
            worker.frame(book_frame(release(2, 3), 'update'))
        self.assertEqual(analytics.load(self.path)['book']['version'], 2)
        restored = self.book_worker()
        self.assertFalse(restored.current)
        restored.frame(book_frame(release(2, 3)))
        self.assertTrue(self.events[-1]['ready'])
        self.assertTrue(self.events[-1]['receipts']['risk.0']['reused'])
        self.assertEqual(self.events[-1]['book']['position_state']['id'], 'positions.13')

    def test_bad_release_is_latched_and_market_update_cannot_reuse_old_book(self):
        worker = self.book_worker()
        worker.frame(book_frame(release()))
        bad = release(2, 3)
        bad['position_step'] = 1
        worker.frame(book_frame(bad, 'update'))
        self.assertFalse(self.events[-1]['ready'])
        calls = len(self.client.calls)
        worker.frame(frame(2, 'update'))
        self.assertFalse(self.events[-1]['ready'])
        self.assertEqual(len(self.client.calls), calls)
        self.assertEqual(analytics.load(self.path)['book']['version'], 1)

    def test_blocked_book_retains_new_confirmed_market_inputs_for_valid_release(self):
        worker = self.book_worker()
        worker.frame(book_frame(release()))
        bad = release(2, 3)
        bad['position_step'] = 1
        worker.frame(book_frame(bad, 'update'))
        worker.frame(frame(2, 'update'))
        self.assertFalse(self.events[-1]['ready'])
        valid = dict(kind='update', session='A', sequence=4,
            records=[dict(schema_version=1, kind='book', payload=release(3, 3))])
        worker.frame(valid)
        self.assertTrue(self.events[-1]['ready'])
        self.assertEqual(analytics.load(self.path)['plan']['curve_sets']['current'][0]['version'], 2)

    def test_rejected_pending_book_high_water_survives_restart(self):
        worker = self.book_worker()
        worker.frame(book_frame(release()))
        bad = release(2, 3)
        bad['position_step'] = 1
        worker.frame(book_frame(bad, 'update'))
        self.assertEqual(analytics.load(self.path)['book_head']['version'], 2)
        restored = self.book_worker()
        restored.frame(book_frame(release()))
        self.assertFalse(self.events[-1]['ready'])
        self.assertEqual(self.events[-1]['reason'], 'book_revision_rollback')
        restored.frame(book_frame(release(3, 3)))
        self.assertTrue(self.events[-1]['ready'])
        self.assertEqual(analytics.load(self.path)['book']['version'], 3)

    def test_future_missing_wrong_or_uncommitted_references_do_not_run(self):
        for mode in ['future', 'missing', 'wrong_version']:
            with self.subTest(mode=mode):
                self.client = BookClient()
                worker = self.book_worker()
                if mode == 'future':
                    self.client.records[2]['recorded_at'] = '2026-10-18T00:00:00Z'
                elif mode == 'missing':
                    self.client.missing = True
                else:
                    self.client.records[2]['version'] = 99
                worker.frame(book_frame(release()))
                self.assertFalse(self.events[-1]['ready'])
                self.assertEqual(self.client.calls, [])

    def test_snapshot_cannot_substitute_a_different_reference_version(self):
        worker = self.book_worker()
        self.client.wrong_snapshot = True
        worker.frame(book_frame(release()))
        self.assertFalse(self.events[-1]['ready'])
        self.assertEqual(self.events[-1]['reason'], 'book_snapshot_reference_mismatch')
        self.assertFalse(any(c[0] == 'run' for c in self.client.calls))

    def test_risk_product_reference_units_and_market_must_match_positions(self):
        changes = [lambda d: d['calculations']['risk.0']['request']['operation'].update(product={}),
                   lambda d: d['calculations']['risk.0']['request']['operation']['input']['identity'].update(instrument={'id': 'other', 'version': 1}),
                   lambda d: d['calculations']['risk.0']['request']['operation']['input']['reference_position'].update(quantity='99'),
                   lambda d: d['calculations']['risk.0'].update(curve_sets=['other'])]
        for change in changes:
            with self.subTest(change=change):
                value = definition()
                change(value)
                with self.assertRaises(ValueError):
                    book_updates.bind(book_config(), value, 0)

    def test_same_revision_conflict_and_rollbacks_never_calculate(self):
        worker = self.book_worker()
        worker.frame(book_frame(release(2, 3)))
        saved = analytics.load(self.path)
        with self.assertRaises(stream.Recover):
            worker.frame(book_frame(release(), 'update'))
        worker = self.book_worker()
        worker.frame(book_frame(release()))
        self.assertFalse(self.events[-1]['ready'])
        self.assertEqual(analytics.load(self.path), saved)
        changed = release(2, 3)
        changed['workflow']['hedge']['operation']['problem']['extra'] = 'conflict'
        with self.assertRaises(adapter.IngestionError):
            worker.frame(book_frame(changed, 'update'))

    def test_gap_full_image_and_wire_coverage_include_exactly_one_book(self):
        envelope = dict(schema_version=1, kind='book', payload=release())
        image = wire_frame(records=[market(), envelope])
        parsed, messages, _ = stream.decode(json.dumps(image).encode() + b'\n', 'test.stream', UNIVERSE_BOOK)
        self.assertEqual(len(messages), 1)  # Book is a read/activation barrier, not a market write.
        self.assertEqual(parsed['records'][1], envelope)
        for records in [[market()], [market(), envelope, envelope], [market(), dict(envelope, payload=dict(release(), version=True))]]:
            with self.subTest(records=records), self.assertRaises(adapter.IngestionError):
                stream.decode(json.dumps(wire_frame(records=records)).encode() + b'\n', 'test.stream', UNIVERSE_BOOK)
        with self.assertRaises(adapter.IngestionError):
            stream.decode(json.dumps(image).encode() + b'\n', 'test.stream', UNIVERSE)

    def test_book_gaps_are_discarded_before_activation_callback(self):
        image = wire_frame(records=[market(), dict(schema_version=1, kind='book', payload=release())])
        skipped = wire_frame(3, kind='update', records=[dict(schema_version=1, kind='book', payload=release(99, 3))])
        observed = []
        with receiver() as (sink, saved), feed([[image, skipped], [dict(image, session='B')]]) as (source, _):
            stream.consume(source, sink, 'test.stream', UNIVERSE_BOOK, retry_delay=.01,
                timeout=1, reconnects=1, max_frames=2, emit=lambda _: None, on_frame=observed.append)
            self.assertEqual(len(saved['calls']), 2)
        self.assertEqual([r['records'][1]['payload']['version'] for r in observed], [1, 1])

    def test_scope_definition_rollback_and_corrupt_restored_book_are_rejected(self):
        worker = self.book_worker()
        worker.frame(book_frame(release(2, 3)))
        bad = release(3, 2)
        worker.frame(book_frame(bad, 'update'))
        self.assertFalse(self.events[-1]['ready'])
        self.assertEqual(self.events[-1]['reason'], 'book_reference_rollback')
        wrong_scope = release(4, 3)
        wrong_scope['workflow']['id'] = 'other.book'
        worker.frame(book_frame(wrong_scope, 'update'))
        self.assertFalse(self.events[-1]['ready'])
        prior = analytics.load(self.path)
        prior['book']['id'] = 'another.subscription'
        analytics.save(self.path, prior)
        with self.assertRaises(ValueError):
            self.book_worker()

    def test_book_change_retains_admitted_market_revision_floor(self):
        worker = self.book_worker()
        initial = book_frame(release())
        initial['records'][0] = market(3)
        worker.frame(initial)
        self.assertTrue(self.events[-1]['ready'])
        with self.assertRaises(stream.Recover):
            worker.frame(book_frame(release(2, 3), 'update'))
        prior = analytics.load(self.path)
        self.assertEqual(prior['book']['version'], 1)
        self.assertEqual(prior['plan']['curve_sets']['current'][0]['version'], 3)
        image = book_frame(release(2, 3))
        image['records'][0] = market(3)
        worker.frame(image)
        self.assertTrue(self.events[-1]['ready'])
        self.assertEqual(analytics.load(self.path)['plan']['curve_sets']['current'][0]['version'], 3)

    def test_book_subscription_and_missing_fx_require_explicit_inputs(self):
        path = self.path.parent / 'subscription.json'
        path.write_text(json.dumps(dict(schema_version=1, curves=[dict(curve_id='CURVE-A', source='provider-neutral')], fixings=[], book='desk.book')))
        self.assertEqual(stream.subscription(path), UNIVERSE_BOOK)
        value = record(definition(), 2)['definition']['definition']
        value['calculations']['pnl']['request']['operation']['input']['steps'][0]['after']['point']['fx'] = []
        with self.assertRaises(ValueError):
            book_updates.bind(book_config(), value, 0)


if __name__ == '__main__':
    unittest.main()
