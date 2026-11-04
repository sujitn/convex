#!/usr/bin/env python3
"""Run fills, official futures settlement and partial cash receipts end to end.

Uses the existing six-trade native book, ports, stream and worker. Historical
synthetic transactions are imported at their captured cutoffs; actual UTC HTTP
reference writes/lost acknowledgements are checked separately. No live orders.
"""
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
import core_live_workflow as automatic
import core_snapshot_workflow as core
import core_streaming_workflow as transport
import market_data_adapter as adapter
import streaming_analytics as analytics

ROOT, require = core.ROOT, core.require


def batch(base, definition, identity):
    config = copy.deepcopy(base['workflow'])
    config['plan']['definition']['version'] += 1
    return dict(schema_version=1, id=identity, base=copy.deepcopy(base), definition=definition, workflow=config)


def lifecycle_definition(previous, step, identity):
    """Append actual supplied facts and a full independent counterfactual.

    This only assembles full existing native inputs. Literal mark/cash amounts
    below are new synthetic source facts; native clearing/reconciliation owns
    their effects, currency arithmetic, outstanding amounts and risk eligibility.
    """
    value = copy.deepcopy(previous)
    data = value['calculations']['pnl']['request']['operation']['input']
    def append(binding, suffix):
        b = copy.deepcopy(binding)
        at = next(i for i, n in enumerate(b['holdings']) if data['holdings'][n]['id'] == 'mixed.overnight')
        holding = copy.deepcopy(data['holdings'][b['holdings'][at]])
        holding['lifecycle']['events'].append(copy.deepcopy(step))
        b['holdings'][at] = len(data['holdings'])
        data['holdings'].append(holding)
        b['point']['snapshot_id'] = identity + suffix
        b['point']['manifest'] = dict(id=identity + suffix, version=1)
        for fx in b['point']['fx']:
            fx['snapshot_id'] = b['point']['snapshot_id']
        return b
    after = append(data['steps'][-1]['after'], '.ordered')
    # Cash receipts require their actual prior variation proposal; a full
    # standalone is unavailable in the original before set without that event.
    if step['event']['action']['kind'] == 'payment':
        standalone = dict(kind='unavailable', reason='Actual clearing variation proposal is a prerequisite of this receipt')
    else:
        standalone = dict(kind='available', information=append(data['before'], '.standalone'))
    data['steps'].append(dict(cause='event', after=after))
    data['standalone'].append(standalone)
    return value


def clearing_step(definition):
    data = definition['calculations']['pnl']['request']['operation']['input']
    binding = data['steps'][-1]['after']
    holding = next(data['holdings'][i] for i in binding['holdings'] if data['holdings'][i]['id'] == 'mixed.overnight')
    inputs = holding['product']['product']['input']
    mark = copy.deepcopy(holding['lifecycle']['instrument']['opening_mark'])
    mark.update(kind='daily_settlement', price_points='99.5')
    mark['stamp'].update(id='core.events.synthetic.official-mark', version=11)
    return dict(event=dict(event_id='core.events.future.daily.1', trade_id='mixed.overnight', expected_revision=0,
        effective_date=binding['point']['valuation_date'], known_at=binding['point']['known_at'],
        action=dict(kind='futures_settlement', terms=mark)), credit_rounding=None, delivery=None,
        futures={k: copy.deepcopy(inputs[k]) for k in ('clearing', 'calendar_stamp', 'context')})


def cash_step(revision, amount):
    return dict(event=dict(event_id='core.events.future.cash.' + str(revision), trade_id='mixed.overnight',
        expected_revision=revision, effective_date='2026-10-07', known_at='2026-10-07T12:00:00Z',
        action=dict(kind='payment', terms=dict(receipt_id='core.events.cash.receipt.' + str(revision),
            obligation=dict(kind='variation_margin', effective_date='2026-10-07', ordinal=0),
            amount=dict(value=amount, currency='USD', unit='currency'), value_date='2026-10-07',
            source='core.synthetic.externally-recorded-cash'))), credit_rounding=None, delivery=None, futures=None)


