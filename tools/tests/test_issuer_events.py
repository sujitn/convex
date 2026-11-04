"""Issuer source/atomic admission controls; native financial checks live in Rust."""
import copy
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import book_events
import core_issuer_workflow as example
import core_snapshot_workflow as core
import issuer_events
import issuer_scope
import market_data_adapter as adapter
from test_book_events import EventClient, example as opaque
from test_cds_events import resolution


def prepared():
    client = EventClient()
    _, batch = opaque()
    client.original['reference_inputs']['issuer_scope'] = dict(id='scope', version=1)
    batch['definition']['reference_inputs']['issuer_scope'] = dict(id='scope', version=2)
    directory = dict(issuers=[dict(entity_id='ACME', version=1, default=None)],
                     instruments=[dict(id='security', version=1, entity_id='ACME')])
    scope = dict(id='scope', effective=copy.deepcopy(client.original['effective']),
                 provenance=copy.deepcopy(client.original['provenance']), directory=directory)
    record = dict(version=2, recorded_at=client.original['effective']['from'],
                  definition=dict(kind='calculation', definition=client.original))
    saved = dict(id='saved-base', content=dict(definition=copy.deepcopy(record),
        plan=copy.deepcopy(batch['base']['workflow']['plan']),
        issuer_scope=dict(version=1, definition=dict(kind='issuer_scope', definition=copy.deepcopy(scope)))))
    scope['directory']['issuers'][0]['default'] = copy.deepcopy(example.NOTICE)
    batch.update(base_snapshot=saved['id'], issuer_scope_change=dict(expected_version=1, definition=dict(kind='issuer_scope', definition=scope)))
    call = client.call
    def with_snapshot(method, path, value=None):
        if path == '/analytics/snapshots/saved-base':
            return copy.deepcopy(saved)
        return call(method, path, value)
    client.call = with_snapshot
    return client, batch, saved


