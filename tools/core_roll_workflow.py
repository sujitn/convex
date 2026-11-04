#!/usr/bin/env python3
"""Date rolls, scheduled bond cash and maturity using the existing server/ports."""
import argparse
import copy
from decimal import Decimal
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile

import book_events as events
import book_rolls as rolls
import core_book_workflow as books
import core_cds_workflow as cds
import core_event_workflow as event_example
import core_issuer_workflow as issuer_example
import core_lifecycle_workflow as life_example
import core_live_workflow as automatic
import core_snapshot_workflow as core
import core_streaming_workflow as transport
import lifecycle_risk
import market_data_adapter as adapter
import streaming_analytics as analytics

ROOT, require = core.ROOT, core.require
BOND = 'mixed.bond'
LIFECYCLE = life_example.LIFECYCLE


def bindings(data):
    return [data['before'], *[s['after'] for s in data['steps']],
            *[s['information'] for s in data['standalone'] if s['kind'] == 'available']]


def sync_risks(value, config):
    data = value['calculations']['pnl']['request']['operation']['input']
    close = data['steps'][-1]['after']
    for position, index in zip(close['point']['positions'], close['holdings'], strict=True):
        calc = value['calculations'][config['risks'][position['identity']['id']]]
        operation = calc['request']['operation']
        operation['product'] = copy.deepcopy(data['holdings'][index]['product']['product'])
        operation['input']['identity'] = copy.deepcopy(position['identity'])
        operation['input']['reference_position'] = copy.deepcopy(position['current'])
        operation['input']['reference_position']['quantity'] = position['current']['reference_quantity']
        calc['curve_sets'] = [value['calculations']['pnl']['curve_sets'][close['market']]]
    return lifecycle_risk.bind_requests(value, config, LIFECYCLE)


def definition(capture, factors, config):
    """A distinct explicit short stepped contract; original fixtures stay intact."""
    value = cds.definition(capture, factors)
    data = value['calculations']['pnl']['request']['operation']['input']
    for holding in data['holdings']:
        if holding['id'] == cds.TRADE:
            holding['product']['product']['context']['cash_settlement_date']='2026-10-15'
    # A separate explicit short OIS keeps the bounded replay's full daily
    # diagnostics small; no original contract or result limit is changed.
    for holding in data['holdings']:
        if holding['id']!='seasoned-OIS':
            continue
        terms=holding['product']['product']['terms']
        terms.update(version=2,maturity_date='2026-10-28')
        for side in ('fixed','floating'):
            leg=terms[side]['leg']
            leg['schedule'].update(end_date='2026-10-28',stub_type='ShortLast')
            leg['notionals']=[dict(start='2026-09-16',end='2026-10-28',value='1000000')]
        terms['fixed']['rates']=[dict(start='2026-09-16',end='2026-10-28',value=terms['fixed']['rates'][0]['value'])]
        terms['floating']['profiles']=dict(gearings=[dict(start='2026-09-16',end='2026-10-28',value='1')],
            spreads=[dict(start='2026-09-16',end='2026-10-28',value='0.0002')])
        holding['lifecycle']['instrument']['terms']=copy.deepcopy(terms)
        holding['lifecycle']['version']=2
        holding['lifecycle']['events']=[]
    for binding in bindings(data):
        for position in binding['point']['positions']:
            if position['identity']['id']=='seasoned-OIS':
                position['identity']['instrument']['version']=2
    for holding in data['holdings']:
        if holding['id'] != BOND:
            continue
        terms = holding['product']['product']['bond']['terms']
        terms.update(face_value='1000', issue_date='2026-09-08', rounding=dict(NearestEven=2))
        terms['schedule'].update(start_date='2026-09-08', end_date='2026-10-09', frequency='Monthly')
        terms['dates']['payment_delay'] = dict(CalendarDays=1)
        terms['calendars'] = {k:'WEEKEND' for k in terms['calendars']}
        holding['product']['product']['bond']['calendars'] = {k:dict(id='WEEKEND',version=1) for k in holding['product']['product']['bond']['calendars']}
        for key, rate in [('coupon', '0.036'), ('gearing', '0'), ('spread', '0')]:
            terms['profiles'][key] = [dict(start='2026-09-08', end='2026-10-09', value=rate)]
        terms['profiles']['notional'] = [dict(start='2026-09-08', end='2026-10-08', value='1000'),
                                         dict(start='2026-10-08', end='2026-10-09', value='600')]
        holding['lifecycle']['instrument']['bond'] = copy.deepcopy(holding['product']['product']['bond'])
        holding['lifecycle']['events'] = []
        holding['lifecycle']['position']['value'] = '-1000'
    for binding in bindings(data):
        position = next(p for p in binding['point']['positions'] if p['identity']['id'] == BOND)
        position['current'].update(quantity='-1000', reference_quantity='1000')
        position['identity']['instrument'] = dict(id='core.roll.supplied.bond', version=1)
    return sync_risks(value, config)


