#!/usr/bin/env python3
"""Run a larger mixed book through existing streaming, snapshot and result ports.

Synthetic legal lots reuse the retained six native products. All pricing, risk,
hedging and P&L stay in native Rust. No provider, database or queue is introduced.
"""
import argparse
import copy
from decimal import Decimal
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile

import book_capacity
import book_checkpoints as checkpoints
import book_events as events
import core_book_workflow as books
import core_live_workflow as automatic
import core_replay_benchmark as benchmark
import core_roll_workflow as rolls
import core_snapshot_workflow as core
import core_streaming_workflow as transport
import lifecycle_history
import lifecycle_risk
import streaming_analytics as analytics

ROOT, require = core.ROOT, core.require


def workload(lots=6, receipts=12):
    if type(lots) is not int or not 1 <= lots <= 20:
        raise ValueError('book_lot_bound')
    case = benchmark.workload(1, receipts)
    value = copy.deepcopy(case['definition'])
    data = value['calculations']['pnl']['request']['operation']['input']
    holdings, before, calculations, risks = [], [], {}, {}
    templates = copy.deepcopy(data['holdings'])
    positions = copy.deepcopy(data['before']['point']['positions'])
    for lot in range(lots):
        names = {'bench.lot.0.': f'bench.lot.{lot}.'}
        for index, holding in enumerate(templates):
            current = benchmark.renamed(holding, names)
            identity, name = current['id'], 'risk.'+str(len(holdings))
            holdings.append(current)
            before.append(benchmark.renamed(positions[index], names))
            calculations[name] = benchmark.renamed(value['calculations']['risk.'+str(index)], names)
            risks[identity] = name
    data['holdings'] = holdings
    data['before']['point']['positions'] = before
    data['before']['holdings'] = list(range(len(holdings)))
    data['steps'][0]['after'] = copy.deepcopy(data['before'])
    identify(data['before'], 'capacity.before')
    identify(data['steps'][0]['after'], 'capacity.position')
    data['steps'][0]['after']['point']['positions'][1]['pending_quantity'] = '-74000'
    value['calculations'] = dict(pnl=value['calculations']['pnl'], **calculations)
    value['id'] = f'core.capacity.{lots}.{receipts}'
    value['reference_inputs']['core_capacity'] = book_capacity.PROFILE
    value = lifecycle_risk.bind_requests(value, dict(pnl='pnl', risks=risks),
        [h['id'] for h in holdings if h['product']['product']['kind'] in ('bond','swap','cds')])
    plan = copy.deepcopy(case['plan'])
    plan['definition'] = dict(id=value['id'], version=1)
    return value, plan, risks


def identify(binding, identity):
    binding['point'].update(snapshot_id=identity, manifest=dict(id=identity, version=1))
    for fx in binding['point']['fx']:
        fx['snapshot_id'] = identity


def bind(value, config):
    data = value['calculations']['pnl']['request']['operation']['input']
    return lifecycle_risk.bind_requests(value, config,
        [h['id'] for h in data['holdings'] if h['product']['product']['kind'] in ('bond','swap','cds')])


def configuration(server, value, plan, risks):
    capture, original_plan, factors = core.prepare()
    config = automatic.configuration(server, capture, original_plan, factors)
    config.update(plan=copy.deepcopy(plan), risks=risks)
    factors = value['calculations']['risk.0']['request']['operation']['input']['factors']
    problem = config['hedge']['operation']['problem']
    for constraint, factor in zip(problem['factors'], factors, strict=True):
        constraint.update(key=copy.deepcopy(factor['key']), risk_settings=copy.deepcopy(factor['settings']))
    original = {row['risk']: row for row in problem['holdings']}
    data = value['calculations']['pnl']['request']['operation']['input']
    rows = []
    for p in data['steps'][-1]['after']['point']['positions']:
        identity = p['identity']['id']
        row = copy.deepcopy(original[identity.split('.', 3)[3]])
        row.update(risk=identity, current_quantity=p['current']['quantity'], pending_quantity=p['pending_quantity'])
        rows.append(row)
    problem['holdings'], problem['expected_holdings'] = rows, list(risks)
    for row in problem['candidates']:
        row['risk'] = 'bench.lot.0.'+row['risk']
        # Explicit synthetic inventory policy for this larger supplied book;
        # trade/liquidity/lot, factor, cost and numerical limits stay unchanged.
        inventory = '30000000' if row['risk'].endswith('.mixed.bond') else '100' if row['lot'] else '10000000'
        row.update(holding_lower='-'+inventory, holding_upper=inventory)
        row['eligibility']['decision'] = dict(id='core.capacity.paper.inventory',version=1)
        row['eligibility']['reason'] = 'Explicit larger-book paper inventory capacity; no execution permission'
    config['bindings'] = []
    fixture = json.loads(benchmark.FIXTURE.read_bytes())
    for key, pins in plan['curve_sets'].items():
        for pin in pins:
            row = next(r for r in fixture['curves'] if r['curve_id']==pin['id'] and r['version']==pin['version'])
            config['bindings'].append(dict(set=key,curve_id=pin['id'],source=row['source'],follow=True,
                max_age_ms=5000,recipe_key=copy.deepcopy(row['recipe']['key'])))
    return config


