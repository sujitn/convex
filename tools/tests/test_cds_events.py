"""Opaque engineering controls for default blocking, scope and safe resumption."""
import copy
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import book_events as events
import core_cds_workflow as cds
import lifecycle_risk as life
import streaming_analytics as analytics
from test_book_events import EventClient, example
from test_lifecycle_risk import LifecycleClient, native
from test_market_data_stream import UNIVERSE
from test_streaming_analytics import AT, config, frame


def pending(snapshot='snapshot'):
    result, _ = native(snapshot)
    result.pop('lifecycle_mark')
    state = result.pop('lifecycle')
    position = state['position']
    position['pending_quantity'] = '0'
    point = dict(positions=[copy.deepcopy(position)], manifest=state['manifest'], valuation_date=state['settlement'])
    view = dict(trade_id='trade.A', status='default_pending', default_event='default.1', recovery=None, remaining_fraction='0')
    result['lifecycle_unpriced'] = dict(position=position, manifest=state['manifest'], settlement=state['settlement'],
        snapshot_id=snapshot, state=view, reason='actual_cds_recovery_unknown', default_entity='ACME')
    operation = dict(kind='lifecycle_hedge_risk', input=dict(identity=dict(id='trade.A')),
        product=dict(kind='cds', terms=dict(reference=dict(entity_id='ACME'))),
        lifecycle=dict(events=[dict(event=dict(event_id='default.1', action=dict(kind='credit_default')))]))
    data = dict(steps=[dict(after=dict(point=point))])
    return result, operation, data


class PendingClient(LifecycleClient):
    def call(self, method, path, value=None):
        response = super().call(method, path, value)
        if method == 'GET':
            _, operation, data = pending()
            definition = response['content']['definition']['definition']['definition']
            definition['calculations']['risk.0']['request']['operation'] = operation
            definition['calculations']['pnl']['request']['operation']['input'] = data
        return response

    def run(self, snapshot, calc):
        receipt, value = super().run(snapshot, calc)
        if calc == 'risk.0':
            value = pending(snapshot)[0]
        return receipt, value


def resolution():
    original, batch = example()
    data = original['calculations']['pnl']['request']['operation']['input']
    data.update(max_points=16, max_product_evaluations=100)
    original['calculations']['risk.0']['request']['operation']['input']['factors'] = [{}]
    cancel = copy.deepcopy(data['steps'][-1]['after'])
    cancel['point']['positions'][0]['pending_quantity'] = '0'
    data['steps'].append(dict(cause='position', after=cancel))
    data['standalone'].append(dict(kind='unavailable', reason='opaque control'))
    default = dict(event=dict(event_id='default.1', trade_id='trade.A', expected_revision=0,
                             action=dict(kind='credit_default', terms=dict(opaque='actual-default'))))
    holding = copy.deepcopy(data['holdings'][0])
    holding['lifecycle']['events'].append(default)
    data['holdings'].append(holding)
    after = copy.deepcopy(cancel)
    after['holdings'] = [1]
    data['steps'].append(dict(cause='event', after=after))
    data['standalone'].append(dict(kind='unavailable', reason='opaque unpriced control'))
    # This branch's native audit is stubbed explicitly; no financial conclusion.
    original = life.bind_requests(original, batch['base']['workflow'], ['trade.A'])
    batch['base']['position_step'] = 2
    proposed = copy.deepcopy(original)
    new = proposed['calculations']['pnl']['request']['operation']['input']
    recovered = copy.deepcopy(new['holdings'][1])
    recovered['lifecycle']['events'].append(dict(event=dict(event_id='recovery.1', trade_id='trade.A', expected_revision=1,
        action=dict(kind='recovery', terms=dict(default_event='default.1', opaque='actual-recovery')))))
    new['holdings'].append(recovered)
    new['steps'][-1]['after']['holdings'] = [2]
    batch.update(schema_version=2, resolve_default=True,
                 definition=life.bind_requests(proposed, batch['workflow'], ['trade.A']))
    client = EventClient()
    client.original = original
    return client, batch


