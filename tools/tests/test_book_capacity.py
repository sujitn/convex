"""Bigger books keep one exact position, risk, receipt and checkpoint binding."""
import copy
import json
from pathlib import Path
import sys
import unittest
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import book_capacity as capacity
import core_capacity_workflow as workflow
import lifecycle_history
import lifecycle_risk


class BookCapacity(unittest.TestCase):
    def test_explicit_profile_and_original_payload_limits(self):
        self.assertFalse(capacity.expanded({}))
        for bad in (None,True,'unbounded',{}):
            with self.subTest(bad=bad),self.assertRaises(ValueError):
                capacity.expanded(dict(reference_inputs=dict(core_capacity=bad)))
        value,_,_=workflow.workload()
        self.assertTrue(capacity.expanded(value))
        self.assertEqual(json.loads(capacity.payload_bytes(dict(definition=value)))['definition'],value)
        plain=copy.deepcopy(value);plain['reference_inputs'].pop('core_capacity')
        with self.assertRaises(ValueError):
            capacity.payload_bytes(dict(definition=plain))

    def test_sixty_legal_trades_have_complete_distinct_risks(self):
        value,plan,risks=workflow.workload(10,0)
        data=value['calculations']['pnl']['request']['operation']['input']
        self.assertEqual(len(risks),60);self.assertEqual(len(value['calculations']),61)
        self.assertEqual(set(risks),{p['identity']['id'] for p in data['before']['point']['positions']})
        self.assertEqual(plan['definition']['id'],value['id'])
        for h in data['holdings']:
            operation=value['calculations'][risks[h['id']]]['request']['operation']
            if operation['kind']=='lifecycle_hedge_risk':
                lifecycle_risk.check_binding(operation,h,data)

    def test_partial_payment_extends_only_one_immutable_lifecycle(self):
        value,_,risks=workflow.workload()
        config=dict(pnl='pnl',risks=risks)
        after=workflow.payment_update(value,config)
        old=value['calculations']['pnl']['request']['operation']['input']
        new=after['calculations']['pnl']['request']['operation']['input']
        self.assertEqual(old['holdings'],new['holdings'][:-1])
        self.assertEqual(old['steps'],new['steps'][:-1])
        step=new['holdings'][-1]['lifecycle']['events'][-1]['event']
        self.assertEqual(step['action']['terms']['amount']['value'],'-1')
        self.assertEqual(step['trade_id'],new['holdings'][-1]['id'])
        for i in new['steps'][-1]['after']['holdings']:
            h=new['holdings'][i]
            op=after['calculations'][risks[h['id']]]['request']['operation']
            if op['kind']=='lifecycle_hedge_risk':lifecycle_risk.check_binding(op,h,new)

    def test_unknown_or_excessive_lot_counts_reject(self):
        for lots in (0,21,True,1.5):
            with self.subTest(lots=lots),self.assertRaises(ValueError):workflow.workload(lots)

    def test_compaction_remains_lossless_on_expanded_definitions(self):
        value,_,_=workflow.workload(6,12)
        value['reference_inputs']['pnl_checkpoint']=dict(snapshot_id='control.saved',calculation='pnl',build_id='control.build')
        expanded=lifecycle_history.expand(lifecycle_history.compact(value))
        expanded['reference_inputs'].pop('lifecycle_history',None)
        self.assertEqual(expanded,value)


if __name__=='__main__':unittest.main()