class IssuerEventsTests(unittest.TestCase):
    def test_atomic_command_and_exact_retry_replay_the_saved_base(self):
        client, batch, _ = prepared()
        # Opaque transport control only; full native coverage is tested separately.
        with patch('issuer_scope.validate_book'):
            command, release = issuer_events.prepare(client, batch)
            first = issuer_events.ingest(client, batch)
            self.assertEqual(first, issuer_events.ingest(client, copy.deepcopy(batch)))
        self.assertEqual(len(command['master_changes']), 2)
        self.assertEqual(command['master_changes'][0]['expected_version'], 2)
        self.assertEqual(command['master_changes'][1], batch['issuer_scope_change'])
        self.assertEqual(first['payload'], release)
        self.assertEqual(client.runs[0], ('saved-base', 'pnl'))
        self.assertEqual(len(client.commands), 1)

    def test_lost_ack_recovers_the_identical_atomic_command(self):
        client, batch, _ = prepared()
        client.ack_loss = True
        with patch('issuer_scope.validate_book'):
            with self.assertRaises(adapter.IngestionError):
                issuer_events.ingest(client, batch)
            output = issuer_events.ingest(client, batch)
        self.assertEqual(output['payload']['version'], 2)
        self.assertEqual(client.head, 3)

    def test_snapshot_or_scope_pin_changes_do_not_commit(self):
        for mutate in (lambda b, s: s.update(id='other'),
                       lambda b, s: s['content']['plan']['definition'].update(version=99),
                       lambda b, s: b['issuer_scope_change'].update(expected_version=2),
                       lambda b, s: b['definition']['reference_inputs']['issuer_scope'].update(version=3)):
            client, batch, saved = prepared()
            mutate(batch, saved)
            with patch('issuer_scope.validate_book'), self.assertRaises(ValueError):
                issuer_events.prepare(client, batch)
            self.assertEqual(client.commands, {})

    def test_retained_mappings_cannot_be_reassigned_or_notice_omitted(self):
        for change in ('remap', 'no_notice'):
            client, batch, _ = prepared()
            directory = batch['issuer_scope_change']['definition']['definition']['directory']
            if change == 'remap':
                directory['issuers'].append(dict(entity_id='OTHER', version=1, default=None))
                directory['instruments'][0]['entity_id'] = 'OTHER'
            else:
                directory['issuers'][0]['default'] = None
            with patch('issuer_scope.validate_book'), self.assertRaises(ValueError):
                issuer_events.prepare(client, batch)
            self.assertEqual(client.commands, {})

    def test_ordinary_immutable_history_checks_still_apply(self):
        client, batch, _ = prepared()
        batch['definition']['calculations']['pnl']['request']['operation']['input']['before']['point']['fills'].append(dict(id='rewrite'))
        with patch('issuer_scope.validate_book'), self.assertRaises(ValueError):
            issuer_events.prepare(client, batch)
        self.assertEqual(client.commands, {})

    def test_one_recovery_with_shared_notice_does_not_cover_another_trade(self):
        client, batch = resolution()
        client.original['reference_inputs']['issuer_scope'] = dict(id='scope', version=2)
        batch['definition']['reference_inputs']['issuer_scope'] = dict(id='scope', version=2)
        pending = [dict(state=dict(trade_id=t, default_event='default.1')) for t in ('trade.A', 'trade.B')]
        with patch('book_events.pending_defaults', return_value=pending), self.assertRaisesRegex(ValueError, 'each_contract_recovery'):
            book_events.prepare(client, batch)
        self.assertEqual(client.commands, {})

    def test_complete_native_book_fanout_and_explicit_other_issuer(self):
        capture, _, factors = core.prepare()
        value = example.definition(capture, factors)
        config = dict(pnl='pnl', risks={c['request']['operation']['input']['identity']['id']: n
            for n, c in value['calculations'].items() if n.startswith('risk.')})
        _, scope = example.reference_changes(value)
        value = example.append(value, {}, 'issuer.test.cancel', cancel=True)
        value = example.append(value, example.defaults(), 'issuer.test.default')
        import lifecycle_risk
        value = lifecycle_risk.bind_requests(value, config, example.LIFECYCLE)
        scope['directory']['issuers'][0]['default'] = copy.deepcopy(example.NOTICE)
        issuer_scope.validate_book(value, config, scope['directory'])
        for missing in example.AFFECTED:
            wrong = copy.deepcopy(value)
            data = wrong['calculations']['pnl']['request']['operation']['input']
            holding = next(data['holdings'][i] for i in data['steps'][-1]['after']['holdings'] if data['holdings'][i]['id'] == missing)
            holding['lifecycle']['events'].pop()
            with self.assertRaisesRegex(ValueError, 'incomplete_contract_coverage'):
                issuer_scope.validate_book(wrong, config, scope['directory'])

    def test_distinct_security_of_defaulted_issuer_is_filtered_and_unmapped_fails(self):
        directory = dict(issuers=[dict(entity_id='ACME', version=1, default=example.NOTICE),
                                 dict(entity_id='OTHER', version=1, default=None)], instruments=[
            dict(id='new.bond', version=3, entity_id='ACME'), dict(id='other.bond', version=1, entity_id='OTHER')])
        scope = dict(reference=dict(id='scope', version=2), snapshot_id='snapshot', directory=directory)
        request = dict(operation=dict(problem=dict(candidates=[dict(risk=dict(product='cash_bond', instrument=dict(id=i, version=v)))
            for i, v in [('new.bond', 3), ('other.bond', 1)]])))
        result = issuer_scope.hedge(copy.deepcopy(request), 'snapshot', scope, [])
        self.assertEqual([c['risk']['instrument']['id'] for c in result['operation']['problem']['candidates']], ['other.bond'])
        request['operation']['problem']['candidates'][0]['risk']['instrument']['version'] = 2
        with self.assertRaisesRegex(ValueError, 'mapping_missing'):
            issuer_scope.hedge(request, 'snapshot', scope, [])


if __name__ == '__main__':
    unittest.main()