def native_observation(record):
    return dict(index_id=record['key']['index_id'], observation_date=record['key']['observation_date'],
        source_id=record['key']['source'], revision=record['version'], rate=record['rate']['value'],
        published_at=record['published_at'], known_at=record['recorded_at'], status='Final')


def fixing(observation, published):
    return dict(schema_version=1, key=dict(index_id='USD.SOFR', observation_date=observation,
        source='engineering-CVX057'), version=1, rate=dict(value='0.0383', unit='decimal_rate'),
        published_at=published+'T08:00:00Z', recorded_at=published+'T08:00:00Z', is_preliminary=False,
        entitled=True, evidence_reference='Explicit synthetic daily published observation; no live data claim')


def event(trade, action, identity, revision, effective, known):
    return dict(event=dict(event_id=identity, trade_id=trade, expected_revision=revision,
        effective_date=effective, known_at=known, action=action), credit_rounding=None, futures=None, delivery=None)


def context(product, day, known):
    if product['kind'] == 'bond':
        product['context']['settlement'] = day
        product['context']['fixing'].update(valuation_date=day, known_at=known)
    elif product['kind'] == 'swap':
        product['context']['fixings'].update(valuation_date=day, known_at=known)
    elif product['kind'] == 'cds':
        # These are supplied quotation dates, not changed contractual cash terms.
        product['context'].update(valuation_date=day, step_in_date={
            '2026-10-07':'2026-10-08', '2026-10-08':'2026-10-09',
            '2026-10-09':'2026-10-12', '2026-10-13':'2026-10-14'}[day],
            cash_settlement_date='2026-10-15')
    else:
        product['input']['context'].update(as_of=day+'T12:00:00Z', known_at=known)


def append_phase(value, cause, identity, *, day=None, known=None, observation=None, scheduled=False,
                 cancel=False, market=None, recipe_version=None, payments=()):
    data = value['calculations']['pnl']['request']['operation']['input']
    close = copy.deepcopy(data['steps'][-1]['after'])
    day = day or close['point']['valuation_date']
    known = known or close['point']['known_at']
    old_day = close['point']['valuation_date']
    close['point'].update(valuation_date=day, known_at=known, snapshot_id=identity,
                          manifest=dict(id=identity, version=1))
    for fx in close['point']['fx']:
        fx['snapshot_id'] = identity
    def repin(item):
        if isinstance(item, dict):
            if set(item) == {'id', 'version'} and item['id'] in {'D.OIS','P.3M','P.SOFR','H.ACME','B.ACME','P.OPTIONAL'}:
                item['version'] = str(recipe_version)
            else:
                for child in item.values():
                    repin(child)
        elif isinstance(item, list):
            for child in item:
                repin(child)
    for at, index in enumerate(close['holdings']):
        holding = copy.deepcopy(data['holdings'][index])
        trade = holding['id']
        context(holding['product']['product'], day, known)
        if recipe_version is not None:
            repin(holding)
            product=holding['product']['product']
            if product['kind']=='rate_future':
                product['input']['projection']['stamp'].update(version=recipe_version,source_at=known,available_at=known)
        life = holding['lifecycle']
        if observation is not None and trade in ('seasoned-OIS', 'mixed.overnight'):
            life['events'].append(event(trade, dict(kind='fixing', terms=native_observation(observation)),
                identity+'.'+trade, len(life['events']), old_day, known))
        if scheduled and trade == BOND:
            for kind in ('contract_step', 'contractual_cash'):
                life['events'].append(event(trade, dict(kind=kind), identity+'.'+kind,
                    len(life['events']), day, known))
        if trade == BOND:
            for payment in payments:
                life['events'].append(event(trade, dict(kind='payment', terms=payment),
                    payment['receipt_id']+'.event', len(life['events']), day, known))
        if holding != data['holdings'][index]:
            close['holdings'][at] = len(data['holdings'])
            data['holdings'].append(holding)
    for position in close['point']['positions']:
        if cancel and position['identity']['id'] == BOND:
            position['pending_quantity'] = '0'
        if recipe_version is not None:
            repin(position['cash_discount'])
        for key in ('economics','calculation'):
            position['identity'][key] = dict(id=identity+'.'+position['identity']['id']+'.'+key,version=1)
    if market is not None:
        close['market'] = market
    data['steps'].append(dict(cause=cause,after=close))
    data['standalone'].append(dict(kind='unavailable',reason='This roll requires the actual preceding market, fixing and contractual claim history'))


