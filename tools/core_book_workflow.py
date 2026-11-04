#!/usr/bin/env python3
"""Run complete position/reference book releases through existing durable ports.

The financial replay retains original full supplied contracts, actual fill facts
and causal history. A future synthetic holiday correction is explicit; Python
performs no valuation, risk, aggregation or P&L arithmetic.
"""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import threading

import core_snapshot_workflow as core
import core_streaming_workflow as transport
import core_live_workflow as automatic
import market_data_adapter as adapter
import streaming_analytics as analytics

ROOT, require = core.ROOT, core.require
BOOK = 'core.synthetic.book-updates'


def definition(capture, factors, step, *, calendar_revision=False):
    value = copy.deepcopy(capture['references'][1]['command']['master_changes'][0]['definition']['definition'])
    pnl = value['calculations']['pnl']['request']['operation']['input']
    pnl['steps'], pnl['standalone'] = pnl['steps'][:step + 1], pnl['standalone'][:step + 1]
    bindings = [pnl['before'], *[s['after'] for s in pnl['steps']],
                *[s['information'] for s in pnl['standalone'] if s['kind'] == 'available']]
    used = sorted({i for b in bindings for i in b['holdings']})
    remap = {old: new for new, old in enumerate(used)}
    pnl['holdings'] = [pnl['holdings'][i] for i in used]
    for binding in bindings:
        binding['holdings'] = [remap[i] for i in binding['holdings']]
    book = core.supplied.snapshot()
    book['operation']['input']['before'] = copy.deepcopy(book['operation']['input']['steps'][step]['after'])
    requests, _ = core.supplied.risk_requests(book)
    for n, request in enumerate(requests):
        request['inputs'] = copy.deepcopy(analytics.EMPTY)
        request['operation']['input']['factors'] = copy.deepcopy(factors)
        value['calculations']['risk.' + str(n)] = dict(request=request, curve_sets=['market.1'])
    if calendar_revision:
        # An explicit new reference correction, not a change to any pinned
        # fixture/default: the future coupon's original date is now a holiday.
        def repin(item):
            if isinstance(item, dict):
                if set(item) == {'id', 'version'} and item['id'] == 'CASH.SYNTHETIC.v1' and item['version'] == 1:
                    item['version'] = 2
                for child in item.values():
                    repin(child)
            elif isinstance(item, list):
                for child in item:
                    repin(child)
        repin(value)
        calendar = next(c for c in value['reference_inputs']['calendars'] if c['pin'] == dict(id='CASH.SYNTHETIC.v1', version=2))
        calendar['holidays'].append('2027-04-07')
    value['provenance']['evidence_reference'] = 'Synthetic complete book at causal step ' + str(step) + ('; explicit future holiday correction v2' if calendar_revision else '')
    return value


def releases(base_config, capture, factors):
    imports, messages = [], []
    for version, step, correction in [(3, 0, False), (4, 2, False), (5, 2, True)]:
        key = 'core.book.release.' + str(version)
        record = definition(capture, factors, step, calendar_revision=correction)
        command = dict(idempotency_key=key, changes=[], master_changes=[dict(expected_version=version - 1,
            definition=dict(kind='calculation', definition=record))])
        imports.append(dict(recorded_at=base_config['plan']['known_at'], command=command))
        config = copy.deepcopy(base_config)
        config['plan']['definition']['version'] = version
        messages.append(transport.item('book', dict(id=BOOK, version=version - 2,
            reference_commit=key, workflow=config, position_step=step)))
    return imports, messages


class BookFeed(transport.LocalFeed):
    def retain_book(self, record):
        with self.lock:
            self.records = [r for r in self.records if r['kind'] != 'book'] + [copy.deepcopy(record)]

    def send(self, record, *, gap=False):
        if record['kind'] != 'book' or gap:
            return super().send(record, gap=gap)
        with self.lock:
            self.sequence += 1
            self.records = [r for r in self.records if r['kind'] != 'book'] + [copy.deepcopy(record)]
            self.last = self.frame('update', [record])
            self.active.put_nowait(copy.deepcopy(self.last))


