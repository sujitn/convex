"""Opaque binding/admission controls; native owners supply all financial values."""
import copy
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import lifecycle_risk as life
import streaming_analytics as analytics
from test_cds_events import pending as cds_pending, PendingClient
from test_lifecycle_risk import native
from test_market_data_stream import UNIVERSE
from test_streaming_analytics import AT, config, frame


def pending(snapshot='snapshot'):
    result, operation, data = cds_pending(snapshot)
    operation['product'] = dict(kind='bond')
    operation['lifecycle']['instrument'] = dict(kind='bond', credit=dict(entity='BOND.ISSUER'))
    result['lifecycle_unpriced'].update(default_entity='BOND.ISSUER', reason='actual_bond_recovery_unknown')
    return result, operation, data


class BondClient(PendingClient):
    def call(self, method, path, value=None):
        response = super().call(method, path, value)
        if method == 'GET':
            _, operation, data = pending()
            definition = response['content']['definition']['definition']['definition']
            definition['calculations']['risk.0']['request']['operation'] = operation
            definition['calculations']['pnl']['request']['operation']['input'] = data
        return response

    def run(self, snapshot, calc):
        receipt, result = super().run(snapshot, calc)
        return receipt, pending(snapshot)[0] if calc == 'risk.0' else result


class BondEventTests(unittest.TestCase):
    def test_unknown_recovery_binds_explicit_bond_mapping_and_no_mark(self):
        result, operation, data = pending()
        self.assertEqual(life.checked_pending(result, operation, data), result['lifecycle_unpriced'])
        for mutate in [lambda v: v['lifecycle']['instrument'].update(credit=None),
                       lambda v: v['lifecycle']['instrument'].update(kind='zero'),
                       lambda v: v['lifecycle']['instrument']['credit'].update(entity='OTHER')]:
            wrong = copy.deepcopy(operation)
            mutate(wrong)
            with self.assertRaises(ValueError):
                life.checked_pending(result, wrong, data)
        result['lifecycle_mark'] = {'invented': 'price'}
        with self.assertRaises(ValueError):
            life.checked_pending(result, operation, data)

    def test_pending_reason_and_original_native_default_identity_are_bound(self):
        result, operation, data = pending()
        for change in [dict(reason='actual_cds_recovery_unknown'), dict(default_entity='OTHER'), dict(snapshot_id='other')]:
            wrong = copy.deepcopy(result)
            wrong['lifecycle_unpriced'].update(change)
            with self.assertRaises(ValueError):
                life.checked_pending(wrong, operation, data)

    def test_default_remains_unavailable_after_worker_restart_without_commits(self):
        with tempfile.TemporaryDirectory(prefix='convex-bond-controls-') as folder:
            client, output = BondClient(), []
            for _ in range(2):
                worker = analytics.Workflow(config(), client, Path(folder) / 'state.json', UNIVERSE,
                    clock=lambda: analytics.timestamp(AT), emit=output.append)
                worker.frame(frame())
                self.assertFalse(output[-1]['ready'])
                self.assertEqual(output[-1]['reason'], 'bond_recovery_pending')
                self.assertEqual(client.commits, {})

    def test_known_bond_claim_requires_definition_mapping_and_exact_pnl(self):
        result, report = native()
        result['vector'].update(product='cash_bond', credit_scope=[])
        for state in [result['lifecycle_mark']['state'], report['trades'][0]['marks'][0]['state']]:
            state['default_event'] = 'default.1'
        result['lifecycle']['default_entity'] = 'BOND.ISSUER'
        operation = pending()[1]
        self.assertEqual(life.checked_state(result, report, operation), result['lifecycle'])
        with self.assertRaisesRegex(ValueError, 'mapping_missing'):
            life.checked_state(result, report)
        operation['lifecycle']['instrument']['credit']['entity'] = 'OTHER'
        with self.assertRaisesRegex(ValueError, 'scope_mismatch'):
            life.checked_state(result, report, operation)

    def test_exclusion_uses_exact_security_and_declared_credit_scope(self):
        with tempfile.TemporaryDirectory(prefix='convex-bond-controls-') as folder:
            worker = analytics.Workflow(config(), BondClient(), Path(folder) / 'state.json', UNIVERSE,
                clock=lambda: analytics.timestamp(AT), emit=lambda _: None)
            worker.frame(frame())
            candidate = worker.hedge_template['operation']['problem']['candidates'][0]
            worker.hedge_template['operation']['problem']['candidates'] = [copy.deepcopy(candidate) for _ in range(3)]
            candidates = worker.hedge_template['operation']['problem']['candidates']
            for candidate, key in zip(candidates, ['bond.alias', 'same.entity.cds', 'other.future'], strict=True):
                candidate['risk'] = key
            risks = {}
            for key, instrument, scope in [('trade.A', 'defaulted.bond', []), ('bond.alias', 'defaulted.bond', []),
                ('same.entity.cds', 'different.security', [dict(entity_id='BOND.ISSUER')]), ('other.future', 'unrelated.future', [])]:
                risks[key] = dict(native()[0]['vector'], instrument=dict(id=instrument, version=1), credit_scope=scope)
            risks['bond.alias']['instrument']['version'] = 2
            state = dict(default_entity='BOND.ISSUER', position=dict(identity=dict(instrument=risks['trade.A']['instrument'])))
            request = worker.hedge('snapshot', risks, [state])
            self.assertEqual([c['risk']['instrument']['id'] for c in request['operation']['problem']['candidates']], ['unrelated.future'])
            self.assertEqual(request['operation']['action']['kind'], 'solve_lifecycle')


if __name__ == '__main__':
    unittest.main()
