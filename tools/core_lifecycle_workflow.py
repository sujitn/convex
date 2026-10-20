#!/usr/bin/env python3
"""Native partial-termination/claim risk, hedging and P&L through existing ports."""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile

import book_events as events
import core_book_workflow as books
import core_event_workflow as event_example
import core_live_workflow as automatic
import core_snapshot_workflow as core
import core_streaming_workflow as transport
import lifecycle_risk
import market_data_adapter as adapter
import streaming_analytics as analytics

ROOT, require = core.ROOT, core.require
TRADE = 'seasoned-OIS'
LIFECYCLE = ['mixed.bond', 'stepped-amortizing-IRS', TRADE, 'graph-hedge.CDS']


def termination(cash='1000', due='2026-10-07'):
    return dict(event=dict(event_id='core.lifecycle.termination.1', trade_id=TRADE, expected_revision=1,
        effective_date='2026-10-07', known_at='2026-10-07T12:00:00Z', action=dict(kind='terminate', terms=dict(
            fraction='0.5', cash=dict(value=cash, currency='USD', unit='currency'), cash_date=due,
            agreement=dict(id='core.synthetic.termination-agreement', version=1)))),
        credit_rounding=None, futures=None, delivery=None)


def receipt(revision, cash):
    return dict(event=dict(event_id='core.lifecycle.receipt.' + str(revision), trade_id=TRADE, expected_revision=revision,
        effective_date='2026-10-07', known_at='2026-10-07T12:00:00Z', action=dict(kind='payment', terms=dict(
            receipt_id='core.lifecycle.cash.' + str(revision), obligation=dict(kind='termination', effective_date='2026-10-07', ordinal=0),
            amount=dict(value=cash, currency='USD', unit='currency'), value_date='2026-10-07', source='core.synthetic.actual-cash'))),
        credit_rounding=None, futures=None, delivery=None)


def append_event(previous, step, identity):
    value = copy.deepcopy(previous)
    data = value['calculations']['pnl']['request']['operation']['input']
    def append(binding, suffix):
        point = copy.deepcopy(binding)
        at = next(i for i, n in enumerate(point['holdings']) if data['holdings'][n]['id'] == TRADE)
        holding = copy.deepcopy(data['holdings'][point['holdings'][at]])
        holding['lifecycle']['events'].append(copy.deepcopy(step))
        point['holdings'][at] = len(data['holdings'])
        data['holdings'].append(holding)
        point['point']['snapshot_id'] = identity + suffix
        point['point']['manifest'] = dict(id=identity + suffix, version=1)
        for fx in point['point']['fx']:
            fx['snapshot_id'] = point['point']['snapshot_id']
        for position in point['point']['positions']:
            for key in ('economics', 'calculation'):
                position['identity'][key] = dict(id=identity + suffix + '.' + position['identity']['id'] + '.' + key, version=1)
        return point
    data['steps'].append(dict(cause='event', after=append(data['steps'][-1]['after'], '.ordered')))
    if step['event']['action']['kind'] == 'payment':
        data['standalone'].append(dict(kind='unavailable', reason='The actual termination agreement/proposal is required before its cash receipt'))
    else:
        data['standalone'].append(dict(kind='available', information=append(data['before'], '.standalone')))
    close = data['steps'][-1]['after']
    for position, index in zip(close['point']['positions'], close['holdings'], strict=True):
        calc = next(c for c in value['calculations'].values() if c['request']['operation']['kind'] in ('graph_hedge_risk', 'lifecycle_hedge_risk')
                    and c['request']['operation']['input']['identity']['id'] == position['identity']['id'])
        operation = calc['request']['operation']
        operation['product'] = copy.deepcopy(data['holdings'][index]['product']['product'])
        operation['input']['identity'] = copy.deepcopy(position['identity'])
        operation['input']['reference_position'] = copy.deepcopy(position['current'])
        operation['input']['reference_position']['quantity'] = position['current']['reference_quantity']
    return value


def native_request(saved, name):
    """Reuse the pinned parity materializer; restore the complete operation tag."""
    value = copy.deepcopy(saved)
    operation = value['content']['definition']['definition']['definition']['calculations'][name]['request']['operation']
    lifecycle = operation['kind'] == 'lifecycle_hedge_risk'
    if lifecycle:
        operation['kind'] = 'graph_hedge_risk'
    request = core.native_request(value, name)
    if lifecycle:
        request['operation']['kind'] = 'lifecycle_hedge_risk'
    return request


