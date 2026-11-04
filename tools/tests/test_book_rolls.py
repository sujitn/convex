"""Date-roll source/history/retry controls; financial envelopes remain opaque."""
import copy
import json
from pathlib import Path
import sys
import tempfile
import unittest
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import book_events as events
import book_rolls as rolls
import market_data_stream as stream
import market_data_adapter as adapter
import streaming_analytics as analytics
from test_book_events import EventClient,example
from test_market_data_stream import UNIVERSE
from test_streaming_analytics import AT,config,PortClient,frame


def control():
    client=EventClient();_,old=example()
    value=copy.deepcopy(client.original)
    cfg=copy.deepcopy(old['base']['workflow'])
    cfg['plan']['definition']['version']=3
    cfg['plan'].update(as_of='2026-10-18T00:00:01Z',known_at='2026-10-18T00:00:01Z')
    cfg['plan']['curve_sets']['next']=[dict(id='CURVE-A',version=2)]
    cfg['bindings'][0]['follow']=False
    cfg['bindings'].append(dict(cfg['bindings'][0],set='next',follow=True,recipe_key=dict(id='native.A',version='2')))
    data=value['calculations']['pnl']['request']['operation']['input']
    point=copy.deepcopy(data['steps'][-1]['after'])
    point['market']=1;point['point']['valuation_date']='2026-10-18'
    data['markets'].append(dict(curves=[],calibration=None))
    data['steps'].append(dict(cause='time',after=point))
    data['standalone'].append(dict(kind='unavailable',reason='Actual prior fixing/cash state required'))
    value['calculations']['pnl']['curve_sets'].append('next')
    value['calculations']['risk.0']['curve_sets']=['next']
    batch=dict(schema_version=1,id='roll.1',base=old['base'],definition=value,workflow=cfg)
    return client,batch


