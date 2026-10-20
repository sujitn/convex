#!/usr/bin/env python3
"""Runnable six-position/five-factor durable P&L checkpoint and recovery example."""
import argparse
import copy
from decimal import Decimal
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile

import book_checkpoints as checkpoints
import book_events as events
import core_book_workflow as books
import core_event_workflow as event_example
import core_live_workflow as automatic
import core_roll_workflow as rolls
import core_snapshot_workflow as core
import core_streaming_workflow as transport
import lifecycle_risk
import streaming_analytics as analytics

ROOT, require = core.ROOT, core.require


def pending_phase(value, identity, pending, payment=None):
    data = value['calculations']['pnl']['request']['operation']['input']
    close = copy.deepcopy(data['steps'][-1]['after'] if data['steps'] else data['before'])
    close['market'] = 1
    close['point'].update(snapshot_id=identity, manifest=dict(id=identity, version=1))
    for fx in close['point']['fx']:
        fx['snapshot_id'] = identity
    if payment is None:
        close['point']['positions'][1]['pending_quantity'] = str(pending)
    else:
        at = close['holdings'][0]
        holding = copy.deepcopy(data['holdings'][at])
        life = holding['lifecycle']
        life['events'].append(rolls.event(rolls.BOND, dict(kind='payment', terms=payment),
            identity+'.event', len(life['events']), close['point']['valuation_date'], close['point']['known_at']))
        close['holdings'][0] = len(data['holdings'])
        data['holdings'].append(holding)
    data['steps'].append(dict(cause='position' if payment is None else 'event', after=close))
    data['standalone'].append(dict(kind='unavailable', reason='Requires the preceding actual claim and pending-position history'))


def fill_phase(value):
    data=value['calculations']['pnl']['request']['operation']['input']
    market=data['steps'][-1]['after']['market']
    pending_phase(value,'core.checkpoint.actual.fill',-75000)
    data['steps'][-1]['after']['market']=market
    point=data['steps'][-1]['after']['point']
    position=point['positions'][1]
    position['current']['quantity']='351000'
    point['fills'].append(dict(id='core.checkpoint.actual.fill',trade_id=position['identity']['id'],
        instrument=copy.deepcopy(position['identity']['instrument']),quantity='1000',
        consideration=dict(value='-10',currency='USD',unit='currency'),fees=dict(value='-1',currency='USD',unit='currency'),
        date=point['valuation_date'],known_at=point['known_at'],source='core.synthetic.actual-fill'))
    names={c['request']['operation']['input']['identity']['id']:name for name,c in value['calculations'].items() if name!='pnl'}
    return rolls.sync_risks(value,dict(pnl='pnl',risks=names))


def payment(identity):
    return dict(receipt_id=identity, obligation=dict(kind='bond_coupon', effective_date='2026-10-08', ordinal=0),
        amount=dict(value='-1',currency='USD',unit='currency'), value_date='2026-10-09', source='core.synthetic.actual-cash')