def native_request(saved, name):
    saved = copy.deepcopy(saved)
    value = lifecycle_history.expand(saved['content']['definition']['definition']['definition'])
    value['reference_inputs'].pop('core_capacity', None)
    value['reference_inputs'].pop('lifecycle_history', None)
    value['reference_inputs'].pop('pnl_checkpoint', None)
    saved['content']['definition']['definition']['definition'] = value
    return rolls.native_request(saved, name)


def compare(client, ready, config, native_binary):
    native, retained = {}, []
    for name, receipt in ready['receipts'].items():
        saved = client.snapshot(receipt['snapshot_id'])
        request = native_request(saved, name)
        expected = subprocess.check_output([str(native_binary)], input=json.dumps(request).encode())
        output = client.result(receipt, raw=True)
        financial = json.loads(output)
        financial['result']['data'].pop('continuity', None)
        require(json.loads(expected)==financial, 'Large-book native parity: '+name)
        retained.append((receipt, output))
        native[name] = json.loads(output)['result']['data']
    report = native[config['pnl']]['report']
    for identity, name in config['risks'].items():
        result = native[name]
        require(result['vector']['id']==identity and result['input']['snapshot_id']==ready['snapshot_id'], 'Mixed snapshot risk')
        if 'lifecycle' in result:
            state = lifecycle_risk.checked_state(result, report)
            require(state in native['hedge']['lifecycle_risk'], 'Hedge lost a distinct owned lot state')
    solved = native['hedge']
    require(solved['status']=='feasible' and solved['proposal'] is not None
        and all(c['satisfied'] for c in solved['proposal']['constraints']), 'Native hedge not feasible')
    # Use the existing raw arithmetic budget; no chosen financial tolerance.
    for total in report['reporting_totals']:
        rolls.issuer_example.check_attribution(total['attribution'])
    return retained, native


def worker(feed, client, folder, cutoff):
    class ObservedWorker(automatic.WorkerProcess):
        def wait(self, *args, **kwargs):
            try:
                return super().wait(*args, **kwargs)
            except RuntimeError as error:
                raise RuntimeError(str(error)+'; states='+json.dumps(self.history[-8:])) from None
    process = ObservedWorker(feed, client.root, folder, cutoff)
    process.command[process.command.index('--timeout')+1] = '60'
    return process


def payment_update(value, config):
    value = copy.deepcopy(value)
    data = value['calculations']['pnl']['request']['operation']['input']
    after = copy.deepcopy(data['steps'][-1]['after'])
    holding = copy.deepcopy(data['holdings'][after['holdings'][0]])
    life = holding['lifecycle']
    life['events'].append(rolls.event(holding['id'], dict(kind='payment', terms=dict(
        receipt_id='capacity.cash.receipt', obligation=dict(kind='bond_coupon',effective_date='2026-10-08',ordinal=0),
        amount=dict(value='-1',currency='USD',unit='currency'),value_date='2026-10-09',source='capacity.actual.partial.receipt')),
        'capacity.cash.event',len(life['events']),'2026-10-09',after['point']['known_at']))
    after['holdings'][0] = len(data['holdings'])
    data['holdings'].append(holding)
    identify(after, 'capacity.cash')
    data['steps'].append(dict(cause='event',after=after))
    data['standalone'].append(dict(kind='unavailable',reason='Actual partial cash receipt'))
    return bind(value, config)


