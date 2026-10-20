#!/usr/bin/env python3
"""CDS default, unknown recovery, actual auction and cash through existing ports."""
import argparse
import copy
import hashlib
import json
import math
from pathlib import Path
import subprocess
import sys
import tempfile
import book_events as events
import core_book_workflow as books
import core_event_workflow as event_example
import core_lifecycle_workflow as life_example
import core_live_workflow as automatic
import core_snapshot_workflow as core
import core_streaming_workflow as transport
import lifecycle_risk
import market_data_adapter as adapter
import streaming_analytics as analytics
ROOT, require = (core.ROOT, core.require)
TRADE = 'graph-hedge.CDS'

def definition(capture, factors):
    """A distinct supplied seasoned contract, eligible for the actual default date."""
    value = books.definition(capture, factors, 0)
    data = value['calculations']['pnl']['request']['operation']['input']
    products = [h['product']['product'] for h in data['holdings'] if h['id'] == TRADE]
    products += [value['calculations']['risk.5']['request']['operation']['product']]
    terms = copy.deepcopy(products[0]['terms'])
    terms.update(version=2, trade_date='2026-10-06', protection_start_date='2026-10-07', step_in_date='2026-10-07', cash_settlement_date='2026-10-09', initial_clean_upfront_fraction=None)
    for product in products:
        curves = {k: copy.deepcopy(product['terms'][k]) for k in ('discount_curve', 'survival_curve')}
        product['terms'] = dict(copy.deepcopy(terms), **curves)
        product['context'].update(step_in_date='2026-10-08', cash_settlement_date='2026-10-09', initial_payment_state='NotContracted')
    for holding in data['holdings']:
        if holding['id'] == TRADE:
            curves = {k: copy.deepcopy(holding['lifecycle']['instrument']['terms'][k]) for k in ('discount_curve', 'survival_curve')}
            holding['lifecycle']['instrument']['terms'] = dict(copy.deepcopy(terms), **curves)
            holding['lifecycle']['version'] = 2
            holding['lifecycle']['events'] = []
    points = [data['before'], *[s['after'] for s in data['steps']], *[s['information'] for s in data['standalone'] if s['kind'] == 'available']]
    for binding in points:
        for position in binding['point']['positions']:
            if position['identity']['id'] == TRADE:
                position['identity']['instrument']['version'] = 2
    value['calculations']['risk.5']['request']['operation']['input']['identity']['instrument']['version'] = 2
    return value

def step(action, identity, revision):
    return dict(event=dict(event_id=identity, trade_id=TRADE, expected_revision=revision, effective_date='2026-10-07', known_at='2026-10-07T12:00:00Z', action=action), credit_rounding=None, futures=None, delivery=None)

def default():
    return step(dict(kind='credit_default', terms=dict(entity='ACME', seniority='SeniorUnsecured', document=dict(id='CDS.Standard.Supplied.Confirmation', version=1), accrual_day_fraction='0.5', rounding=dict(NearestEven=2))), 'core.cds.default.1', 0)

def recovery(fraction='0.35', due='2026-10-21'):
    return step(dict(kind='recovery', terms=dict(default_event='core.cds.default.1', recovery_fraction=fraction, reference=dict(id='core.synthetic.auction', version=1), cash_date=due)), 'core.cds.recovery.1', 1)

def payment(identity, revision, kind, amount):
    return step(dict(kind='payment', terms=dict(receipt_id=identity + '.cash', obligation=dict(kind=kind, effective_date='2026-10-07', ordinal=0), amount=dict(value=amount, currency='USD', unit='currency'), value_date='2026-10-07', source='core.synthetic.actual-cash')), identity, revision)

