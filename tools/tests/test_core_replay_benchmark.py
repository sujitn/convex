"""Workload correctness controls; timings never act as a CI latency threshold."""
import hashlib
import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import book_events
import core_replay_benchmark as bench
import lifecycle_history
import lifecycle_risk


class BenchmarkWorkloads(unittest.TestCase):
    def test_lots_keep_complete_unique_legal_and_risk_coverage(self):
        case = bench.workload(2, 12)
        data = case['definition']['calculations']['pnl']['request']['operation']['input']
        ids = [p['identity']['id'] for p in data['before']['point']['positions']]
        self.assertEqual(len(ids), 12)
        self.assertEqual(len(set(ids)), 12)
        self.assertEqual(case['trades'], 12)
        for holding in data['holdings']:
            life = holding['lifecycle']
            self.assertEqual(life['trade_id'], holding['id'])
            if life['instrument']['kind'] in ('swap', 'cds'):
                self.assertEqual(life['instrument']['terms']['trade_id'], holding['id'])
            operation = next(c['request']['operation'] for name, c in case['definition']['calculations'].items()
                             if name != 'pnl' and c['request']['operation']['input']['identity']['id'] == holding['id'])
            if operation['kind'] == 'lifecycle_hedge_risk':
                lifecycle_risk.check_binding(operation, holding, data)

    def test_distinct_causal_snapshots_and_exact_checkpoint_opening(self):
        case = bench.workload(1, 12)
        source = case['definition']['calculations']['pnl']['request']['operation']['input']
        following = case['next_definition']['calculations']['pnl']['request']['operation']['input']
        a, b = source['before']['point'], source['steps'][0]['after']['point']
        self.assertNotEqual(a['snapshot_id'], b['snapshot_id'])
        self.assertNotEqual(a['manifest'], b['manifest'])
        self.assertNotEqual(a['positions'][1]['pending_quantity'], b['positions'][1]['pending_quantity'])
        self.assertEqual(book_events.full_point(source, source['steps'][0]['after']),
                         book_events.full_point(following, following['before']))
        for binding in [source['before'], source['steps'][0]['after'], following['steps'][0]['after']]:
            self.assertTrue(all(fx['snapshot_id'] == binding['point']['snapshot_id'] for fx in binding['point']['fx']))

    def test_receipt_stress_is_actual_signed_unique_ordered_cash(self):
        case = bench.workload(1, 1024)
        life = case['replays'][0]['operation']['input']
        self.assertEqual(life['position']['value'], '-1000000')
        paid = [s['event'] for s in life['events'] if s['event']['action']['kind'] == 'payment']
        self.assertEqual(len(paid), 1024)
        self.assertEqual(len({e['action']['terms']['receipt_id'] for e in paid}), 1024)
        self.assertEqual([e['expected_revision'] for e in paid], list(range(4, 1028)))
        self.assertTrue(all(e['trade_id'] == life['trade_id'] and e['action']['terms']['amount']['value'] == '-1' for e in paid))
        self.assertEqual(case['classification'], 'split-receipt history stress')

    def test_compaction_reconstructs_all_original_requests(self):
        case = bench.workload(1, 12)
        expanded = lifecycle_history.expand(case['compact_definition'])
        expanded['reference_inputs'].pop('lifecycle_history', None)
        self.assertEqual(expanded, case['next_definition'])

    def test_supplied_fixture_is_read_only_and_rules_are_retained(self):
        before = hashlib.sha256(bench.FIXTURE.read_bytes()).hexdigest()
        case = bench.workload(1, 0)
        self.assertEqual(before, hashlib.sha256(bench.FIXTURE.read_bytes()).hexdigest())
        original = json.loads(bench.FIXTURE.read_bytes())
        self.assertEqual(case['definition']['reference_inputs'], original['definition']['reference_inputs'])
        self.assertEqual(len(case['requests']['pnl']['operation']['input']['resources']['publications']), 2002)

    def test_larger_cases_do_not_raise_any_application_bounds(self):
        small, large = bench.workload(1, 0), bench.workload(6, 0)
        self.assertEqual(len(large['definition']['calculations']), 37)
        self.assertLess(len(json.dumps(small['definition'])), len(json.dumps(large['definition'])))
        self.assertEqual(small['requests']['pnl']['operation']['input']['max_points'],
                         large['requests']['pnl']['operation']['input']['max_points'])

    def test_invalid_workload_parameters_reject(self):
        for lots, receipts in [(0, 0), (7, 0), (True, 0), (1, -1), (1, 2049), (1, True)]:
            with self.subTest(lots=lots, receipts=receipts), self.assertRaises(ValueError):
                bench.workload(lots, receipts)


if __name__ == '__main__':
    unittest.main()
