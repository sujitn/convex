"""Event admission/retry controls; no canned financial answers qualify finance."""
import copy
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import book_events as events
import market_data_adapter as adapter
import streaming_analytics as analytics
from test_book_updates import book_config, definition, record, release
from test_streaming_analytics import AT


def example():
    original = definition()
    original.update(effective={'from': AT, 'until': None}, reference_inputs=copy.deepcopy(analytics.EMPTY),
                    provenance=dict(source='engineering', evidence_reference='Opaque orchestration control'))
    data = original['calculations']['pnl']['request']['operation']['input']
    data['holdings'][0]['lifecycle'] = dict(events=[])
    data['markets'] = [dict(curves=[], calibration=None)]
    data['before'] = copy.deepcopy(data['steps'][0]['after'])
    data['before']['point']['fills'] = []
    data['steps'][0].update(cause='market')
    data['steps'][0]['after']['point']['fills'] = []
    data['standalone'] = [dict(kind='available', information=copy.deepcopy(data['steps'][0]['after']))]
    original = record(original, 2)['definition']['definition']
    value = copy.deepcopy(original)
    data = value['calculations']['pnl']['request']['operation']['input']
    close = copy.deepcopy(data['steps'][0]['after'])
    close['point']['positions'][0]['current']['quantity'] = '7'
    close['point']['fills'] = [dict(id='fill.1', trade_id='trade.A', quantity='2', source='recorder')]
    data['steps'].append(dict(cause='position', after=close))
    data['standalone'].append(dict(kind='available', information=copy.deepcopy(close)))
    return original, dict(schema_version=1, id='execution.1', base=release(), definition=value, workflow=book_config(3))


def native_report():
    state = dict(status='active', remaining_fraction='1', exercise_notices=[], delivery_pending=None, obligations=[])
    original, batch = example()
    data = original['calculations']['pnl']['request']['operation']['input']
    after = batch['definition']['calculations']['pnl']['request']['operation']['input']['steps'][-1]['after']['point']
    return dict(input=dict(causes=['market', 'position'], points=[data['before']['point'], data['steps'][0]['after']['point'], after]),
                trades=[dict(id='trade.A', marks=[dict(state=copy.deepcopy(state)) for _ in range(3)])])


class EventClient:
    def __init__(self):
        self.original, _ = example()
        self.commits, self.commands, self.runs = {}, {}, []
        self.head, self.ack_loss, self.fail_native = 2, False, False
        self.report = native_report()

    def call(self, method, path, value=None):
        if method == 'GET':
            key = path.rsplit('/', 1)[-1]
            if key == 'commit.2':
                r = dict(version=2, recorded_at=AT, definition=dict(kind='calculation', definition=self.original))
                return dict(idempotency_key=key, recorded_at=AT, master_records=[r])
            return copy.deepcopy(self.commits[key])
        key = value['idempotency_key']
        if key in self.commands and self.commands[key] != value:
            raise adapter.IngestionError('Conflict', http_status=409)
        if key not in self.commands:
            change = value['master_changes'][0]
            if change['expected_version'] != self.head:
                raise adapter.IngestionError('Conflict', http_status=409)
            self.commands[key] = copy.deepcopy(value)
            self.head += 1
            self.commits[key] = dict(idempotency_key=key, recorded_at=AT,
                master_records=[dict(version=self.head, recorded_at=AT, definition=copy.deepcopy(change['definition']))])
        if self.ack_loss:
            self.ack_loss = False
            raise adapter.IngestionError('Lost acknowledgement')
        return copy.deepcopy(self.commits[key])

    def assemble(self, plan):
        self.plan = copy.deepcopy(plan)
        return analytics.digest(plan)

    def run(self, snapshot, calc):
        self.runs.append((snapshot, calc))
        if self.fail_native:
            raise analytics.Blocked('native_calculation_failed')
        return dict(result_id='opaque'), dict(report=copy.deepcopy(self.report))