def append(previous, steps, identity, *, cancel=False):
    """Preserve actual facts, use complete supplied after points, no cash arithmetic."""
    value = copy.deepcopy(previous)
    data = value['calculations']['pnl']['request']['operation']['input']

    def point(binding, suffix):
        close = copy.deepcopy(binding)
        at = next((i for i, n in enumerate(close['holdings']) if data['holdings'][n]['id'] == TRADE))
        holding = copy.deepcopy(data['holdings'][close['holdings'][at]])
        holding['lifecycle']['events'].extend(copy.deepcopy(steps))
        close['holdings'][at] = len(data['holdings'])
        data['holdings'].append(holding)
        close['point']['snapshot_id'] = identity + suffix
        close['point']['manifest'] = dict(id=identity + suffix, version=1)
        for fx in close['point']['fx']:
            fx['snapshot_id'] = close['point']['snapshot_id']
        for position in close['point']['positions']:
            if cancel and position['identity']['id'] == TRADE:
                position['pending_quantity'] = '0'
            for key in ('economics', 'calculation'):
                position['identity'][key] = dict(id=identity + suffix + '.' + position['identity']['id'] + '.' + key, version=1)
        return close
    data['steps'].append(dict(cause='position' if cancel else 'event', after=point(data['steps'][-1]['after'], '.ordered')))
    if cancel:
        data['standalone'].append(dict(kind='available', information=point(data['before'], '.standalone')))
    else:
        data['standalone'].append(dict(kind='unavailable', reason='Actual prior CDS state and order cancellation are required'))
    close = data['steps'][-1]['after']
    for position, index in zip(close['point']['positions'], close['holdings'], strict=True):
        calc = next((c for c in value['calculations'].values() if c['request']['operation']['kind'] in ('graph_hedge_risk', 'lifecycle_hedge_risk') and c['request']['operation']['input']['identity']['id'] == position['identity']['id']))
        operation = calc['request']['operation']
        operation['product'] = copy.deepcopy(data['holdings'][index]['product']['product'])
        operation['input']['identity'] = copy.deepcopy(position['identity'])
        operation['input']['reference_position'] = copy.deepcopy(position['current'])
        operation['input']['reference_position']['quantity'] = position['current']['reference_quantity']
    return value

def resolve(previous, event):
    """Complete one unpriced event information set at actual recovery knowledge.

    The earlier pending definition stays immutable/auditable. Every prior priced
    phase and the complete original default fact are retained in the new path.
    """
    value = append(previous, [event], 'core.cds.known-recovery')
    data = value['calculations']['pnl']['request']['operation']['input']
    data['steps'].pop(-2)
    data['standalone'].pop(-2)
    bindings = [data['before'], *[s['after'] for s in data['steps']], *[s['information'] for s in data['standalone'] if s['kind'] == 'available']]
    used = sorted({i for binding in bindings for i in binding['holdings']})
    mapping = {old: new for new, old in enumerate(used)}
    data['holdings'] = [data['holdings'][i] for i in used]
    for binding in bindings:
        binding['holdings'] = [mapping[i] for i in binding['holdings']]
    return value

def wait_book(worker, version):
    for _ in range(4):
        event = worker.wait('ready')
        if event['book']['version'] == version:
            return event
    raise RuntimeError('Worker did not select the requested CDS book revision')

def compare(client, event):
    native, saved = ({}, [])
    for calc, row in event['receipts'].items():
        result = client.result(row)
        request = life_example.native_request(client.snapshot(row['snapshot_id']), calc)
        require(client.server.call(request) == result, 'CDS direct native parity failed')
        native[calc] = result
        saved.append((row, client.result(row, raw=True)))
    report = native['pnl']['result']['data']['report']
    for group in report['reporting_totals']:
        check_attribution(group['attribution'])
    hedge = native['hedge']['result']['data']
    require(hedge['status'] == 'feasible' and all((c['satisfied'] for c in hedge['proposal']['constraints'])), 'CDS hedge constraints failed')
    states = [lifecycle_risk.checked_state(result['result']['data'], report) for calc, result in native.items() if calc.startswith('risk.') and 'lifecycle' in result['result']['data']]
    order = lambda state: state['position']['identity']['id']
    require(sorted(hedge['lifecycle_risk'], key=order) == sorted(states, key=order), 'CDS hedge did not bind the exact native states')
    return (saved, native)