def rebase(previous, config, saved, receipt, ordinal):
    """Producer-side wire binding only; all economics and totals stay native."""
    value, cfg = copy.deepcopy(previous), copy.deepcopy(config)
    data = value['calculations']['pnl']['request']['operation']['input']
    close = copy.deepcopy(data['steps'][-1]['after'])
    market = close['market']
    old_set = value['calculations']['pnl']['curve_sets'][market]
    holdings = [copy.deepcopy(data['holdings'][i]) for i in close['holdings']]
    before = copy.deepcopy(close)
    before.update(holdings=list(range(len(holdings))), market=0)
    before['point']['fills'] = []
    data['opening_quantities'] = copy.deepcopy(data.get('opening_quantities') or [p['current'] for p in data['before']['point']['positions']])
    data.update(before=before, holdings=holdings, markets=[copy.deepcopy(data['markets'][market]) for _ in range(2)],
                steps=[], standalone=[])
    pins = saved['content']['plan']['curve_sets'][old_set]
    value['calculations']['pnl']['curve_sets'] = ['opening', 'current']
    cfg['plan'] = copy.deepcopy(saved['content']['plan'])
    cfg['plan']['definition']['version'] += 1
    cfg['plan']['curve_sets'] = dict(opening=copy.deepcopy(pins), current=copy.deepcopy(pins))
    chosen = [r for r in config['bindings'] if r['set'] == old_set]
    cfg['bindings'] = [dict(copy.deepcopy(r), set=key, follow=key=='current') for key in ('opening','current') for r in chosen]
    value['reference_inputs']['pnl_checkpoint'] = dict(snapshot_id=saved['id'], calculation='pnl', build_id=receipt['build_id'])
    for n in range(8):
        identity = 'core.checkpoint.'+str(ordinal)+'.'+str(n)
        pending_phase(value, identity, -75000+(ordinal*8+n)*1000,
                      payment(identity+'.receipt') if n==0 else None)
    return rolls.sync_risks(value,cfg), cfg


def compare(client, ready, config):
    retained, native = [], {}
    for calc, row in ready['receipts'].items():
        saved = copy.deepcopy(client.snapshot(row['snapshot_id']))
        saved['content']['definition']['definition']['definition']['reference_inputs'].pop('pnl_checkpoint',None)
        expected = client.server.call(rolls.native_request(saved,calc))
        output = client.result(row)
        financial = copy.deepcopy(output)
        financial['result']['data'].pop('continuity',None)
        require(expected==financial,'Checkpoint full native parity failed')
        native[calc]=output;retained.append((row,client.result(row,raw=True)))
    report=native['pnl']['result']['data']['report']
    for total in report['reporting_totals']:
        rolls.issuer_example.check_attribution(total['attribution'])
    hedge=native['hedge']['result']['data']
    require(hedge['status']=='feasible' and all(c['satisfied'] for c in hedge['proposal']['constraints']),
            'Checkpoint hedge failed original policy')
    for identity in rolls.LIFECYCLE:
        calc=config['risks'][identity]
        operation=configured_operation(client,ready,calc)
        require(lifecycle_risk.checked_state(native[calc]['result']['data'],report,operation) in hedge['lifecycle_risk'],
                'Checkpoint hedge lost exact lifecycle state')
    if 'continuity' in native['pnl']['result']['data']:
        require(ready['pnl_continuity']==native['pnl']['result']['data']['continuity'],'Worker lost cumulative P&L')
    return retained,native