def roll(previous, config, saved_plan, records, day, observation, *, cancel=False):
    value, cfg = copy.deepcopy(previous), copy.deepcopy(config)
    data = value['calculations']['pnl']['request']['operation']['input']
    at, version = day+'T12:00:00Z', max(r['version'] for r in records)+1
    if cancel:
        append_phase(value,'position','core.roll.cancel.'+day,cancel=True)
    append_phase(value,'fixing','core.roll.fixing.'+day,known=at,observation=observation)
    append_phase(value,'time','core.roll.time.'+day,day=day,known=at,scheduled=day in ('2026-10-08','2026-10-09'))
    current = [r for r in records if r['version'] == version-1]
    fresh = copy.deepcopy(current)
    for record in fresh:
        record.update(version=version,source_time=at,recorded_at=at)
        record['recipe']['key']['version'] = str(version)
        record['recipe']['reference_date'] = day
    market = len(data['markets'])
    data['markets'].append(dict(curves=[],calibration=None))
    key = 'market.'+str(market)
    value['calculations']['pnl']['curve_sets'].append(key)
    append_phase(value,'market','core.roll.market.'+day,market=market,recipe_version=version)
    cfg['plan'] = copy.deepcopy(saved_plan)
    cfg['plan'].update(as_of=at,known_at=at)
    cfg['plan']['definition']['version'] += 1
    cfg['plan']['fixings'].append(copy.deepcopy(observation['key']))
    cfg['plan']['curve_sets'][key] = [dict(id=r['curve_id'],version=r['version']) for r in fresh]
    for binding in cfg['bindings']:
        binding['follow'] = False
    for record in fresh:
        cfg['bindings'].append(dict(set=key,curve_id=record['curve_id'],source=record['source'],
            recipe_key=copy.deepcopy(record['recipe']['key']),follow=True,max_age_ms=5000))
    for calc in value['calculations'].values():
        if calc['request']['operation']['kind'] in ('graph_hedge_risk','lifecycle_hedge_risk'):
            for factor in calc['request']['operation']['input']['factors']:
                for pin in factor['key']['coordinate'].values():
                    if isinstance(pin,dict) and set(pin)=={'id','version'}:
                        pin['version']=str(version)
    for factor in cfg['hedge']['operation']['problem']['factors']:
        for pin in factor['key']['coordinate'].values():
            if isinstance(pin,dict) and set(pin)=={'id','version'}:
                pin['version']=str(version)
    return sync_risks(value,cfg),cfg,fresh


def receipt(identity,kind,day,ordinal,amount):
    return dict(receipt_id=identity,obligation=dict(kind=kind,effective_date=day,ordinal=ordinal),
        amount=dict(value=amount,currency='USD',unit='currency'),value_date='2026-10-13',source='core.synthetic.actual-cash')


def native_request(saved, calc):
    """Bind each P&L point to eligible captured fixing revisions, like the port owner."""
    request=life_example.native_request(saved,calc)
    if request['operation']['kind']=='book_pnl':
        data=request['operation']['input']
        points=bindings(data)
        cutoff=min(analytics.timestamp(b['point']['known_at']) for b in points)
        observations=data['resources']['fixings']
        def eligible(row,at):
            return max(analytics.timestamp(row['known_at']),analytics.timestamp(row['published_at']))<=at
        data['resources']['fixings']=[r for r in observations if eligible(r,cutoff)]
        for binding in points:
            at=analytics.timestamp(binding['point']['known_at'])
            binding['additional_fixings']=[r for r in observations if not eligible(r,cutoff) and eligible(r,at)]
    return request


