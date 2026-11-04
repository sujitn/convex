"""Lossless prefix wiring; native replay owns all financial admission."""
import copy
import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import book_checkpoints
import lifecycle_history as history
from test_book_checkpoints import control


def example():
    value = json.loads((Path(__file__).resolve().parents[2] / 'tests/fixtures/library/date-rolls-core.json').read_text())['definition']
    data = value['calculations']['pnl']['request']['operation']['input']
    data['before'] = copy.deepcopy(data['steps'][-1]['after'])
    value['reference_inputs']['pnl_checkpoint'] = dict(snapshot_id='saved', calculation='pnl', build_id='build')
    return value


class HistoryTests(unittest.TestCase):
    def test_roundtrip_keeps_every_original_fact_and_reduces_bytes(self):
        full = example()
        packed = history.compact(full)
        expanded = history.expand(packed)
        expanded['reference_inputs'].pop('lifecycle_history')
        self.assertEqual(expanded, full)
        self.assertEqual(history.pack(history.expand(packed)), packed)
        self.assertLess(len(json.dumps(packed)), len(json.dumps(full)))

    def test_repeated_checkpoint_is_flat_and_self_contained(self):
        packed = history.compact(example())
        rows = history.archive(packed)
        self.assertTrue(rows)
        self.assertEqual(history.compact(packed), packed)
        self.assertTrue(all(isinstance(s, list) for s in rows.values()))

    def test_new_facts_are_tails_and_are_not_lost_or_relabelled(self):
        packed = history.compact(example())
        full = history.expand(packed)
        rows = history.archive(full)
        life = next(l for l in history.lifecycles(full) if l['events'] == rows.get(l['trade_id']))
        event = copy.deepcopy(life['events'][-1])
        event['event']['event_id'] = 'new.actual.event'
        life['events'].append(event)
        changed = history.pack(full)
        self.assertEqual(history.expand(changed), full)
        selected = next(l for l in history.lifecycles(changed) if l['events'] == dict(prefix=life['trade_id'], tail=[event]))
        self.assertEqual(selected['trade_id'], life['trade_id'])

    def test_wrong_trade_missing_nested_or_overlong_prefix_reject(self):
        original = history.compact(example())
        for case in range(4):
            value = copy.deepcopy(original)
            rows = value['reference_inputs']['lifecycle_history']
            identity = next(iter(rows))
            if case == 0:
                del rows[identity]
            elif case == 1:
                rows[identity] = dict(prefix=identity, tail=[])
            elif case == 2:
                rows[identity] = rows[identity] * 4097
            else:
                rows[identity][0]['event']['trade_id'] = 'wrong.trade'
            with self.subTest(case=case), self.assertRaises(ValueError):
                history.expand(value)

    def test_tail_cannot_bypass_event_bound(self):
        value = history.compact(example())
        life = next(l for l in history.lifecycles(value) if isinstance(l['events'], dict))
        life['events']['tail'] = [{}] * 4096
        with self.assertRaises(ValueError):
            history.expand(value)

    def test_compaction_requires_a_checkpoint_and_keeps_other_references(self):
        full = example()
        full['reference_inputs']['issuer_scope'] = dict(id='actual.issuer.directory', version=7)
        packed = history.compact(full)
        self.assertEqual(packed['reference_inputs']['issuer_scope'], full['reference_inputs']['issuer_scope'])
        full['reference_inputs'].pop('pnl_checkpoint')
        with self.assertRaises(ValueError):
            history.compact(full)

    def test_unique_history_is_not_archived_when_sharing_would_add_bytes(self):
        value = example()
        data = value['calculations']['pnl']['request']['operation']['input']
        life = copy.deepcopy(data['holdings'][data['before']['holdings'][0]])
        data.update(holdings=[life], before=dict(holdings=[0]))
        value['calculations'] = dict(pnl=value['calculations']['pnl'])
        self.assertEqual(history.compact(value), value)

    def test_existing_checkpoint_admission_supports_opt_in_and_exact_retry(self):
        client, batch = control()
        batch['definition'] = history.compact(batch['definition'])
        first = book_checkpoints.ingest(client, batch)
        self.assertEqual(first, book_checkpoints.ingest(client, copy.deepcopy(batch)))
        self.assertEqual(client.head, 3)


if __name__ == '__main__':
    unittest.main()