class CdsEventTests(unittest.TestCase):
    def test_unknown_recovery_has_exact_state_and_no_actual_prices(self):
        result, operation, data = pending()
        self.assertEqual(life.checked_pending(result, operation, data), result['lifecycle_unpriced'])
        self.assertNotIn('lifecycle', result)
        self.assertNotIn('lifecycle_mark', result)

    def test_pending_scope_position_snapshot_and_default_identity_cannot_change(self):
        result, operation, data = pending()
        changes = [lambda v: v['lifecycle_unpriced'].update(snapshot_id='other'),
                   lambda v: v['lifecycle_unpriced'].update(default_entity='other'),
                   lambda v: v['lifecycle_unpriced']['position'].update(pending_quantity='1'),
                   lambda v: v['lifecycle_unpriced']['state'].update(default_event='other'),
                   lambda v: v['lifecycle_unpriced']['state'].update(recovery={'fabricated': True}),
                   lambda v: v.update(lifecycle={'fabricated': True})]
        for change in changes:
            wrong = copy.deepcopy(result)
            change(wrong)
            with self.assertRaises(ValueError):
                life.checked_pending(wrong, operation, data)

    def test_unpriced_default_survives_worker_restart_without_a_hedge(self):
        with tempfile.TemporaryDirectory(prefix='convex-default-controls-') as folder:
            client, output = PendingClient(), []
            for _ in range(2):
                worker = analytics.Workflow(config(), client, Path(folder)/'state.json', UNIVERSE,
                    clock=lambda: analytics.timestamp(AT), emit=output.append)
                worker.frame(frame())
                self.assertFalse(output[-1]['ready'])
                self.assertEqual(output[-1]['reason'], 'cds_recovery_pending')
                self.assertEqual(client.commits, {})

    def test_resolution_requires_verified_unpriced_default(self):
        client, batch = resolution()
        with patch.object(events, 'pending_defaults', return_value=[]):
            with self.assertRaisesRegex(ValueError, 'requires_unpriced_base'):
                events.prepare(client, batch)

    def test_resolution_keeps_older_phases_and_every_actual_default_fact(self):
        client, batch = resolution()
        proof = [dict(state=dict(default_event='default.1'))]
        with patch.object(events, 'pending_defaults', return_value=proof):
            events.prepare(client, batch)
            for mode in ['older_phase', 'default_fact', 'recovery_link']:
                wrong = copy.deepcopy(batch)
                data = wrong['definition']['calculations']['pnl']['request']['operation']['input']
                holding = data['holdings'][data['steps'][-1]['after']['holdings'][0]]
                if mode == 'older_phase':
                    data['steps'][0]['cause'] = 'event'
                elif mode == 'default_fact':
                    holding['lifecycle']['events'][0]['event']['action']['terms']['opaque'] = 'changed'
                else:
                    holding['lifecycle']['events'][1]['event']['action']['terms']['default_event'] = 'other'
                with self.assertRaises(ValueError):
                    events.prepare(client, wrong)

    def test_schema_two_requires_explicit_resolution_and_rejects_extra_fields(self):
        client, batch = resolution()
        for value in [False, 1, None]:
            wrong = copy.deepcopy(batch)
            wrong['resolve_default'] = value
            with self.assertRaisesRegex(ValueError, 'event_batch_schema'):
                events.prepare(client, wrong)
        batch['unknown'] = True
        with self.assertRaisesRegex(ValueError, 'event_batch_schema'):
            events.prepare(client, batch)

    def test_worker_excludes_native_default_entity_from_fresh_candidates(self):
        with tempfile.TemporaryDirectory(prefix='convex-default-controls-') as folder:
            worker = analytics.Workflow(config(), PendingClient(), Path(folder)/'state.json', UNIVERSE,
                clock=lambda: analytics.timestamp(AT), emit=lambda _: None)
            vector = native()[0]['vector']
            vector['credit_scope'] = [dict(entity_id='ACME')]
            request = worker.hedge('snapshot', {'trade.A': vector}, [dict(default_entity='ACME')])
            self.assertEqual(request['operation']['problem']['candidates'], [])
            self.assertEqual(request['operation']['action']['kind'], 'solve_lifecycle')

    def test_native_marking_roundoff_remains_visible_with_exact_cash_balance(self):
        def money(value):
            return dict(value=value)
        attribution = dict(residual=dict(cash=money(0), current_model_pnl=money(0),
            marking=dict(current=money(-5.551115123125783e-17))),
            total=dict(marking=dict(current=money(0.3))),
            ordered=[dict(marking=dict(current=money(0.1))), dict(marking=dict(current=money(0.2)))])
        cds.check_attribution(attribution)
        masked = copy.deepcopy(attribution)
        masked['residual']['marking']['current']['value'] = 0
        with self.assertRaises(RuntimeError):
            cds.check_attribution(masked)
        wrong_cash = copy.deepcopy(attribution)
        wrong_cash['residual']['cash']['value'] = 0.01
        with self.assertRaises(RuntimeError):
            cds.check_attribution(wrong_cash)

    def test_real_marking_imbalance_cannot_pass_the_roundoff_budget(self):
        attribution = dict(residual=dict(cash=dict(value=0), current_model_pnl=dict(value=0),
            marking=dict(current=dict(value=0.3 - (0.1 + 0.21)))),
            total=dict(marking=dict(current=dict(value=0.3))),
            ordered=[dict(marking=dict(current=dict(value=0.1))), dict(marking=dict(current=dict(value=0.21)))])
        with self.assertRaises(RuntimeError):
            cds.check_attribution(attribution)


if __name__ == '__main__':
    unittest.main()
