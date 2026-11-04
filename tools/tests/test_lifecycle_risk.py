"""State/provenance admission controls; all financial values are opaque markers."""
import copy
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import book_updates
import lifecycle_risk as life
import streaming_analytics as analytics
from test_book_events import example
from test_market_data_stream import UNIVERSE
from test_streaming_analytics import AT, PortClient, config, frame


def inputs():
    _, batch = example()
    definition = batch['definition']
    data = definition['calculations']['pnl']['request']['operation']['input']
    data.update(reporting_currency='USD', max_points=16, max_product_evaluations=100)
    definition['calculations']['risk.0']['request']['operation']['input']['factors'] = [dict(opaque='factor')]
    return definition, batch['workflow']


def native(snapshot='snapshot'):
    """Opaque native envelopes: no formulas or manufactured financial answers."""
    position = dict(identity=dict(id='trade.A'), current=dict(quantity='native-current'), pending_quantity='native-pending')
    mark = dict(state=dict(remaining_fraction='native-fraction'), claims=['native-claim'],
                local=dict(current=dict(value='native-current-pv'), pending=dict(value='native-pending-pv')))
    state = dict(position=copy.deepcopy(position), snapshot_id=snapshot,
        manifest=dict(id='native-manifest', version=1), settlement='2026-10-17',
        current=[dict(snapshot_id=snapshot, base_pv='native-current-pv')],
        pending=[dict(snapshot_id=snapshot, base_pv='native-pending-pv')])
    result = dict(input=dict(snapshot_id=snapshot), basis='native-basis',
        vector=dict(id='trade.A', reference_position={'quantity': 'native-reference'},
                    sensitivities=[dict(snapshot_id=snapshot, base_pv='native-fresh-pv')]),
        lifecycle=state, lifecycle_mark=copy.deepcopy(mark))
    point = dict(positions=[position], manifest=state['manifest'], valuation_date=state['settlement'])
    report = dict(input=dict(causes=[], points=[point]), trades=[dict(id='trade.A', marks=[mark])])
    return result, report


class LifecycleClient(PortClient):
    def __init__(self):
        super().__init__()
        self.missing, self.wrong_claim = False, False

    def call(self, method, path, value=None):
        result = super().call(method, path, value)
        if method == 'GET':
            result['content']['definition']['definition']['definition']['calculations']['risk.0']['request']['operation']['kind'] = 'lifecycle_hedge_risk'
        return result

    def run(self, snapshot, calc):
        receipt, result = super().run(snapshot, calc)
        risk, report = native(snapshot)
        if calc == 'risk.0':
            result = risk
            if self.missing:
                del result['lifecycle']
        elif calc == 'pnl':
            if self.wrong_claim:
                report['trades'][0]['marks'][0]['claims'] = ['different-owner']
            result = dict(report=report)
        return receipt, result