def assert_bundle(client, event, step):
    saved, native = automatic.compare(client, event)
    snapshot = client.snapshot(event['snapshot_id'])
    definition = snapshot['content']['definition']['definition']['definition']
    close = definition['calculations']['pnl']['request']['operation']['input']['steps'][step]['after']['point']
    problem = native['hedge']['result']['data']['exposure']['problem']
    quantities = {p['identity']['id']: p for p in close['positions']}
    for row in problem['holdings']:
        p = quantities[row['risk']['id']]
        require(row['current_quantity'] == p['current']['quantity'] and row['pending_quantity'] == p['pending_quantity'],
                'Hedge used another position revision')
    require(problem['position_state'] == close['manifest'] == event['book']['position_state'], 'Position manifest mismatch')
    require(snapshot['content']['plan']['definition'] == event['book']['definition'], 'Reference revision mismatch')
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
    binary = (directory / 'convex-server').resolve(strict=True)
    importer = (directory / 'convex-core-inputs').resolve(strict=True)
    capture, plan, factors = core.prepare()
    records = [transport.item('curve', r) for r in capture['curves']] + [transport.item('fixings', capture['fixings'])]
    with tempfile.TemporaryDirectory(prefix='convex-core-books-') as name, BookFeed(records) as feed:
        folder = Path(name)
        source = folder / 'references.json'
        def import_references(references):
            source.write_text(json.dumps(dict(schema_version=1, references=references, curves=[], fixings=[])))
            subprocess.run([str(importer), str(folder / 'state.redb'), str(source)], cwd=ROOT, check=True, stdout=subprocess.PIPE)
        import_references(capture['references'])
        with core.supplied.ServerProcess(binary, folder) as server:
            config = automatic.configuration(server, capture, plan, factors)
        # Historical reference transactions retain their genuine replay cutoffs;
        # the live HTTP writer is exercised separately below with actual UTC time.
        imports, books = releases(config, capture, factors)
        import_references(imports)
        config = copy.deepcopy(books[0]['payload']['workflow'])
        (folder / 'workflow.json').write_text(json.dumps(config))
        selection = transport.universe(records)
        selection['book'] = BOOK
        (folder / 'subscription.json').write_text(json.dumps(selection))
        feed.retain_book(books[0])
        with core.supplied.ServerProcess(binary, folder) as server:
            client = core.Client(server)
            with automatic.WorkerProcess(feed, client.root, folder, plan['known_at']) as worker:
                initial = worker.wait('ready')
                saved, before = assert_bundle(client, initial, 0)
            feed.retain_book(books[1])
            with automatic.lost_result_ack(client.root) as (proxy, blocked), automatic.WorkerProcess(feed, proxy, folder, plan['known_at']) as worker:
                require(blocked.wait(30), 'Updated position result never committed before acknowledgement')
                worker.process.kill()
                worker.process.wait(timeout=5)
            with automatic.WorkerProcess(feed, client.root, folder, plan['known_at']) as worker:
                positioned = worker.wait('ready')
                require(positioned['book']['version'] == 2 and any(r['reused'] for r in positioned['receipts'].values()), 'Position revision was not recovered')
                more, after = assert_bundle(client, positioned, 2)
                saved.extend(more)
                by_trade = lambda report: {h['risk']['id']: h for h in report['hedge']['result']['data']['exposure']['problem']['holdings']}
                require(by_trade(before)['stepped-amortizing-IRS']['current_quantity'] != by_trade(after)['stepped-amortizing-IRS']['current_quantity'], 'Actual supplied fill did not update positions')
                feed.send(books[2])
                referenced = worker.wait('ready')
                more, corrected = assert_bundle(client, referenced, 2)
                saved.extend(more)
                require(core.measure(after['risk.0']) != core.measure(corrected['risk.0']), 'Calendar reference correction did not reprice the bond')
                bad = copy.deepcopy(books[2])
                bad['payload'].update(version=4, position_step=0)
                feed.send(bad)
                worker.wait('unavailable', 'book_position_must_be_pnl_close')
                require(analytics.load(folder / 'workflow-state.json')['book']['version'] == 3, 'Inconsistent book advanced the admitted receipt')
                current_book = copy.deepcopy(books[2])
                current_book['payload']['version'] = 5
                feed.send(current_book)
                current = worker.wait('ready')
                require(current['snapshot_id'] == referenced['snapshot_id'], 'Identical corrected native inputs changed identity')
                skipped = copy.deepcopy(current_book)
                skipped['payload']['version'] = 99
                feed.send(skipped, gap=True)
                worker.wait('unavailable', 'sequence_gap')
                require(worker.wait('ready')['snapshot_id'] == current['snapshot_id'], 'Skipped book revision leaked into analytics')
                feed.send(books[1])  # Old image advertised as a new in-session update.
                worker.wait('unavailable', 'book_revision_rollback')
                feed.retain_book(current_book)
                feed.disconnect()
                require(worker.wait('ready')['book']['version'] == 5, 'Book rollback bypassed saved high-water mark')
                # Exercise a real UTC HTTP reference CAS and lost response. No
                # release activates this later revision in the historical book.
                command = copy.deepcopy(imports[-1]['command'])
                command['idempotency_key'] = 'core.book.actual-http-update'
                command['master_changes'][0]['expected_version'] = 5
                failures = []
                with automatic.lost_result_ack(client.root, path_marker='/reference/transactions') as (proxy, blocked):
                    def publish():
                        try:
                            analytics.Client(proxy, timeout=.2).call('POST', '/reference/transactions', command)
                        except adapter.IngestionError as error:
                            failures.append(error)
                    thread = threading.Thread(target=publish)
                    thread.start()
                    require(blocked.wait(5), 'Reference transaction did not reach durable acknowledgement gate')
                    thread.join(timeout=3)
                    require(not thread.is_alive() and failures, 'Reference acknowledgement fault did not complete')
                commit = client.call('POST', '/reference/transactions', command)
                require(commit == client.call('GET', '/reference/commits/' + command['idempotency_key']), 'Reference acknowledgement recovery changed receipt')
                require(commit['master_records'][0]['version'] == 6, 'Reference CAS did not create revision6')
                conflict = copy.deepcopy(command)
                conflict['idempotency_key'] += '.conflict'
                client.call('POST', '/reference/transactions', conflict, status=409)
                update = copy.deepcopy(next(r for r in capture['curves'] if r['curve_id'] == 'D.OIS' and r['version'] == 2))
                update['version'] = 3
                feed.send(transport.item('curve', update))
                current = worker.wait('ready')
                more, _ = assert_bundle(client, current, 2)
                saved.extend(more)
                require(current['book']['definition']['version'] == 5, 'Unreleased latest reference leaked into calculations')
            source.unlink()
        with core.supplied.ServerProcess(binary, folder) as server:
            client = core.Client(server)
            with automatic.WorkerProcess(feed, client.root, folder, plan['known_at']) as worker:
                require(not worker.wait('unavailable')['ready'], 'Restart trusted a saved ready flag')
                recovered = worker.wait('ready')
                require(recovered['book']['version'] == 5 and recovered['snapshot_id'] == current['snapshot_id'], 'Restart lost admitted book/market selection')
                require(all(r['reused'] for r in recovered['receipts'].values()), 'Restart lost native result cache')
                for receipt, raw in saved:
                    require(client.result(receipt, raw=True) == raw, 'Restart changed saved revision result')
                for calc in ['risk.0', 'pnl', 'hedge']:
                    receipt = recovered['receipts'][calc]
                    replay = client.run(receipt['snapshot_id'], receipt['calculation'], recompute=True)
                    require(client.result(replay, raw=True) == client.result(receipt, raw=True), 'Saved-input replay changed')
    print(json.dumps(dict(status='passed', data_quality='synthetic historical replay', trades=6, factors=5,
        persisted_results=len(saved), position_fill_consumed=True, reference_calendar_correction_consumed=True,
        one_closing_position_reference_manifest=True, native_parity=True, causal_pnl_reconciled=True,
        inconsistent_release_rejected=True, gap_and_book_rollback_recovered=True,
        updated_position_interrupted_result_recovered=True, actual_http_reference_ack_recovered=True,
        actual_http_cas_conflict_rejected=True, unreleased_latest_reference_ignored=True,
        restart_results_and_cache_identical=True, saved_recomputation_identical=True,
        result_digests={r['result_id']: hashlib.sha256(raw).hexdigest() for r, raw in saved},
        execution_authority='none', processes_stopped=True), indent=2))


if __name__ == '__main__':
    try:
        main()
    except (RuntimeError, OSError, ValueError, KeyError, StopIteration, subprocess.SubprocessError) as error:
        print(str(error), file=sys.stderr)
        raise SystemExit(1)