class RollTests(unittest.TestCase):
    def test_pure_date_roll_needs_no_invented_cash_or_fill(self):
        client,batch=control()
        command,release=events.prepare(client,batch,date_roll=True)
        self.assertTrue(command['idempotency_key'].startswith('core.rolls.'))
        self.assertEqual(release['position_step'],1)
        self.assertEqual(events.facts(client.original['calculations']['pnl']['request']['operation']['input']),{})
        with self.assertRaises(ValueError):events.prepare(client,batch)

    def test_old_history_sources_policy_and_actual_receipts_are_not_rewritten(self):
        changes=[lambda b:b['workflow']['plan']['curve_sets']['current'][0].update(version=9),
            lambda b:b['workflow']['bindings'][0].update(max_age_ms=10000),
            lambda b:b['workflow']['bindings'][-1].update(source='other-provider'),
            lambda b:b['definition']['calculations']['pnl']['request']['operation']['input']['before']['point'].update(valuation_date='2026-10-16'),
            lambda b:b['definition']['calculations']['pnl']['request']['operation']['input']['steps'][-1].update(cause='event'),
            lambda b:b['workflow']['hedge']['operation']['problem'].update(reporting_currency='EUR'),
            lambda b:b['workflow']['plan'].update(as_of=AT)]
        for change in changes:
            client,batch=control();change(batch)
            with self.subTest(change=change),self.assertRaises((ValueError,KeyError)):
                events.prepare(client,batch,date_roll=True)

    def test_exact_saved_base_is_required_before_reference_write(self):
        client,batch=control();batch['base_snapshot']='saved'
        original=client.call
        def call(method,path,value=None):
            if path=='/analytics/snapshots/saved':
                record=original('GET','/reference/commits/commit.2')['master_records'][0]
                return dict(id='saved',content=dict(definition=record,plan=batch['base']['workflow']['plan']))
            return original(method,path,value)
        client.call=call
        command,release=rolls.prepare(client,batch)
        self.assertEqual(client.head,2)
        self.assertEqual(client.runs[0],('saved','pnl'))
        self.assertIn('date roll sha256=',command['master_changes'][0]['definition']['definition']['provenance']['evidence_reference'])
        batch['base_snapshot']='another'
        with self.assertRaises(KeyError):rolls.prepare(client,batch)
        self.assertEqual(release['version'],2)

    def test_fixing_date_follow_is_opt_in_and_keeps_source_and_image_minimum(self):
        key=('INDEX','2026-10-17','provider-neutral');new=(key[0],'2026-10-19',key[2])
        self.assertFalse(stream.subscribed_fixings({new},UNIVERSE))
        universe=(UNIVERSE[0],{key},'book',{(key[0],key[2])})
        self.assertTrue(stream.subscribed_fixings({new},universe))
        self.assertFalse(stream.subscribed_fixings({(key[0],new[1],'other-source')},universe))
        with tempfile.TemporaryDirectory() as name:
            path=Path(name)/'subscription.json'
            value=dict(schema_version=1,curves=[dict(curve_id=c,source=s) for c,s in UNIVERSE[0]],
                fixings=[dict(index_id=i,observation_date=d,source=s) for i,d,s in {key}],book='book',follow_fixing_dates=True)
            path.write_text(json.dumps(value));self.assertEqual(stream.subscription(path),universe)
            value['follow_fixing_dates']='yes';path.write_text(json.dumps(value))
            with self.assertRaises(adapter.IngestionError):stream.subscription(path)

    def test_worker_maturity_filter_uses_security_across_trade_aliases(self):
        cfg=config()
        with tempfile.TemporaryDirectory() as name:
            worker=analytics.Workflow(cfg,PortClient(),Path(name)/'state',UNIVERSE,emit=lambda _:None)
            vector=dict(instrument=dict(id='security',version=2),credit_scope=[])
            state=dict(matured_instrument=dict(id='security',version=1))
            request=worker.hedge('snapshot',{'trade.A':vector},[state])
            self.assertEqual(request['operation']['problem']['candidates'],[])
            self.assertEqual(request['operation']['action']['states'],[state])
            self.assertEqual(cfg,config())

    def test_dated_replay_restarts_with_advanced_clock_without_configuration_change(self):
        cfg=config()
        universe=(*UNIVERSE,'desk.book',set())
        with tempfile.TemporaryDirectory() as name:
            path=Path(name)/'state'
            worker=analytics.Workflow(cfg,PortClient(),path,universe,mode='replay:2026-10-17T00:00:01Z',emit=lambda _:None)
            worker.persist()
            recovered=analytics.Workflow(cfg,PortClient(),path,universe,mode='replay:2026-10-18T00:00:01Z',emit=lambda _:None)
            self.assertEqual(recovered.config_digest,worker.config_digest)
            self.assertFalse(recovered.current)
            with self.assertRaisesRegex(ValueError,'receipt/config mismatch'):
                analytics.Workflow(cfg,PortClient(),path,universe,mode='live',emit=lambda _:None)

    def test_scalar_subscription_has_a_fixed_rejection(self):
        with tempfile.TemporaryDirectory() as name:
            path=Path(name)/'subscription.json'
            for value in [None,0,'invalid',[]]:
                path.write_text(json.dumps(value))
                with self.subTest(value=value),self.assertRaises(adapter.IngestionError):
                    stream.subscription(path)

    def test_image_must_cover_selected_dates_beyond_initial_subscription(self):
        cfg=config()
        keys=[dict(index_id='INDEX',observation_date=d,source='provider-neutral') for d in ('2026-10-15','2026-10-16')]
        cfg['plan']['fixings']=keys
        universe=(UNIVERSE[0],{stream.fixing_key(keys[0])},'desk.book',{('INDEX','provider-neutral')})
        with tempfile.TemporaryDirectory() as name:
            output=[];client=PortClient()
            worker=analytics.Workflow(cfg,client,Path(name)/'state',universe,clock=lambda:analytics.timestamp(AT),emit=output.append)
            # Isolate image admission; reference/native values remain opaque.
            worker.observe_book=lambda _:None
            worker.select_book=lambda:None
            incoming=frame()
            incoming['records'].append(dict(kind='fixings',payload=[dict(key=keys[0],published_at=AT,recorded_at=AT)]))
            worker.frame(incoming)
            self.assertEqual(output[-1]['reason'],'incomplete_current_fixing_image')
            self.assertFalse(worker.current)
            self.assertEqual(client.calls,[])

    def test_maturity_uses_native_decimal_zero_without_a_string_default(self):
        from test_lifecycle_risk import native
        import lifecycle_risk
        result,report=native()
        pin=dict(id='security',version=1)
        result['valuation']='Matured'
        state=result['lifecycle'];state['matured_instrument']=pin
        for position in [state['position'],report['input']['points'][0]['positions'][0]]:
            position['identity']['instrument']=pin
            position['pending_quantity']='0.00'
        for mark in [result['lifecycle_mark'],report['trades'][0]['marks'][0]]:
            mark['state'].update(status='matured',remaining_fraction='0')
            mark['local']['pending']['value']=0
        state['pending'][0]['base_pv']=0
        result['vector']['sensitivities'][0].update(base_pv=0,value=0)
        self.assertEqual(lifecycle_risk.checked_state(result,report)['position']['pending_quantity'],'0.00')

    def test_policy_reference_is_not_a_curve_revision_even_with_the_same_id(self):
        client,batch=control()
        for cfg in [batch['base']['workflow'],batch['workflow']]:
            cfg['hedge']['operation']['problem']['candidates'][0]['eligibility']=dict(
                eligible=True,decision=dict(id='native.A',version=1),reason='Opaque policy')
        batch['workflow']['hedge']['operation']['problem']['candidates'][0]['eligibility']['decision']['version']=2
        with self.assertRaisesRegex(ValueError,'date_roll_policy_changed'):
            events.prepare(client,batch,date_roll=True)
        self.assertEqual(client.head,2)


if __name__=='__main__':unittest.main()