def closing_mark(native, trade):
    report = native['pnl']['result']['data']['report']
    return next(t for t in report['trades'] if t['id'] == trade)['marks'][len(report['input']['causes'])]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary-dir', type=Path)
    args = parser.parse_args()
    directory = args.binary_dir
    if directory is None:
        subprocess.run(['cargo', 'build', '--locked', '-p', 'convex-server', '--bins'], cwd=ROOT, check=True)
        metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--locked', '--no-deps', '--format-version', '1'], cwd=ROOT))
        directory = Path(metadata['target_directory']) / 'debug'
    binary, importer = [(directory / name).resolve(strict=True) for name in ('convex-server', 'convex-core-inputs')]
    capture, plan, factors = core.prepare()
    records = [transport.item('curve', r) for r in capture['curves']] + [transport.item('fixings', capture['fixings'])]
    with tempfile.TemporaryDirectory(prefix='convex-core-events-') as name, books.BookFeed(records) as feed:
        folder = Path(name)
        def import_at_cutoff(commands):
            source = folder / 'captured-reference-transactions.json'
            source.write_text(json.dumps(dict(schema_version=1, references=[dict(recorded_at=plan['known_at'], command=c) for c in commands], curves=[], fixings=[])))
            subprocess.run([str(importer), str(folder / 'state.redb'), str(source)], cwd=ROOT, check=True, stdout=subprocess.PIPE)
            source.unlink()
        import_at_cutoff([r['command'] for r in capture['references']])
        with core.supplied.ServerProcess(binary, folder) as server:
            config = automatic.configuration(server, capture, plan, factors)
        config['plan']['definition']['version'] = 3
        initial_definition = books.definition(capture, factors, 1)
        initial_command = dict(idempotency_key='core.events.initial', changes=[], master_changes=[dict(expected_version=2,
            definition=dict(kind='calculation', definition=initial_definition))])
        import_at_cutoff([initial_command])
        current = dict(id=books.BOOK, version=1, reference_commit=initial_command['idempotency_key'], workflow=config, position_step=1)
        feed.retain_book(transport.item('book', current))
        selection = transport.universe(records)
        selection['book'] = books.BOOK
        (folder / 'subscription.json').write_text(json.dumps(selection))
        (folder / 'workflow.json').write_text(json.dumps(config))
        saved, reports = [], []
        with core.supplied.ServerProcess(binary, folder) as server, automatic.WorkerProcess(feed, core.Client(server).root, folder, plan['known_at']) as worker:
            ready = worker.wait('ready')
            more, native = books.assert_bundle(core.Client(server), ready, 1)
            saved.extend(more)
            reports.append(native)
        proposal = batch(current, books.definition(capture, factors, 2), 'actual-fill.1')
        for phase in range(3):
            if phase == 1:
                proposed_definition = lifecycle_definition(proposal['definition'], clearing_step(proposal['definition']), 'core.events.official')
                data = proposed_definition['calculations']['pnl']['request']['operation']['input']
                # One atomic Event phase retains both independently identified
                # facts, in actual native replay order. Its final state exposes
                # the partial receipt without an extra full-book mark/report.
                for binding in [data['steps'][-1]['after'], data['standalone'][-1]['information']]:
                    holding = next(data['holdings'][i] for i in binding['holdings'] if data['holdings'][i]['id'] == 'mixed.overnight')
                    holding['lifecycle']['events'].append(cash_step(1, '750'))
                proposal = batch(current, proposed_definition, 'official-clearing.1')
            elif phase == 2:
                proposed_definition = lifecycle_definition(proposal['definition'], cash_step(2, '1750'), 'core.events.cash.2')
                proposal = batch(current, proposed_definition, 'actual-cash.' + str(phase))
            path = folder / 'event-batch.json'
            path.write_text(json.dumps(proposal))
            with core.supplied.ServerProcess(binary, folder) as server:
                command_line = [sys.executable, str(ROOT / 'tools/book_events.py'), str(path), '--url', core.Client(server).root, '--timeout', '120']
                prepared = json.loads(subprocess.check_output([*command_line, '--prepare-only'], cwd=ROOT))
            # Explicit historical import, using exactly the runnable adapter's
            # prepared command and original cutoff. No backdating HTTP facility.
            import_at_cutoff([prepared['command']])
            with core.supplied.ServerProcess(binary, folder) as server:
                client = core.Client(server)
                command_line[command_line.index('--url') + 1] = client.root
                try:
                    output = json.loads(subprocess.check_output(command_line, cwd=ROOT))
                except subprocess.CalledProcessError:
                    probe = analytics.Client(client.root, timeout=120)
                    try:
                        snapshot = probe.assemble(prepared['release']['workflow']['plan'])
                        receipt = probe.call('POST', '/analytics/snapshots/' + snapshot + '/run/pnl', {})
                        diagnostic = probe.call('GET', '/analytics/results/' + receipt['result_id'])
                    except adapter.IngestionError as error:
                        raise RuntimeError('Synthetic event phase ' + proposal['id'] + ': HTTP ' + str(error.http_status)) from None
                    raise RuntimeError('Synthetic event phase ' + proposal['id'] + ': '
                        + json.dumps(diagnostic['error'] if diagnostic['ok'] == 'false' else {'reason': 'event_risk_scope_rejected'})) from None
                require(output == json.loads(subprocess.check_output(command_line, cwd=ROOT)), 'Event retry changed release')
                require(output['payload'] == prepared['release'], 'Prepared/recovered event release differed')
                if phase == 0:
                    feed.retain_book(output)
                    with automatic.lost_result_ack(client.root) as (proxy, committed), automatic.WorkerProcess(feed, proxy, folder, plan['known_at']) as worker:
                        require(committed.wait(30), 'Event-updated result never reached durable acknowledgement gate')
                        worker.process.kill()
                        worker.process.wait(timeout=5)
                with automatic.WorkerProcess(feed, client.root, folder, plan['known_at']) as worker:
                    prior = worker.wait('ready')
                    if phase:
                        require(prior['book']['version'] == current['version'], 'An unreleased event candidate became active')
                        feed.send(output)
                        ready = worker.wait('ready')
                    else:
                        ready = prior
                        require(any(r['reused'] for r in ready['receipts'].values()), 'Interrupted event result was not recovered')
                    current = output['payload']
                    more, native = books.assert_bundle(client, ready, current['position_step'])
                    saved.extend(more)
                    reports.append(native)
                    if phase == 0:
                        before = closing_mark(reports[0], 'stepped-amortizing-IRS')
                        after = closing_mark(native, 'stepped-amortizing-IRS')
                        require(before['local']['current'] != after['local']['current'], 'Actual fill did not update native current risk exposure')
                        require(before['executed_quantity'] != after['executed_quantity'], 'Fill facts were lost')
                        exposure_before = reports[0]['hedge']['result']['data']['exposure']
                        exposure_after = native['hedge']['result']['data']['exposure']
                        require(exposure_before['current'] != exposure_after['current']
                            and exposure_before['pending'] != exposure_after['pending'], 'Fill did not update native current/pending factor exposure')
                    else:
                        mark = closing_mark(native, 'mixed.overnight')
                        obligation = next(o for o in mark['state']['obligations'] if o['key']['kind'] == 'variation_margin')
                        require(obligation['settled'] == (phase == 2), 'Partial/complete cash settlement lost its native state')
                        if phase == 1:
                            require(mark['state']['futures_mark']['price_points'] == '99.5', 'Official mark not replayed')
                            require(obligation['recorded']['value'] == '750' and obligation['amount'] != obligation['recorded'], 'Partial receipt became full proposed cash')
                        require(mark['recorded_cash'] != closing_mark(reports[-2], 'mixed.overnight')['recorded_cash'], 'Actual receipt did not update cash P&L')
                if phase == 2:
                    # Bad quantities keep their original fill fact. Native
                    # admission must reject, without relaxing a numerical bound.
                    invalid = client.snapshot(ready['snapshot_id'])
                    target = invalid['content']['definition']['definition']['definition']['calculations']['pnl']['request']['operation']['input']['steps'][-1]['after']['point']['positions']
                    next(p for p in target if p['identity']['id'] == 'stepped-amortizing-IRS')['current']['quantity'] = '450001'
                    rejected = server.call(core.native_request(invalid, 'pnl'), success=False)
                    require(rejected['ok'] == 'false', 'Native accepted a quantity inconsistent with actual fills')
                    # Actual UTC reference commit, killed after durable write
                    # before ACK. Its unreleased revision cannot affect the book.
                    command = copy.deepcopy(prepared['command'])
                    command['idempotency_key'] = 'core.events.actual-http-ack'
                    command['master_changes'][0]['expected_version'] += 1
                    (folder / 'utc-command.json').write_text(json.dumps(command))
                    with automatic.lost_result_ack(client.root, path_marker='/reference/transactions') as (proxy, committed):
                        child = subprocess.Popen([sys.executable, '-c',
                            'import sys; sys.path.insert(0,"tools"); import streaming_analytics as a; a.Client(sys.argv[1]).call("POST","/reference/transactions",a.load(sys.argv[2]))', proxy,
                            str(folder / 'utc-command.json')], cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                        try:
                            require(committed.wait(10), 'UTC event reference was not committed before ACK')
                        finally:
                            child.kill()
                            child.communicate(timeout=5)
                    commit = client.call('POST', '/reference/transactions', command)
                    require(commit == client.call('GET', '/reference/commits/' + command['idempotency_key']), 'Event receipt changed after lost acknowledgement')
                    conflict = copy.deepcopy(command)
                    conflict['idempotency_key'] += '.conflict'
                    client.call('POST', '/reference/transactions', conflict, status=409)
                    changed = copy.deepcopy(command)
                    changed['master_changes'][0]['definition']['definition']['provenance']['evidence_reference'] += ' changed'
                    client.call('POST', '/reference/transactions', changed, status=409)
        path.unlink()
        with core.supplied.ServerProcess(binary, folder) as server, automatic.WorkerProcess(feed, core.Client(server).root, folder, plan['known_at']) as worker:
            client = core.Client(server)
            worker.wait('unavailable')
            recovered = worker.wait('ready')
            require(recovered['snapshot_id'] == ready['snapshot_id'] and recovered['book']['version'] == current['version'], 'Restart selected another event/market revision')
            require(all(r['reused'] for r in recovered['receipts'].values()), 'Restart lost event result cache')
            for receipt, raw in saved:
                require(client.result(receipt, raw=True) == raw, 'Restart changed event result bytes')
            for calc in ('risk.0', 'pnl', 'hedge'):
                receipt = recovered['receipts'][calc]
                replay = client.run(receipt['snapshot_id'], calc, recompute=True)
                require(client.result(replay, raw=True) == client.result(receipt, raw=True), 'Saved event recomputation changed')
    print(json.dumps(dict(status='passed', data_quality='synthetic historical replay', trades=6, factors=5,
        batches=3, persisted_results=len(saved), actual_fill_and_pending_positions=True,
        official_futures_lifecycle_replayed=True, partial_and_complete_receipts=True,
        proposed_cash_distinct_from_received=True, native_parity=True, causal_pnl_reconciled=True,
        constrained_hedge_from_same_positions=True, duplicate_event_batch_idempotent=True,
        event_result_crash_recovered=True, actual_utc_reference_ack_recovered=True,
        stale_and_conflicting_reference_writes_rejected=True, native_bad_quantity_rejected=True,
        unreleased_revision_ignored=True, restart_bytes_cache_recompute_identical=True,
        result_digests={r['result_id']: hashlib.sha256(raw).hexdigest() for r, raw in saved},
        execution_authority='none', processes_stopped=True), indent=2))


if __name__ == '__main__':
    try:
        main()
    except (RuntimeError, OSError, ValueError, KeyError, IndexError, StopIteration, subprocess.SubprocessError,
            adapter.IngestionError, analytics.Blocked) as error:
        print(str(error), file=sys.stderr)
        raise SystemExit(1)