class EventTests(unittest.TestCase):
    def setUp(self):
        self.client = EventClient()
        _, self.batch = example()

    def test_native_checked_release_and_exact_idempotent_retry(self):
        first = events.ingest(self.client, self.batch)
        self.assertEqual(first, events.ingest(self.client, copy.deepcopy(self.batch)))
        self.assertEqual(first['payload']['workflow']['plan']['definition']['version'], 3)
        self.assertEqual(first['payload']['position_step'], 1)
        self.assertEqual(self.client.head, 3)
        self.assertEqual(len(self.client.commands), 1)
        self.assertEqual(self.client.runs[0], self.client.runs[1])

    def test_lost_reference_ack_recovers_without_new_event_or_revision(self):
        self.client.ack_loss = True
        with self.assertRaises(adapter.IngestionError):
            events.ingest(self.client, self.batch)
        self.assertEqual(self.client.head, 3)
        recovered = events.ingest(self.client, self.batch)
        self.assertEqual(recovered['payload']['version'], 2)
        self.assertEqual(self.client.head, 3)

    def test_crash_after_commit_before_release_recovers_from_durable_input(self):
        self.client.fail_native = True
        with self.assertRaises(analytics.Blocked):
            events.ingest(self.client, self.batch)
        self.client.fail_native = False
        self.assertEqual(events.ingest(self.client, self.batch)['payload']['version'], 2)
        self.assertEqual(self.client.head, 3)

    def test_same_batch_id_changed_fact_or_workflow_conflicts(self):
        events.ingest(self.client, self.batch)
        changed = copy.deepcopy(self.batch)
        changed['definition']['provenance']['evidence_reference'] += ' changed'
        with self.assertRaises(adapter.IngestionError) as caught:
            events.ingest(self.client, changed)
        self.assertEqual(caught.exception.http_status, 409)
        self.assertEqual(self.client.head, 3)

    def test_stale_competing_batch_cannot_drop_already_committed_facts(self):
        events.ingest(self.client, self.batch)
        self.batch['id'] = 'competing.batch'
        with self.assertRaises(adapter.IngestionError) as caught:
            events.ingest(self.client, self.batch)
        self.assertEqual(caught.exception.http_status, 409)

    def test_opening_prior_phase_or_counterfactual_cannot_change(self):
        changes = [lambda d: d['before']['point'].update(manifest={'id': 'changed', 'version': 1}),
                   lambda d: d['steps'][0].update(cause='event'),
                   lambda d: d['standalone'][0]['information']['point'].update(manifest={'id': 'changed', 'version': 1})]
        for change in changes:
            with self.subTest(change=change):
                batch = copy.deepcopy(self.batch)
                change(batch['definition']['calculations']['pnl']['request']['operation']['input'])
                with self.assertRaises(ValueError):
                    events.prepare(self.client, batch)
        self.assertEqual(self.client.commands, {})

    def test_opaque_holding_pool_reindexing_preserves_original_prefix(self):
        data = self.batch['definition']['calculations']['pnl']['request']['operation']['input']
        data['holdings'].insert(0, {'unused': 'native rejects unused separately'})
        for b in [data['before'], *[s['after'] for s in data['steps']], *[s['information'] for s in data['standalone']]]:
            b['holdings'] = [1]
        events.prepare(self.client, self.batch)  # Representation equality, not array-index equality.

    def test_revision_scope_configuration_and_reference_cannot_change(self):
        changes = [lambda b: b['workflow']['plan']['definition'].update(version=4),
                   lambda b: b['workflow'].update(id='other'),
                   lambda b: b['definition']['reference_inputs'].update(publications=[{'changed': True}]),
                   lambda b: b['definition'].update(id='other')]
        for change in changes:
            with self.subTest(change=change):
                value = copy.deepcopy(self.batch)
                change(value)
                with self.assertRaises(ValueError):
                    events.prepare(self.client, value)

    def test_new_identity_required_and_source_identity_immutable(self):
        data = self.batch['definition']['calculations']['pnl']['request']['operation']['input']
        data['steps'][-1]['after']['point']['fills'] = []
        with self.assertRaises(ValueError):
            events.prepare(self.client, self.batch)
        data['before']['point']['fills'] = [dict(id='same', trade_id='trade.A', quantity='1')]
        data['steps'][-1]['after']['point']['fills'] = [dict(id='same', trade_id='trade.A', quantity='2')]
        with self.assertRaises(ValueError):
            events.facts(data)

    def test_cash_receipt_identity_and_actual_lifecycle_payload_retained(self):
        data = self.batch['definition']['calculations']['pnl']['request']['operation']['input']
        step = dict(event=dict(event_id='cash.1', trade_id='trade.A', expected_revision=0,
            action=dict(kind='payment', terms=dict(receipt_id='receipt.1', amount={'value': '3'}))))
        data['holdings'][0]['lifecycle']['events'].append(step)
        found = events.facts(data)
        self.assertEqual(found[('lifecycle', 'trade.A', 'cash.1')], step)
        self.assertEqual(found[('receipt', 'trade.A', 'receipt.1')], step)

    def test_native_rejection_never_returns_publishable_book(self):
        self.client.fail_native = True
        with self.assertRaises(analytics.Blocked):
            events.ingest(self.client, self.batch)
        self.assertEqual(len(self.client.commands), 1)  # Invalid input remains auditable, unreleased.

    def test_unsupported_lifecycle_risk_never_releases_original_exposure(self):
        for field, value in [('status', 'terminated'), ('remaining_fraction', '0.5'),
                             ('exercise_notices', [[1]]), ('delivery_pending', ['pending']),
                             ('obligations', [dict(due_date='2026-10-18')])]:
            with self.subTest(field=field):
                self.client.report = native_report()
                self.client.report['trades'][0]['marks'][2]['state'][field] = value
                with self.assertRaises(ValueError):
                    events.ingest(self.client, self.batch)

    def test_bounds_and_unknown_keys_reject_before_writes(self):
        for field, value in [('schema_version', True), ('id', '../bad'), ('unknown', 1)]:
            with self.subTest(field=field):
                batch = copy.deepcopy(self.batch)
                batch[field] = value
                with self.assertRaises(ValueError):
                    events.prepare(self.client, batch)
        self.assertEqual(self.client.commands, {})

    def test_unchanged_claim_owner_ignores_only_native_point_metadata(self):
        report = native_report()
        close = copy.deepcopy(report['input']['points'][1])
        close['positions'][0]['identity']['economics'] = dict(id='point.new', version=1)
        close['positions'][0]['identity']['calculation'] = dict(id='calc.new', version=1)
        for mark in report['trades'][0]['marks']:
            mark['state']['obligations'] = [dict(due_date='2026-10-18')]
        events.risk_scope(report, close, 0)
        close['positions'][0]['current']['quantity'] = '8'
        with self.assertRaises(ValueError):
            events.risk_scope(report, close, 0)


if __name__ == '__main__':
    unittest.main()
