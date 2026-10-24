"""Checkpoint wiring/retry controls; native controls separately own financial truth."""
import copy
from pathlib import Path
import sys
import tempfile
import unittest
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import book_checkpoints as checkpoints
import book_events as events
import market_data_adapter as adapter
import streaming_analytics as analytics
from test_book_events import EventClient,example
from test_streaming_analytics import AT,config,PortClient,frame
from test_market_data_stream import UNIVERSE


def control():
    client=EventClient();_,old=example();batch=copy.deepcopy(old)
    batch.update(id='checkpoint.1',base_snapshot='saved')
    value=copy.deepcopy(client.original)
    data=value['calculations']['pnl']['request']['operation']['input']
    close=copy.deepcopy(data['steps'][-1]['after']);close['point']['fills']=[]
    data['before']=copy.deepcopy(close);data['steps']=[dict(cause='position',after=copy.deepcopy(close))]
    data['standalone']=[dict(kind='unavailable',reason='Opaque full preceding state')]
    value['reference_inputs']['pnl_checkpoint']=dict(snapshot_id='saved',calculation='pnl',build_id='build')
    batch['definition']=value
    call,run=client.call,client.run
    def get(method,path,value=None):
        if path=='/analytics/snapshots/saved':
            record=call('GET','/reference/commits/commit.2')['master_records'][0]
            return dict(id='saved',content=dict(definition=record,plan=batch['base']['workflow']['plan']))
        return call(method,path,value)
    def calculate(snapshot,calc):
        receipt,result=run(snapshot,calc);receipt['build_id']='build';return receipt,result
    client.call,client.run=get,calculate
    return client,batch


class CheckpointTests(unittest.TestCase):
    def test_exact_source_closing_is_required_before_reference_write(self):
        client,batch=control();command,release=checkpoints.prepare(client,batch)
        self.assertTrue(command['idempotency_key'].startswith('core.checkpoint.'))
        self.assertEqual(client.head,2)
        self.assertEqual(client.runs,[('saved','pnl')])
        self.assertEqual(release['position_step'],0)
        self.assertEqual(release['version'],2)

    def test_changed_opening_and_policy_do_not_write(self):
        changes=[lambda b:b['definition']['calculations']['pnl']['request']['operation']['input']['before']['point']['positions'][0].update(pending_quantity='999'),
            lambda b:b['workflow']['hedge']['operation']['problem'].update(reporting_currency='EUR'),
            lambda b:b['workflow']['bindings'][0].update(max_age_ms=9000),
            lambda b:b['workflow']['bindings'][0].update(source='another'),
            lambda b:b['workflow']['plan']['curve_sets']['current'][0].update(version=9),
            lambda b:b['definition']['reference_inputs']['pnl_checkpoint'].update(build_id='another'),
            lambda b:b['definition']['calculations']['pnl']['request']['operation']['input']['holdings'][0]['lifecycle']['events'].append(dict(changed='event'))]
        for change in changes:
            client,batch=control();change(batch)
            with self.subTest(change=change),self.assertRaises((ValueError,KeyError,TypeError)):
                checkpoints.prepare(client,batch)
            self.assertEqual(client.head,2)

    def test_source_must_be_priced_and_snapshot_must_match(self):
        client,batch=control();client.fail_native=True
        with self.assertRaises(analytics.Blocked):checkpoints.prepare(client,batch)
        self.assertEqual(client.head,2)
        client.fail_native=False;batch['base_snapshot']='other'
        with self.assertRaises(KeyError):checkpoints.prepare(client,batch)
        self.assertEqual(client.head,2)

    def test_lost_commit_ack_retry_retains_one_revision(self):
        client,batch=control();client.ack_loss=True
        with self.assertRaises(adapter.IngestionError):checkpoints.ingest(client,batch)
        self.assertEqual(client.head,3)
        first=checkpoints.ingest(client,batch)
        self.assertEqual(first,checkpoints.ingest(client,copy.deepcopy(batch)))
        self.assertEqual(client.head,3)
        changed=copy.deepcopy(batch);changed['definition']['provenance']['evidence_reference']+=' changed'
        with self.assertRaises(adapter.IngestionError):checkpoints.ingest(client,changed)

    def test_bounds_and_unsupported_schema(self):
        for field,value in [('schema_version',True),('base_snapshot',None),('schema_version',2)]:
            client,batch=control();batch[field]=value
            with self.subTest(field=field,value=value),self.assertRaises((ValueError,KeyError,TypeError)):
                checkpoints.prepare(client,batch)
        client,batch=control()
        data=batch['definition']['calculations']['pnl']['request']['operation']['input']
        data['steps']*=17
        with self.assertRaisesRegex(ValueError,'checkpoint_window_bound'):checkpoints.prepare(client,batch)

    def test_worker_persists_carry_but_needs_a_new_image_on_restart(self):
        client=PortClient();original=client.run
        def run(snapshot,calc):
            receipt,value=original(snapshot,calc)
            if calc=='pnl':value['continuity']=dict(completed_causes=25,completed_windows=3,local_totals='opaque',reporting_totals='opaque')
            return receipt,value
        client.run=run;output=[]
        with tempfile.TemporaryDirectory() as name:
            path=Path(name)/'receipt'
            worker=analytics.Workflow(config(),client,path,UNIVERSE,clock=lambda:analytics.timestamp(AT),emit=output.append)
            worker.frame(frame())
            self.assertTrue(worker.current)
            self.assertEqual(output[-1]['pnl_continuity']['completed_causes'],25)
            recovered=analytics.Workflow(config(),client,path,UNIVERSE,clock=lambda:analytics.timestamp(AT),emit=output.append)
            self.assertFalse(recovered.current)
            self.assertIsNone(recovered.bundle)
            recovered.frame(frame());self.assertEqual(output[-1]['pnl_continuity']['completed_causes'],25)


if __name__=='__main__':unittest.main()