def compare(client, event):
    native, saved = {}, []
    for calc, row in event['receipts'].items():
        result = client.result(row)
        require(client.server.call(native_request(client.snapshot(row['snapshot_id']), calc)) == result, 'Lifecycle native parity failed')
        native[calc] = result
        saved.append((row, client.result(row, raw=True)))
    report = native['pnl']['result']['data']['report']
    for total in report['reporting_totals']:
        r = total['attribution']['residual']
        require(all(x['value'] == 0 for x in [r['cash'], r['current_model_pnl'], *r['marking'].values()]), 'Lifecycle P&L did not reconcile')
    hedge = native['hedge']['result']['data']
    require(hedge['status'] == 'feasible' and all(c['satisfied'] for c in hedge['proposal']['constraints']), 'Lifecycle hedge constraints failed')
    states = []
    for calc, result in native.items():
        data = result['result']['data']
        if calc.startswith('risk.') and 'lifecycle' in data:
            states.append(lifecycle_risk.checked_state(data, report))
    require(sorted(hedge['lifecycle_risk'], key=lambda s: s['position']['identity']['id'])
            == sorted(states, key=lambda s: s['position']['identity']['id']), 'Hedge did not use exact native state risk')
    return saved, native


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary-dir', type=Path)
    args = parser.parse_args()
    directory = args.binary_dir
    if directory is None:
        subprocess.run(['cargo', 'build', '--locked', '-p', 'convex-server', '--bins'], cwd=ROOT, check=True)
        meta = json.loads(subprocess.check_output(['cargo', 'metadata', '--locked', '--no-deps', '--format-version', '1'], cwd=ROOT))
        directory = Path(meta['target_directory']) / 'debug'
    binary, importer = [(directory / n).resolve(strict=True) for n in ('convex-server', 'convex-core-inputs')]
    capture, plan, factors = core.prepare()
    records = [transport.item('curve', r) for r in capture['curves']] + [transport.item('fixings', capture['fixings'])]
    with tempfile.TemporaryDirectory(prefix='convex-core-lifecycle-') as name, books.BookFeed(records) as feed:
        folder = Path(name)
        def import_at_cutoff(commands):
            path = folder / 'captured.json'
            path.write_text(json.dumps(dict(schema_version=1, references=[dict(recorded_at=plan['known_at'], command=c) for c in commands], curves=[], fixings=[])))
            subprocess.run([str(importer), str(folder / 'state.redb'), str(path)], cwd=ROOT, check=True, stdout=subprocess.PIPE)
            path.unlink()
        import_at_cutoff([r['command'] for r in capture['references']])
        with core.supplied.ServerProcess(binary, folder) as server:
            config = automatic.configuration(server, capture, plan, factors)
        config['plan']['definition']['version'] = 3
        definition = lifecycle_risk.bind_requests(books.definition(capture, factors, 0), config, LIFECYCLE)
        command = dict(idempotency_key='core.lifecycle.initial', changes=[], master_changes=[dict(expected_version=2,
            definition=dict(kind='calculation', definition=definition))])
        import_at_cutoff([command])
        current = dict(id=books.BOOK, version=1, reference_commit=command['idempotency_key'], workflow=config, position_step=0)
        feed.retain_book(transport.item('book', current))
        selection = transport.universe(records)
        selection['book'] = books.BOOK
        (folder / 'subscription.json').write_text(json.dumps(selection))
        (folder / 'workflow.json').write_text(json.dumps(config))
        saved, reports = [], []
        with core.supplied.ServerProcess(binary, folder) as server, automatic.WorkerProcess(feed, core.Client(server).root, folder, plan['known_at']) as worker:
            ready = worker.wait('ready')
            more, native = compare(core.Client(server), ready)
            saved.extend(more)
            reports.append(native)
        for step, identity in [(termination(), 'partial-termination'), (receipt(2, '400'), 'partial-receipt'), (receipt(3, '600'), 'complete-receipt')]:
            definition = lifecycle_risk.bind_requests(append_event(definition, step, 'core.lifecycle.' + identity), config, LIFECYCLE)
            proposal = event_example.batch(current, definition, identity)
            path = folder / 'batch.json'
            path.write_text(json.dumps(proposal))
            with core.supplied.ServerProcess(binary, folder) as server:
                prepared = events.prepare(analytics.Client(core.Client(server).root), proposal)
            import_at_cutoff([prepared[0]])
            with core.supplied.ServerProcess(binary, folder) as server:
                client = core.Client(server)
                output = json.loads(subprocess.check_output([sys.executable, str(ROOT / 'tools/book_events.py'), str(path), '--url', client.root, '--timeout', '120'], cwd=ROOT))
                require(output == events.ingest(analytics.Client(client.root, timeout=120), proposal), 'Lifecycle retry changed release')
                if identity == 'partial-termination':
                    feed.retain_book(output)
                    with automatic.lost_result_ack(client.root, path_marker='/run/hedge') as (proxy, committed), automatic.WorkerProcess(feed, proxy, folder, plan['known_at']) as worker:
                        require(committed.wait(30), 'Lifecycle hedge did not reach durable result before ACK')
                        worker.process.kill()
                        worker.process.wait(timeout=5)
                with automatic.WorkerProcess(feed, client.root, folder, plan['known_at']) as worker:
                    ready = worker.wait('ready')
                    if identity != 'partial-termination':
                        require(ready['book']['version'] == current['version'], 'Unreleased lifecycle input became active')
                        feed.send(output)
                        ready = worker.wait('ready')
                    else:
                        require(all(r['reused'] for r in ready['receipts'].values()), 'Interrupted lifecycle results did not recover')
                    current = output['payload']
                    more, native = compare(client, ready)
                    saved.extend(more)
                    reports.append(native)
                    mark = event_example.closing_mark(native, TRADE)
                    require(mark['state']['remaining_fraction'] == '0.5', 'Partial exposure disappeared')
                    obligation = next(o for o in mark['state']['obligations'] if o['key']['kind'] == 'termination')
                    require(obligation['settled'] == (identity == 'complete-receipt'), 'Cash settlement state wrong')
                    if identity == 'partial-termination':
                        for key in ('current', 'pending'):
                            require(native['hedge']['result']['data']['exposure'][key] != reports[0]['hedge']['result']['data']['exposure'][key], 'Termination did not change ' + key + ' risk')
                        require(native['hedge']['result']['data']['proposal']['quantities']
                                != reports[0]['hedge']['result']['data']['proposal']['quantities'], 'Termination did not update hedge sizes')
                        for calc in config['risks'].values():
                            def fresh(report):
                                return [{k: v for k, v in s.items() if k != 'snapshot_id'}
                                        for s in report[calc]['result']['data']['vector']['sensitivities']]
                            require(fresh(native) == fresh(reports[0]), 'Termination changed fresh candidate risk')
                    else:
                        require(mark['recorded_cash'] != event_example.closing_mark(reports[-2], TRADE)['recorded_cash'], 'Receipt missing from P&L')
            path.unlink()
        with core.supplied.ServerProcess(binary, folder) as server, automatic.WorkerProcess(feed, core.Client(server).root, folder, plan['known_at']) as worker:
            client = core.Client(server)
            worker.wait('unavailable')
            recovered = worker.wait('ready')
            require(recovered['snapshot_id'] == ready['snapshot_id'] and all(r['reused'] for r in recovered['receipts'].values()), 'Restart selected another state')
            for row, raw in saved:
                require(client.result(row, raw=True) == raw, 'Restart changed lifecycle bytes')
            for calc in ('risk.2', 'pnl', 'hedge'):
                row = recovered['receipts'][calc]
                again = client.run(row['snapshot_id'], calc, recompute=True)
                require(client.result(row, raw=True) == client.result(again, raw=True), 'Saved lifecycle recomputation changed')
    print(json.dumps(dict(status='passed', data_quality='synthetic historical replay', trades=6, factors=5,
        batches=3, persisted_results=len(saved), partial_termination=True, outstanding_claim_risk=True,
        current_pending_and_fresh_candidate_risk_separate=True, partial_complete_cash_reconciled=True,
        native_parity=True, causal_pnl_reconciled=True, constrained_hedge=True,
        post_commit_hedge_ack_kill_recovered=True, server_restart_bytes_cache_recompute_identical=True,
        result_digests={row['result_id']: hashlib.sha256(raw).hexdigest() for row, raw in saved},
        execution_authority='none', processes_stopped=True), indent=2))


if __name__ == '__main__':
    try:
        main()
    except (RuntimeError, OSError, ValueError, KeyError, IndexError, StopIteration, subprocess.SubprocessError,
            adapter.IngestionError, analytics.Blocked) as error:
        print(str(error), file=sys.stderr)
        raise SystemExit(1)