def checkpoint(value, config, saved, receipt):
    value, config = copy.deepcopy(value), copy.deepcopy(config)
    data = value['calculations']['pnl']['request']['operation']['input']
    close = copy.deepcopy(data['steps'][-1]['after'])
    data['holdings'] = [data['holdings'][i] for i in close['holdings']]
    close.update(holdings=list(range(len(data['holdings']))),market=0)
    close['point']['fills'] = []
    data['opening_quantities'] = data.get('opening_quantities') or [p['current'] for p in data['before']['point']['positions']]
    data['before'] = close
    after = copy.deepcopy(close)
    identify(after, 'capacity.checkpoint.after')
    after['point']['positions'][1]['pending_quantity'] = '-73000'
    data['steps'] = [dict(cause='position', after=after)]
    data['standalone'] = [dict(kind='unavailable',reason='Next bounded position window')]
    value['reference_inputs']['pnl_checkpoint'] = dict(snapshot_id=saved['id'],calculation='pnl',build_id=receipt['build_id'])
    config['plan'] = copy.deepcopy(saved['content']['plan'])
    config['plan']['definition']['version'] += 1
    return lifecycle_history.compact(bind(value, config)), config


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary-dir', type=Path)
    parser.add_argument('--lots', type=int, default=6)
    parser.add_argument('--receipts', type=int, default=12)
    args = parser.parse_args()
    directory = args.binary_dir
    if directory is None:
        subprocess.run(['cargo','build','--locked','-p','convex-server','--bins'],cwd=ROOT,check=True)
        subprocess.run(['cargo','build','--locked','-p','convex','--example','book_capacity_native'],cwd=ROOT,check=True)
        directory = Path(json.loads(subprocess.check_output(['cargo','metadata','--locked','--no-deps','--format-version','1'],cwd=ROOT))['target_directory'])/'debug'
    binary, importer = [(directory/n).resolve(strict=True) for n in ('convex-server','convex-core-inputs')]
    native_binary = (directory/'examples/book_capacity_native').resolve(strict=True)
    fixture = json.loads(benchmark.FIXTURE.read_bytes())
    value, plan, risks = workload(args.lots, args.receipts)
    records = [transport.item('curve',r) for r in fixture['curves']] + [transport.item('fixings',fixture['fixings'])]
    retained, sizes = [], []
    with tempfile.TemporaryDirectory(prefix='convex-core-capacity-') as name, books.BookFeed(records) as feed:
        folder = Path(name)
        def publish(command, sources=False):
            path = folder/'capture.json'
            path.write_text(json.dumps(dict(schema_version=1,references=[dict(recorded_at=plan['known_at'],command=command)],
                curves=fixture['curves'] if sources else [],fixings=fixture['fixings'] if sources else [])))
            subprocess.run([str(importer),str(folder/'state.redb'),str(path)],check=True,cwd=ROOT,stdout=subprocess.PIPE)
            path.unlink()
        command = dict(idempotency_key='capacity.initial',changes=[],master_changes=[dict(expected_version=None,definition=dict(kind='calculation',definition=value))])
        publish(command, True)
        with core.supplied.ServerProcess(binary, folder) as server:
            config = configuration(server, value, plan, risks)
        current = dict(id='core.capacity.book',version=1,reference_commit=command['idempotency_key'],workflow=config,position_step=0)
        feed.retain_book(transport.item('book',current))
        selection = transport.universe(records);selection['book']=current['id']
        (folder/'subscription.json').write_text(json.dumps(selection))
        (folder/'workflow.json').write_text(json.dumps(config))
        with core.supplied.ServerProcess(binary,folder) as server:
            client = core.Client(server)
            with worker(feed,client,folder,plan['known_at']) as process:
                initial = process.wait('ready')
                rows, first = compare(client,initial,config,native_binary);retained.extend(rows)
                sizes.append(len(json.dumps(client.snapshot(initial['snapshot_id']),separators=(',',':'))))
                changed = copy.deepcopy(next(r for r in fixture['curves'] if r['curve_id']=='D.OIS' and r['version']==4))
                changed['version'] = 5
                changed['recipe']['source'] = copy.deepcopy(next(r for r in fixture['curves'] if r['curve_id']=='D.OIS' and r['version']==1)['recipe']['source'])
                feed.send(transport.item('curve',changed))
                ready = process.wait('ready')
                rows, revised = compare(client,ready,config,native_binary);retained.extend(rows)
                require(ready['snapshot_id']!=initial['snapshot_id'], 'Market update reused old inputs')
                # The bond is matured: its fresh risk is correctly zero. Check
                # the still-live IRS, whose discount input actually changed.
                require(first['risk.1']['vector']['sensitivities'][0]['base_pv']!=revised['risk.1']['vector']['sensitivities'][0]['base_pv'],'Market update did not reprice the IRS')
        value = payment_update(value,config)
        next_config = copy.deepcopy(config);next_config['plan']['definition']['version']+=1
        batch = dict(schema_version=1,id='capacity.cash',base=current,definition=value,workflow=next_config)
        with core.supplied.ServerProcess(binary,folder) as server:
            command,_ = events.prepare(analytics.Client(core.Client(server).root,timeout=120),batch)
        publish(command)
        with core.supplied.ServerProcess(binary,folder) as server:
            client=core.Client(server)
            release=events.ingest(analytics.Client(client.root,timeout=120),batch)
            current=release['payload'];feed.retain_book(release);config=next_config
            with worker(feed,client,folder,plan['known_at']) as process:
                ready=process.wait('ready');rows,paid=compare(client,ready,config,native_binary);retained.extend(rows)
                saved=client.snapshot(ready['snapshot_id'])
                mark=paid['pnl']['report']['trades'][0]['marks'][2]
                require(Decimal(mark['recorded_cash']['value'])==-args.receipts-1,'Partial receipt lost or repeated')
        value,next_config=checkpoint(value,config,saved,ready['receipts']['pnl'])
        batch=dict(schema_version=1,id='capacity.checkpoint',base=current,base_snapshot=saved['id'],definition=value,workflow=next_config)
        with core.supplied.ServerProcess(binary,folder) as server:
            command,_=checkpoints.prepare(analytics.Client(core.Client(server).root,timeout=120),batch)
        publish(command)
        with core.supplied.ServerProcess(binary,folder) as server:
            client=core.Client(server)
            release=checkpoints.ingest(analytics.Client(client.root,timeout=120),batch);feed.retain_book(release);config=next_config
            with automatic.lost_result_ack(client.root,path_marker='/run/hedge') as (proxy,committed):
                proxy_client=copy.copy(client)
                proxy_client.root=proxy
                with worker(feed,proxy_client,folder,plan['known_at']) as process:
                    require(committed.wait(30),'No durable hedge before ACK loss')
                    process.process.kill();process.process.wait(timeout=5)
        with core.supplied.ServerProcess(binary,folder) as server:
            client=core.Client(server)
            with worker(feed,client,folder,plan['known_at']) as process:
                process.wait('unavailable');ready=process.wait('ready')
                require(all(r['reused'] for r in ready['receipts'].values()),'Checkpoint restart did not reuse durable results')
                rows,final=compare(client,ready,config,native_binary);retained.extend(rows)
                require(ready['pnl_continuity']['completed_causes']==3,'P&L continuity lost')
                for receipt,raw in retained:
                    require(client.result(receipt,raw=True)==raw,'Old output changed')
                for name,receipt in ready['receipts'].items():
                    again=client.run(receipt['snapshot_id'],name,recompute=True)
                    require(client.result(again,raw=True)==client.result(receipt,raw=True),'Saved-input recompute changed')
    print(json.dumps(dict(status='passed',data_quality='synthetic historical replay',trades=6*args.lots,
        calculations=6*args.lots+1,receipts_per_bond=args.receipts,profile=book_capacity.PROFILE,
        persisted_results=len(retained),initial_snapshot_bytes=sizes[0],native_parity=True,market_repriced=True,
        partial_receipt=True,checkpoint_continuity=True,post_commit_pre_ack_kill_recovered=True,
        restart_bytes_cache_recompute_identical=True,result_digests={r['result_id']:hashlib.sha256(raw).hexdigest() for r,raw in retained},
        execution_authority='none',processes_stopped=True),indent=2))


if __name__=='__main__':
    main()