def check_attribution(attribution):
    """Retain native marking residuals; test the actual binary64 operation budget.

    Native endpoint differences telescope in exact arithmetic. Each rounded
    difference/addition contributes at most half an ULP of its result. This is
    an arithmetic reconstruction bound, not a financial tolerance or zero clamp.
    Actual cash and model-P&L reconciliation remain exactly zero in this example.
    """
    residual = attribution['residual']
    require(residual['cash']['value'] == 0 and residual['current_model_pnl']['value'] == 0, 'CDS cash/model P&L did not reconcile')
    for key, money in residual['marking'].items():
        total = attribution['total']['marking'][key]['value']
        running, budget = (0.0, math.ulp(total))
        for row in attribution['ordered']:
            delta = row['marking'][key]['value']
            running += delta
            budget += math.ulp(delta) + math.ulp(running)
        actual = money['value']
        budget = (budget + math.ulp(actual)) / 2
        require(math.isfinite(actual) and actual == total - running and (abs(actual) <= budget), 'CDS marking residual exceeds its native arithmetic roundoff budget')

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
    with tempfile.TemporaryDirectory(prefix='convex-core-cds-') as name, books.BookFeed(records) as feed:
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
        value = lifecycle_risk.bind_requests(definition(capture, factors), config, life_example.LIFECYCLE)
        command = dict(idempotency_key='core.cds.initial', changes=[], master_changes=[dict(expected_version=2, definition=dict(kind='calculation', definition=value))])
        import_at_cutoff([command])
        current = dict(id=books.BOOK, version=1, reference_commit=command['idempotency_key'], workflow=config, position_step=0)
        feed.retain_book(transport.item('book', current))
        selection = transport.universe(records)
        selection['book'] = books.BOOK
        (folder / 'subscription.json').write_text(json.dumps(selection))
        (folder / 'workflow.json').write_text(json.dumps(config))
        saved, audits, reports = ([], [], [])
        with core.supplied.ServerProcess(binary, folder) as server:
            client = core.Client(server)
            with automatic.WorkerProcess(feed, client.root, folder, plan['known_at']) as worker:
                worker.wait('unavailable', 'calculating')
                snapshot = client.assemble(config['plan'])
                for calc in [*config['risks'].values(), config['pnl']]:
                    receipt = client.run(snapshot, calc)
                    envelope = client.call('GET', '/analytics/results/' + receipt['result_id'])
                    if envelope.get('ok') != 'true':
                        print('CDS baseline ' + calc + ': ' + json.dumps(envelope.get('error')), file=sys.stderr)
                ready = worker.wait('ready')
                more, initial = compare(client, ready)
                saved.extend(more)
                reports.append(initial)
        value = append(value, [], 'core.cds.cancel', cancel=True)
        value = append(value, [default()], 'core.cds.default')
        value = lifecycle_risk.bind_requests(value, config, life_example.LIFECYCLE)
        batches = [('default', False), ('recovery', True), ('partial-cash', False), ('complete-cash', False)]
        for kind, resolving in batches:
            if kind == 'recovery':
                value = resolve(value, recovery(due='2026-10-07'))
            elif kind == 'partial-cash':
                value = append(value, [payment('core.cds.receipt.1', 2, 'recovery', '-100000')], 'core.cds.partial')
            elif kind == 'complete-cash':
                value = append(value, [payment('core.cds.receipt.2', 3, 'recovery', '-192500'), payment('core.cds.accrual.1', 4, 'default_accrual', '218.75')], 'core.cds.complete')
            value = lifecycle_risk.bind_requests(value, config, life_example.LIFECYCLE)
            proposal = event_example.batch(current, value, 'cds.' + kind)
            if resolving:
                proposal.update(schema_version=2, resolve_default=True)
            with core.supplied.ServerProcess(binary, folder) as server:
                command, release = events.prepare(analytics.Client(core.Client(server).root, timeout=120), proposal)
            import_at_cutoff([command])
            with core.supplied.ServerProcess(binary, folder) as server:
                client = core.Client(server)
                native_client = analytics.Client(client.root, timeout=120)
                try:
                    output = events.ingest(native_client, proposal)
                except analytics.Blocked:
                    snapshot = client.assemble(release['workflow']['plan'])
                    for calc in [*config['risks'].values(), config['pnl']]:
                        row = client.run(snapshot, calc)
                        envelope = client.call('GET', '/analytics/results/' + row['result_id'])
                        if envelope.get('ok') != 'true':
                            print('CDS ' + kind + ' ' + calc + ': ' + json.dumps(envelope.get('error')), file=sys.stderr)
                    raise
                require(output == events.ingest(native_client, proposal), 'CDS event retry changed input selection')
                if kind == 'recovery':
                    feed.retain_book(output)
                    with automatic.lost_result_ack(client.root, path_marker='/run/hedge') as (proxy, committed), automatic.WorkerProcess(feed, proxy, folder, plan['known_at']) as worker:
                        require(committed.wait(30), 'Recovery hedge did not commit before ACK')
                        worker.process.kill()
                        worker.process.wait(timeout=5)
                with automatic.WorkerProcess(feed, client.root, folder, plan['known_at']) as worker:
                    worker.wait('unavailable')
                    if kind != 'recovery':
                        feed.send(output)
                    if kind == 'default':
                        blocked = worker.wait('unavailable', 'cds_recovery_pending')
                        require(blocked['reason'] == 'cds_recovery_pending', 'Unknown recovery did not withhold pricing/hedges')
                        feed.retain_book(output)
                        require(blocked['ready'] is False and 'receipts' not in blocked,
                                'Unknown recovery exposed priced or hedge receipts')
                        snapshot = client.assemble(release['workflow']['plan'])
                        row = client.run(snapshot, 'risk.5')
                        result = client.result(row)
                        require('lifecycle_unpriced' in result['result']['data'] and 'lifecycle' not in result['result']['data'], 'Default audit invented actual risk')
                        require(client.server.call(life_example.native_request(client.snapshot(snapshot), 'risk.5')) == result, 'Unpriced default native parity failed')
                        audits.append((row, client.result(row, raw=True)))
                    else:
                        ready = wait_book(worker, output['payload']['version'])
                        if kind == 'recovery':
                            require(all((r['reused'] for r in ready['receipts'].values())), 'Interrupted recovery results were not reused')
                        more, native = compare(client, ready)
                        saved.extend(more)
                        reports.append(native)
                        mark = event_example.closing_mark(native, TRADE)
                        require(mark['state']['remaining_fraction'] == '0', 'Default retained economic exposure')
                        hedge = native['hedge']['result']['data']
                        require(all((c['risk']['id'] != TRADE for c in hedge['exposure']['problem']['candidates'])), 'Defaulted CDS remained hedge-eligible')
                        risk = native['risk.5']['result']['data']['lifecycle']
                        require(all((s['base_pv'] == 0 and s['value'] == 0 for s in risk['pending'])), 'Default retained pending risk')
                        require(all((s['value'] == 0 for s in risk['current'][1:])), 'Fixed recovery cash retained credit/projection risk')
                        if kind == 'recovery':
                            require(hedge['proposal']['quantities'] != initial['hedge']['result']['data']['proposal']['quantities'], 'Default did not update hedges')
                        if kind == 'complete-cash':
                            require(mark['state']['status'] == 'closed', 'Final actual cash did not close the default')
                current = output['payload']
            if kind == 'default':
                with core.supplied.ServerProcess(binary, folder) as server, automatic.WorkerProcess(feed, core.Client(server).root, folder, plan['known_at']) as worker:
                    worker.wait('unavailable')
                    require(worker.wait('unavailable', 'cds_recovery_pending')['reason'] == 'cds_recovery_pending', 'Restart trusted old readiness')
                    client = core.Client(server)
                    row, raw = audits[-1]
                    require(client.run(row['snapshot_id'], 'risk.5')['reused'] and client.result(row, raw=True) == raw, 'Restart changed the unpriced default audit')
        with core.supplied.ServerProcess(binary, folder) as server, automatic.WorkerProcess(feed, core.Client(server).root, folder, plan['known_at']) as worker:
            client = core.Client(server)
            worker.wait('unavailable')
            restored = worker.wait('ready')
            require(all((r['reused'] for r in restored['receipts'].values())), 'CDS restart did not reuse durable results')
            for row, raw in [*saved, *audits]:
                require(client.result(row, raw=True) == raw, 'CDS restart changed stored bytes')
            for calc in ('risk.5', 'pnl', 'hedge'):
                row = restored['receipts'][calc]
                again = client.run(row['snapshot_id'], calc, recompute=True)
                require(client.result(row, raw=True) == client.result(again, raw=True), 'CDS recomputation changed')
            row, raw = audits[-1]
            again = client.run(row['snapshot_id'], 'risk.5', recompute=True)
            require(client.result(again, raw=True) == raw, 'Unpriced audit recomputation invented a recovery')
    roundoff = [g['attribution']['residual']['marking']
                for report in reports for g in report['pnl']['result']['data']['report']['reporting_totals']
                if any(m['value'] != 0 for m in g['attribution']['residual']['marking'].values())]
    print(json.dumps(dict(
        status='passed', data_quality='synthetic historical replay', trades=6, factors=5, batches=4,
        persisted_priced_results=len(saved), persisted_unpriced_results=len(audits),
        unknown_recovery_blocks_and_survives_restart=True, actual_default_facts_retained=True,
        recovery_cash_risk=True, defaulted_candidates_excluded=True, partial_complete_receipts=True,
        native_parity=True, causal_pnl_reconciled=True, cash_model_pnl_zero_residual=True,
        retained_marking_roundoff=roundoff, constrained_hedge=True,
        post_commit_hedge_ack_kill_recovered=True, server_restart_bytes_cache_recompute_identical=True,
        result_digests={row['result_id']: hashlib.sha256(raw).hexdigest() for row, raw in saved},
        unpriced_result_digests={row['result_id']: hashlib.sha256(raw).hexdigest() for row, raw in audits},
        execution_authority='none', processes_stopped=True), indent=2))
if __name__ == '__main__':
    try:
        main()
    except (RuntimeError, OSError, ValueError, KeyError, IndexError, StopIteration, subprocess.SubprocessError, adapter.IngestionError, analytics.Blocked) as error:
        print(str(error), file=sys.stderr)
        raise SystemExit(1)