def configured_operation(client, ready, calc):
    return client.snapshot(ready['receipts'][calc]['snapshot_id'])['content']['definition']['definition']['definition']['calculations'][calc]['request']['operation']


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary-dir',type=Path)
    args=parser.parse_args()
    directory=args.binary_dir
    if directory is None:
        subprocess.run(['cargo','build','--locked','-p','convex-server','--bins'],cwd=ROOT,check=True)
        directory=Path(json.loads(subprocess.check_output(['cargo','metadata','--locked','--no-deps','--format-version','1'],cwd=ROOT))['target_directory'])/'debug'
    binary, importer=[(directory/n).resolve(strict=True) for n in ('convex-server','convex-core-inputs')]
    capture=json.loads((ROOT/'tests/fixtures/library/date-rolls-core.json').read_text())
    value,plan=capture['definition'],capture['plan'];plan['definition']['version']=1
    value=fill_phase(value)
    records=[transport.item('curve',r) for r in capture['curves']]+[transport.item('fixings',capture['fixings'])]
    with tempfile.TemporaryDirectory(prefix='convex-core-checkpoint-') as name,books.BookFeed(records) as feed:
        folder=Path(name)
        def import_capture(references,curves=(),fixings=()):
            path=folder/'capture.json';path.write_text(json.dumps(dict(schema_version=1,references=references,curves=list(curves),fixings=list(fixings))))
            subprocess.run([str(importer),str(folder/'state.redb'),str(path)],cwd=ROOT,check=True,stdout=subprocess.PIPE)
            path.unlink()
        command=dict(idempotency_key='core.checkpoint.initial',changes=[],master_changes=[dict(expected_version=None,definition=dict(kind='calculation',definition=value))])
        import_capture([dict(recorded_at=plan['known_at'],command=command)],capture['curves'],capture['fixings'])
        with core.supplied.ServerProcess(binary,folder) as server:
            initial,initial_plan,factors=core.prepare()
            config=automatic.configuration(server,initial,initial_plan,factors)
        config['plan']=copy.deepcopy(plan)
        for factor in config['hedge']['operation']['problem']['factors']:
            for pin in factor['key']['coordinate'].values():
                if isinstance(pin,dict) and set(pin)=={'id','version'}:
                    pin['version']='4'
        config['bindings']=[]
        for key,pins in plan['curve_sets'].items():
            for pin in pins:
                row=next(r for r in capture['curves'] if r['curve_id']==pin['id'] and r['version']==pin['version'])
                config['bindings'].append(dict(set=key,curve_id=pin['id'],source=row['source'],follow=key=='market.3',
                    max_age_ms=5000,recipe_key=copy.deepcopy(row['recipe']['key'])))
        current=dict(id=books.BOOK,version=1,reference_commit=command['idempotency_key'],workflow=config,position_step=8)
        feed.retain_book(transport.item('book',current))
        subscription=transport.universe(records);subscription.update(book=books.BOOK,follow_fixing_dates=True)
        (folder/'subscription.json').write_text(json.dumps(subscription));(folder/'workflow.json').write_text(json.dumps(config))
        retained,reports=[],[]
        with core.supplied.ServerProcess(binary,folder) as server,automatic.WorkerProcess(feed,core.Client(server).root,folder,plan['known_at']) as worker:
            ready=worker.wait('ready');more,native=compare(core.Client(server),ready,config)
            retained.extend(more);reports.append(native);saved=core.Client(server).snapshot(ready['snapshot_id'])
        for ordinal in (1,2):
            value,config=rebase(value,config,saved,ready['receipts']['pnl'],ordinal)
            batch=dict(schema_version=1,id='window.'+str(ordinal),base=copy.deepcopy(current),base_snapshot=ready['snapshot_id'],definition=value,workflow=config)
            with core.supplied.ServerProcess(binary,folder) as server:
                command,_=checkpoints.prepare(analytics.Client(core.Client(server).root,timeout=120),batch)
            import_capture([dict(recorded_at=plan['known_at'],command=command)])
            path=folder/'checkpoint.json';path.write_text(json.dumps(batch))
            with core.supplied.ServerProcess(binary,folder) as server:
                client=core.Client(server)
                output=json.loads(subprocess.check_output([sys.executable,str(ROOT/'tools/book_checkpoints.py'),str(path),'--url',client.root,'--timeout','120'],cwd=ROOT))
                require(output==checkpoints.ingest(analytics.Client(client.root,timeout=120),batch),'Checkpoint retry changed release')
                current=output['payload'];feed.retain_book(output)
                if ordinal==2:
                    with automatic.lost_result_ack(client.root,path_marker='/run/hedge') as (proxy,committed),automatic.WorkerProcess(feed,proxy,folder,plan['known_at']) as worker:
                        require(committed.wait(30),'Checkpoint hedge did not commit before ACK')
                        worker.process.kill();worker.process.wait(timeout=5)
                with automatic.WorkerProcess(feed,client.root,folder,plan['known_at']) as worker:
                    worker.wait('unavailable');ready=worker.wait('ready')
                    more,native=compare(client,ready,config);retained.extend(more);reports.append(native)
                    require(ready['pnl_continuity']['completed_causes']==9+8*ordinal,'Cumulative causal count changed')
                    report=native['pnl']['result']['data']['report']
                    mark=event_example.closing_mark(native,rolls.BOND)
                    require(mark['recorded_cash']['value']==str(-ordinal),'Receipt cash lost across checkpoint')
                    require(sum(Decimal(c['outstanding']['value']) for c in mark['claims'])==Decimal('-1003.06')+ordinal,'Unpaid claim lost')
                    saved=client.snapshot(ready['snapshot_id'])
        # An ordinary existing event batch can extend the new checkpoint window.
        pending_phase(value,'core.checkpoint.last.receipt',0,payment('core.checkpoint.last.receipt'))
        value=rolls.sync_risks(value,config);config=copy.deepcopy(config);config['plan']['definition']['version']+=1
        batch=event_example.batch(current,value,'core.checkpoint.ordinary.event');batch['workflow']=config
        with core.supplied.ServerProcess(binary,folder) as server:
            command,_=events.prepare(analytics.Client(core.Client(server).root,timeout=120),batch)
        import_capture([dict(recorded_at=plan['known_at'],command=command)])
        with core.supplied.ServerProcess(binary,folder) as server:
            client=core.Client(server);output=events.ingest(analytics.Client(client.root,timeout=120),batch);feed.retain_book(output)
            with automatic.WorkerProcess(feed,client.root,folder,plan['known_at']) as worker:
                ready=worker.wait('ready');more,native=compare(client,ready,config);retained.extend(more)
                require(ready['pnl_continuity']['completed_causes']==26,'Ordinary post-checkpoint event lost carry')
                mark=event_example.closing_mark(native,rolls.BOND)
                require(mark['recorded_cash']['value']=='-3','Final coupon receipt lost')
                totals=ready['pnl_continuity']['local_totals']
                pv=next(t for t in totals if t['basis']=='CurrencyPresentValue' and t['currency']=='USD')
                require(pv['total']['cash']['value']==-14,'Prior fill/fee or receipt cash was counted again')
                report=native['pnl']['result']['data']['report']
                require(report['input']['points'][0]['positions'][1]['current']['quantity']=='351000'
                    and report['opening_quantities'][1]['quantity']=='350000','Actual filled position or original claim owner lost')
                require(sum(Decimal(c['outstanding']['value']) for c in mark['claims'])==Decimal('-1000.06'),'Residual principal/coupon claim lost')
        with core.supplied.ServerProcess(binary,folder) as server,automatic.WorkerProcess(feed,core.Client(server).root,folder,plan['known_at']) as worker:
            client=core.Client(server);worker.wait('unavailable');recovered=worker.wait('ready')
            require(recovered['snapshot_id']==ready['snapshot_id'] and all(r['reused'] for r in recovered['receipts'].values()),'Checkpoint restart did not reuse exact saved results')
            require(recovered['pnl_continuity']==ready['pnl_continuity'],'Restart lost cumulative P&L')
            for row,raw in retained:
                require(client.result(row,raw=True)==raw,'Prior immutable result bytes changed')
            for calc in ready['receipts']:
                row=ready['receipts'][calc];again=client.run(row['snapshot_id'],calc,recompute=True)
                require(client.result(again,raw=True)==client.result(row,raw=True),'Checkpoint saved-input recompute changed')
    print(json.dumps(dict(status='passed',data_quality='synthetic historical replay',trades=6,factors=5,checkpoints=2,
        causal_phases=26,native_window_causes=[9,8,9],full_lifecycle_replay=True,outstanding_claims='1000.06 USD payable',
        actual_receipts='3 USD paid',current_pending_risk_and_hedges=True,native_parity=True,cumulative_pnl=True,
        adapter_cli_retry=True,post_commit_pre_ack_kill_recovered=True,restart_bytes_cache_recompute_identical=True,
        persisted_results=len(retained),result_digests={r['result_id']:hashlib.sha256(raw).hexdigest() for r,raw in retained},
        execution_authority='none',processes_stopped=True),indent=2))


if __name__=='__main__':
    main()