class LifecycleBindingTests(unittest.TestCase):
    def test_history_preserves_order_standalone_and_native_quantities(self):
        definition, _ = inputs()
        data = definition['calculations']['pnl']['request']['operation']['input']
        original = copy.deepcopy(data)
        data['standalone'].append(dict(kind='unavailable', reason='actual prior state required'))
        history = life.history(data)
        self.assertEqual(history['causes'], [s['cause'] for s in data['steps']])
        self.assertEqual(history['points'][2], data['steps'][-1]['after']['point'])
        self.assertEqual(history['standalone'][-1], data['standalone'][-1])
        history['points'][0].clear()
        self.assertEqual(data['before'], original['before'])

    def test_bind_is_copy_and_requires_exact_complete_history_and_budget(self):
        definition, cfg = inputs()
        original = copy.deepcopy(definition)
        bound = life.bind_requests(definition, cfg, ['trade.A'])
        self.assertEqual(definition, original)
        operation = bound['calculations']['risk.0']['request']['operation']
        data = bound['calculations']['pnl']['request']['operation']['input']
        holding = data['holdings'][data['steps'][-1]['after']['holdings'][0]]
        life.check_binding(operation, holding, data)
        for mutation in [lambda v: v['history']['points'].pop(),
                         lambda v: v['lifecycle'].update(events=[{'changed': True}]),
                         lambda v: v.update(max_state_evaluations=4),
                         lambda v: v.update(max_state_evaluations=True),
                         lambda v: v.update(max_state_evaluations=81)]:
            with self.subTest(mutation=mutation):
                invalid = copy.deepcopy(operation)
                mutation(invalid)
                with self.assertRaises(ValueError):
                    life.check_binding(invalid, holding, data)

    def test_partial_termination_cannot_fall_back_to_fresh_product_risk(self):
        definition, cfg = inputs()
        data = definition['calculations']['pnl']['request']['operation']['input']
        data['holdings'][0]['lifecycle']['events'].append(dict(event=dict(action=dict(kind='terminate'))))
        with self.assertRaisesRegex(book_updates.BookInputError, 'lifecycle_risk_required'):
            book_updates.bind(cfg, definition, len(data['steps']) - 1)
        bound = life.bind_requests(definition, cfg, ['trade.A'])
        request = book_updates.bind(cfg, bound, len(data['steps']) - 1)
        self.assertEqual(request['operation']['problem']['holdings'][0]['current_quantity'], '7')

    def test_native_claim_state_amount_and_cutoff_must_all_match(self):
        result, report = native()
        self.assertEqual(life.checked_state(result, report), result['lifecycle'])
        changes = [lambda v: v['lifecycle']['position'].update(pending_quantity='changed'),
                   lambda v: v['lifecycle']['manifest'].update(version=2),
                   lambda v: v['lifecycle'].update(settlement='2026-10-18'),
                   lambda v: v['lifecycle'].update(snapshot_id='other'),
                   lambda v: v['lifecycle_mark'].update(claims=['different-owner']),
                   lambda v: v['lifecycle_mark']['state'].update(remaining_fraction='changed'),
                   lambda v: v['lifecycle_mark']['local']['current'].update(value='changed'),
                   lambda v: v['lifecycle']['current'][0].update(base_pv='wrong'),
                   lambda v: v['lifecycle']['pending'][0].update(snapshot_id='other'),
                   lambda v: v['lifecycle']['pending'].clear()]
        for change in changes:
            with self.subTest(change=change):
                wrong = copy.deepcopy(result)
                change(wrong)
                with self.assertRaises(ValueError):
                    life.checked_state(wrong, report)

    def worker(self, folder, client, events):
        return analytics.Workflow(config(), client, Path(folder) / 'receipt.json', UNIVERSE,
            clock=lambda: analytics.timestamp(AT), emit=events.append)

    def test_worker_passes_actual_states_and_preserves_fresh_candidate_vectors(self):
        with tempfile.TemporaryDirectory(prefix='convex-life-controls-') as folder:
            client, events = LifecycleClient(), []
            self.worker(folder, client, events).frame(frame())
            self.assertTrue(events[-1]['ready'])
            command = next(iter(client.commits.values()))
            request = command['master_changes'][0]['definition']['definition']['calculations']['hedge']['request']
            action = request['operation']['action']
            expected, _ = native(events[-1]['snapshot_id'])
            self.assertEqual(action, dict(kind='solve_lifecycle', states=[expected['lifecycle']]))
            candidate = request['operation']['problem']['candidates'][0]['risk']
            self.assertEqual(candidate['sensitivities'], expected['vector']['sensitivities'])

    def test_missing_native_state_blocks_then_exact_restart_recovers(self):
        with tempfile.TemporaryDirectory(prefix='convex-life-controls-') as folder:
            client, events = LifecycleClient(), []
            client.missing = True
            self.worker(folder, client, events).frame(frame())
            self.assertFalse(events[-1]['ready'])
            self.assertEqual(events[-1]['reason'], 'lifecycle_risk_result_missing')
            self.assertEqual(client.commits, {})
            client.missing = False
            self.worker(folder, client, events).frame(frame())
            self.assertTrue(events[-1]['ready'])
            self.assertTrue(events[-1]['receipts']['risk.0']['reused'])

    def test_wrong_claim_owner_blocks_before_hedge_publication(self):
        with tempfile.TemporaryDirectory(prefix='convex-life-controls-') as folder:
            client, events = LifecycleClient(), []
            client.wrong_claim = True
            self.worker(folder, client, events).frame(frame())
            self.assertFalse(events[-1]['ready'])
            self.assertEqual(client.commits, {})


if __name__ == '__main__':
    unittest.main()