def compare(client, ready, config):
    saved,native=[],{}
    for calc,row in ready['receipts'].items():
        result=client.result(row)
        require(client.server.call(native_request(client.snapshot(row['snapshot_id']),calc))==result,'Date-roll native parity failed')
        native[calc]=result
        saved.append((row,client.result(row,raw=True)))
    report=native['pnl']['result']['data']['report']
    for total in report['reporting_totals']:
        issuer_example.check_attribution(total['attribution'])
    hedge=native['hedge']['result']['data']
    require(hedge['status'] in ('feasible','ill_conditioned'),'Unexpected date-roll hedge outcome')
    if hedge['status']=='feasible':
        require(all(c['satisfied'] for c in hedge['proposal']['constraints']),'Date-roll hedge constraint failed')
    else:
        require(hedge['proposal'] is None and hedge['rank']['condition']>1e4,'Conditioning guard did not withhold proposal')
    for trade in LIFECYCLE:
        calc=config['risks'][trade]
        risk=native[calc]['result']['data']
        operation=life_example.native_request(client.snapshot(ready['receipts'][calc]['snapshot_id']),calc)['operation']
        require(lifecycle_risk.checked_state(risk,report,operation) in hedge['lifecycle_risk'],'Hedge lost native lifecycle risk')
    return saved,native


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary-dir',type=Path)
    args=parser.parse_args()
    directory=args.binary_dir
    if directory is None:
        subprocess.run(['cargo','build','--locked','-p','convex-server','--bins'],cwd=ROOT,check=True)
        meta=json.loads(subprocess.check_output(['cargo','metadata','--locked','--no-deps','--format-version','1'],cwd=ROOT))
        directory=Path(meta['target_directory'])/'debug'
    binary,importer=[(directory/n).resolve(strict=True) for n in ('convex-server','convex-core-inputs')]
    capture,plan,factors=core.prepare()
    records=[transport.item('curve',r) for r in capture['curves']]+[transport.item('fixings',capture['fixings'])]
    with tempfile.TemporaryDirectory(prefix='convex-core-roll-') as name,books.BookFeed(records) as feed:
        folder=Path(name)
        def import_capture(references,curves=(),fixings=()):
            path=folder/'capture.json'
            path.write_text(json.dumps(dict(schema_version=1,references=references,curves=list(curves),fixings=list(fixings))))
            subprocess.run([str(importer),str(folder/'state.redb'),str(path)],cwd=ROOT,check=True,stdout=subprocess.PIPE)
            path.unlink()
        import_capture(capture['references'])
        with core.supplied.ServerProcess(binary,folder) as server:
            config=automatic.configuration(server,capture,plan,factors)
        config['plan']['definition']['version']=3
        value=definition(capture,factors,config)
        command=dict(idempotency_key='core.roll.initial',changes=[],master_changes=[dict(expected_version=2,definition=dict(kind='calculation',definition=value))])
        import_capture([dict(recorded_at=plan['known_at'],command=command)])
        current=dict(id=books.BOOK,version=1,reference_commit=command['idempotency_key'],workflow=config,position_step=0)
        feed.retain_book(transport.item('book',current))
        subscription=transport.universe(records)
        subscription.update(book=books.BOOK,follow_fixing_dates=True)
        (folder/'subscription.json').write_text(json.dumps(subscription))
        (folder/'workflow.json').write_text(json.dumps(config))
        retained,reports=[],[]
        with core.supplied.ServerProcess(binary,folder) as server,automatic.WorkerProcess(feed,core.Client(server).root,folder,plan['known_at']) as worker:
            ready=worker.wait('ready')
            more,native=compare(core.Client(server),ready,config)
            retained.extend(more);reports.append(native)
            saved=core.Client(server).snapshot(ready['snapshot_id'])
        all_curves,all_fixings=copy.deepcopy(capture['curves']),copy.deepcopy(capture['fixings'])
        for day,observed in [('2026-10-08','2026-10-07'),('2026-10-09','2026-10-08'),('2026-10-13','2026-10-09')]:
            observation=fixing(observed,day)
            value,config,fresh=roll(value,config,saved['content']['plan'],all_curves,day,observation,cancel=day=='2026-10-09')
            proposal=dict(schema_version=1,id='daily.'+day,base=copy.deepcopy(current),base_snapshot=ready['snapshot_id'],definition=value,workflow=config)
            with core.supplied.ServerProcess(binary,folder) as server:
                command,_=rolls.prepare(analytics.Client(core.Client(server).root,timeout=120),proposal)
            at=day+'T12:00:00Z'
            import_capture([dict(recorded_at=at,command=command)],fresh,[] if day=='2026-10-08' else [observation])
            if day=='2026-10-08':
                with core.supplied.ServerProcess(binary,folder) as server:
                    try:
                        rolls.ingest(analytics.Client(core.Client(server).root,timeout=120),proposal)
                    except adapter.IngestionError as error:
                        require(error.http_status==404,'Unexpected missing-fixing diagnostic')
                    else:
                        raise RuntimeError('Roll released without its newly due fixing')
                import_capture([],fixings=[observation])
            path=folder/'roll.json'
            path.write_text(json.dumps(proposal))
            with core.supplied.ServerProcess(binary,folder) as server:
                client=core.Client(server)
                output=json.loads(subprocess.check_output([sys.executable,str(ROOT/'tools/book_rolls.py'),str(path),'--url',client.root,'--timeout','120'],cwd=ROOT))
                require(output==rolls.ingest(analytics.Client(client.root,timeout=120),proposal),'Date-roll retry changed release')
                current=output['payload']
                path.unlink()
                all_curves.extend(fresh);all_fixings.append(observation)
                with feed.lock:
                    feed.records=[transport.item('curve',r) for r in all_curves]+[transport.item('fixings',all_fixings),output]
                if day=='2026-10-09':
                    with automatic.lost_result_ack(client.root,path_marker='/run/hedge') as (proxy,committed),automatic.WorkerProcess(feed,proxy,folder,at) as worker:
                        require(committed.wait(30),'Maturity hedge did not reach durable commit')
                        worker.process.kill();worker.process.wait(timeout=5)
                if day=='2026-10-13':
                    with feed.lock:
                        feed.records=[r for r in feed.records if not (r['kind']=='curve' and r['payload']['curve_id']=='D.OIS' and r['payload']['version']==fresh[0]['version'])]
                with automatic.WorkerProcess(feed,client.root,folder,at) as worker:
                    if day=='2026-10-13':
                        worker.wait('unavailable','source_version_rollback')
                        feed.send(transport.item('curve',next(r for r in fresh if r['curve_id']=='D.OIS')))
                    ready=worker.wait('ready')
                    require(ready['book']['version']==current['version'],'Wrong date-roll revision')
                    if day=='2026-10-09':
                        require(all(r['reused'] for r in ready['receipts'].values()),'Maturity pre-ACK recovery missed stored results')
                    more,native=compare(client,ready,config)
                    retained.extend(more);reports.append(native)
                    mark=event_example.closing_mark(native,BOND)
                    require(mark['recorded_cash']['value']=='0','Scheduled cash was treated as actual received cash')
                    require(mark['state']['remaining_fraction']==('1' if day=='2026-10-08' else '0'),'Wrong native remaining face')
                    expected={'2026-10-08':{'bond_coupon':['-3'],'principal':['-400']},
                              '2026-10-09':{'bond_coupon':['-3','-0.06'],'principal':['-400','-600']},
                              '2026-10-13':{'bond_coupon':['-3','-0.06'],'principal':['-400','-600']}}[day]
                    for kind,amounts in expected.items():
                        require([Decimal(o['amount']['value']) for o in mark['state']['obligations'] if o['key']['kind']==kind]==[Decimal(a) for a in amounts],'Independent scheduled cash mismatch')
                    if day!='2026-10-08':
                        candidates=native['hedge']['result']['data']['exposure']['problem']['candidates']
                        require(native['hedge']['result']['data']['status']=='feasible','Matured-book hedge is not feasible')
                        require(len(candidates)==3 and all(c['risk']['instrument']['id']!='core.roll.supplied.bond' for c in candidates),'Matured security remained a candidate')
                        require(native['risk.0']['result']['data']['valuation']=='Matured','Maturity priced a fresh security')
                    saved=client.snapshot(ready['snapshot_id'])
                with feed.lock:
                    feed.records=[transport.item('curve',r) for r in all_curves]+[transport.item('fixings',all_fixings),output]
        batches=[('partial',[receipt('core.roll.partial','principal','2026-10-08',0,'-200')]),
                 ('complete',[receipt('core.roll.remaining','principal','2026-10-08',0,'-200'),
                   receipt('core.roll.redemption','principal','2026-10-09',1,'-600'),
                   receipt('core.roll.coupon.0','bond_coupon','2026-10-08',0,'-3'),
                   receipt('core.roll.coupon.1','bond_coupon','2026-10-09',1,'-0.06')])]
        for identity,payments in batches:
            append_phase(value,'event','core.roll.receipts.'+identity,payments=payments)
            value=sync_risks(value,config)
            proposal=event_example.batch(current,value,'core.roll.cash.'+identity)
            with core.supplied.ServerProcess(binary,folder) as server:
                command,_=events.prepare(analytics.Client(core.Client(server).root),proposal)
            import_capture([dict(recorded_at='2026-10-13T12:00:00Z',command=command)])
            with core.supplied.ServerProcess(binary,folder) as server:
                client=core.Client(server)
                output=events.ingest(analytics.Client(client.root,timeout=120),proposal)
                require(output==events.ingest(analytics.Client(client.root,timeout=120),proposal),'Cash receipt retry changed release')
                current=output['payload'];config=current['workflow']
                feed.retain_book(output)
                with automatic.WorkerProcess(feed,client.root,folder,'2026-10-13T12:00:00Z') as worker:
                    ready=worker.wait('ready')
                    more,native=compare(client,ready,config)
                    retained.extend(more);reports.append(native)
                    mark=event_example.closing_mark(native,BOND)
                    require(mark['state']['status']==('matured' if identity=='partial' else 'closed'),'Incorrect cash/closure status')
                    require(mark['recorded_cash']['value']==('-200' if identity=='partial' else '-1003.06'),'Received cash did not reconcile')
        with core.supplied.ServerProcess(binary,folder) as server,automatic.WorkerProcess(feed,core.Client(server).root,folder,'2026-10-13T12:00:00Z') as worker:
            client=core.Client(server)
            worker.wait('unavailable');recovered=worker.wait('ready')
            require(recovered['snapshot_id']==ready['snapshot_id'] and all(r['reused'] for r in recovered['receipts'].values()),'Restart changed roll state')
            for row,raw in retained:
                require(client.result(row,raw=True)==raw,'Restart changed persisted result bytes')
            for calc in ('risk.0','pnl','hedge'):
                row=recovered['receipts'][calc]
                again=client.run(row['snapshot_id'],calc,recompute=True)
                require(client.result(row,raw=True)==client.result(again,raw=True),'Saved-input recompute changed')
    print(json.dumps(dict(status='passed',data_quality='synthetic historical replay',trades=6,factors=5,
        hedge_statuses=[r['hedge']['result']['data']['status'] for r in reports],
        date_rolls=3,scheduled_coupon=True,partial_amortization=True,maturity=True,actual_receipts_separate=True,
        complete_causal_pnl=True,native_parity=True,matured_candidates_excluded=True,persisted_results=len(retained),
        missing_fixing_blocks_release=True,unrefreshed_curve_blocks_roll=True,adapter_cli_and_idempotent_retry=True,
        post_commit_pre_ack_kill_recovered=True,restart_bytes_cache_recompute_identical=True,
        result_digests={row['result_id']:hashlib.sha256(raw).hexdigest() for row,raw in retained},
        execution_authority='none',processes_stopped=True),indent=2))


if __name__=='__main__':
    try:
        main()
    except (RuntimeError,OSError,ValueError,KeyError,IndexError,StopIteration,subprocess.SubprocessError,
            adapter.IngestionError,analytics.Blocked) as error:
        print(str(error),file=sys.stderr)
        raise SystemExit(1)
