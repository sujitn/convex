#!/usr/bin/env python3
"""Issuer-wide bond/CDS defaults, actual recovery and cash on the existing server."""
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
import core_cds_workflow as cds
import core_event_workflow as event_example
import core_lifecycle_workflow as life_example
import core_live_workflow as automatic
import core_snapshot_workflow as core
import core_streaming_workflow as transport
import issuer_events
import lifecycle_risk
import market_data_adapter as adapter
import streaming_analytics as analytics

ROOT, require = core.ROOT, core.require
BOND, BOND2, OTHER, CDS = 'mixed.bond', 'issuer.bond.2', 'other.bond', cds.TRADE
AFFECTED = (BOND, BOND2, CDS)
LIFECYCLE = [*life_example.LIFECYCLE, BOND2, OTHER]
SCOPE = 'core.synthetic.issuer-scope'
NOTICE = dict(id='core.issuer.default.1', effective_date='2026-10-07', known_at='2026-10-07T12:00:00Z',
              reference_id='core.synthetic.actual-notice', reference_version=1)


def bindings(data):
    return [data['before'], *[s['after'] for s in data['steps']],
            *[s['information'] for s in data['standalone'] if s['kind'] == 'available']]


def definition(capture, factors):
    value = cds.definition(capture, factors)
    data = value['calculations']['pnl']['request']['operation']['input']
    for holding in data['holdings']:
        if holding['id'] == BOND:
            holding['lifecycle']['instrument']['credit'] = dict(entity='ACME', seniority='SeniorUnsecured',
                document=dict(id='core.bond.principal-only.confirmation', version=1))
    originals = list(enumerate(data['holdings']))
    for trade, security, quantity, pending, entity, name in (
            (BOND2, 'issuer.security.2', '2400', '500', 'ACME', 'risk.6'),
            (OTHER, 'other.security', '-600', '0', 'OTHER', 'risk.7')):
        clones = {}
        for index, holding in originals:
            if holding['id'] != BOND:
                continue
            clone = copy.deepcopy(holding)
            clone['id'] = clone['lifecycle']['trade_id'] = trade
            clone['lifecycle']['position']['value'] = quantity
            clone['lifecycle']['instrument']['credit']['entity'] = entity
            if trade == BOND2:
                # This distinct supplied contract is denominated in2400 face.
                # Its earned cash is exact, rather than a rounded receipt against
                # a fractional copy of the first contract's Unrounded coupon.
                for bond in (clone['product']['product']['bond'], clone['lifecycle']['instrument']['bond']):
                    bond['terms']['face_value'] = '2400'
                    bond['terms']['rounding'] = dict(NearestEven=2)
                    bond['terms']['profiles']['notional'][0]['value'] = '2400'
                    bond['terms']['profiles']['notional'][1]['value'] = '1440'
            for event in clone['lifecycle']['events']:
                event['event']['trade_id'] = trade
                event['event']['event_id'] = trade + '.event.' + str(event['event']['expected_revision'])
            clones[index] = len(data['holdings'])
            data['holdings'].append(clone)
        for binding in bindings(data):
            at = next(i for i, p in enumerate(binding['point']['positions']) if p['identity']['id'] == BOND)
            position = copy.deepcopy(binding['point']['positions'][at])
            position['identity'].update(id=trade, instrument=dict(id=security, version=1))
            for key in ('economics', 'calculation'):
                position['identity'][key]['id'] = trade + '.' + position['identity'][key]['id']
            position['current']['quantity'] = quantity
            if trade == BOND2:
                position['current']['reference_quantity'] = '2400'
            position['pending_quantity'] = pending
            binding['point']['positions'].append(position)
            binding['holdings'].append(clones[binding['holdings'][at]])
        close = data['steps'][-1]['after']
        position = next(p for p in close['point']['positions'] if p['identity']['id'] == trade)
        calc = copy.deepcopy(value['calculations']['risk.0'])
        calc['request']['operation']['input']['identity'] = copy.deepcopy(position['identity'])
        if trade == BOND2:
            calc['request']['operation']['product'] = copy.deepcopy(data['holdings'][close['holdings'][-1]]['product']['product'])
            calc['request']['operation']['input']['reference_position'].update(quantity='2400', reference_quantity='2400')
        value['calculations'][name] = calc
    value['reference_inputs']['issuer_scope'] = dict(id=SCOPE, version=1)
    return value


def configure(config, value):
    config['plan']['definition']['version'] = 3
    problem = config['hedge']['operation']['problem']
    for trade, name, current, pending in ((BOND2, 'risk.6', '2400', '500'), (OTHER, 'risk.7', '-600', '0')):
        config['risks'][trade] = name
        holding = copy.deepcopy(next(h for h in problem['holdings'] if h['risk'] == BOND))
        holding.update(risk=trade, current_quantity=current, pending_quantity=pending)
        problem['holdings'].append(holding)
        problem['expected_holdings'].append(trade)
    # A distinct bond of the same issuer is in the supplied fresh hedge universe.
    next(c for c in problem['candidates'] if c['risk'] == BOND)['risk'] = BOND2
    return lifecycle_risk.bind_requests(value, config, LIFECYCLE)


def reference_changes(value):
    close = value['calculations']['pnl']['request']['operation']['input']['steps'][-1]['after']['point']
    instruments = [dict(id=p['identity']['instrument']['id'], version=p['identity']['instrument']['version'],
                        entity_id='OTHER' if p['identity']['id'] == OTHER else 'ACME')
                   for p in close['positions'] if p['identity']['id'] in (*AFFECTED, OTHER)]
    provenance = dict(source='core.synthetic.reference', evidence_reference='Explicit synthetic legal-entity mappings')
    rows = [dict(expected_version=None, definition=dict(kind='entity', definition=dict(entity_id=entity,
        legal_name=entity + ' synthetic legal entity', parent=None if entity == 'ACME' else dict(entity_id='ACME', version=1),
        country='GB', effective=copy.deepcopy(value['effective']), provenance=copy.deepcopy(provenance))))
        for entity in ('ACME', 'OTHER')]
    scope = dict(id=SCOPE, directory=dict(issuers=[dict(entity_id=i, version=1, default=None) for i in ('ACME', 'OTHER')],
                instruments=instruments), effective=copy.deepcopy(value['effective']), provenance=copy.deepcopy(provenance))
    rows.append(dict(expected_version=None, definition=dict(kind='issuer_scope', definition=scope)))
    return rows, scope


def step(trade, action, identity, revision):
    return dict(event=dict(event_id=identity, trade_id=trade, expected_revision=revision,
        effective_date=NOTICE['effective_date'], known_at=NOTICE['known_at'], action=action),
        credit_rounding=None, futures=None, delivery=None)


def defaults():
    return {trade: [step(trade, dict(kind='credit_default', terms=dict(entity='ACME', seniority='SeniorUnsecured',
        document=dict(id='CDS.Standard.Supplied.Confirmation' if trade == CDS else 'core.bond.principal-only.confirmation', version=1),
        accrual_day_fraction='0.5' if trade == CDS else '0', rounding=dict(NearestEven=2))), NOTICE['id'], 0 if trade == CDS else 1)]
        for trade in AFFECTED}


def recoveries(due='2026-10-07'):
    return {trade: [step(trade, dict(kind='recovery', terms=dict(default_event=NOTICE['id'],
        recovery_fraction='0.40' if trade == CDS else '0.35', reference=dict(id='core.synthetic.auction.' + trade, version=1),
        cash_date=due)), 'core.issuer.recovery.' + trade, 1 if trade == CDS else 2)] for trade in AFFECTED}


def payment(trade, identity, revision, kind, amount):
    return step(trade, dict(kind='payment', terms=dict(receipt_id=identity + '.cash',
        obligation=dict(kind=kind, effective_date=NOTICE['effective_date'], ordinal=0),
        amount=dict(value=amount, currency='USD', unit='currency'), value_date=NOTICE['effective_date'],
        source='core.synthetic.actual-cash')), identity, revision)


def append(previous, changes, identity, *, cancel=False, resolve=False):
    value = copy.deepcopy(previous)
    data = value['calculations']['pnl']['request']['operation']['input']
    def point(binding, suffix):
        close = copy.deepcopy(binding)
        for at, index in enumerate(close['holdings']):
            trade = data['holdings'][index]['id']
            if trade in changes:
                holding = copy.deepcopy(data['holdings'][index])
                holding['lifecycle']['events'].extend(copy.deepcopy(changes[trade]))
                close['holdings'][at] = len(data['holdings'])
                data['holdings'].append(holding)
        close['point']['snapshot_id'] = identity + suffix
        close['point']['manifest'] = dict(id=identity + suffix, version=1)
        for fx in close['point']['fx']:
            fx['snapshot_id'] = close['point']['snapshot_id']
        for position in close['point']['positions']:
            if cancel and position['identity']['id'] in AFFECTED:
                position['pending_quantity'] = '0'
            for key in ('economics', 'calculation'):
                position['identity'][key] = dict(id=identity + suffix + '.' + position['identity']['id'] + '.' + key, version=1)
        return close
    data['steps'].append(dict(cause='position' if cancel else 'event', after=point(data['steps'][-1]['after'], '.ordered')))
    data['standalone'].append(dict(kind='available', information=point(data['before'], '.standalone')) if cancel
                             else dict(kind='unavailable', reason='The actual preceding contract states are required'))
    if resolve:
        data['steps'].pop(-2)
        data['standalone'].pop(-2)
        used = sorted({i for binding in bindings(data) for i in binding['holdings']})
        mapping = {old: new for new, old in enumerate(used)}
        data['holdings'] = [data['holdings'][i] for i in used]
        for binding in bindings(data):
            binding['holdings'] = [mapping[i] for i in binding['holdings']]
    close = data['steps'][-1]['after']
    for position, index in zip(close['point']['positions'], close['holdings'], strict=True):
        operation = next(c['request']['operation'] for c in value['calculations'].values()
            if c['request']['operation']['kind'] in ('graph_hedge_risk', 'lifecycle_hedge_risk')
            and c['request']['operation']['input']['identity']['id'] == position['identity']['id'])
        operation['product'] = copy.deepcopy(data['holdings'][index]['product']['product'])
        operation['input']['identity'] = copy.deepcopy(position['identity'])
        operation['input']['reference_position'] = copy.deepcopy(position['current'])
        operation['input']['reference_position']['quantity'] = position['current']['reference_quantity']
    return value


def native_request(saved, calc):
    saved = copy.deepcopy(saved)
    saved['content']['definition']['definition']['definition']['reference_inputs'].pop('issuer_scope', None)
    return life_example.native_request(saved, calc)


def compare(client, event, config):
    native, saved = {}, []
    for calc, row in event['receipts'].items():
        result = client.result(row)
        require(client.server.call(native_request(client.snapshot(row['snapshot_id']), calc)) == result, 'Issuer native parity failed')
        native[calc] = result
        saved.append((row, client.result(row, raw=True)))
    report = native['pnl']['result']['data']['report']
    for group in report['reporting_totals']:
        check_attribution(group['attribution'])
    hedge = native['hedge']['result']['data']
    require(hedge['status'] == 'feasible' and all(c['satisfied'] for c in hedge['proposal']['constraints']), 'Issuer hedge constraints failed')
    scope = hedge['issuer_scope']
    require(scope['reference']['id'] == SCOPE, 'Hedge lost captured issuer reference')
    for trade in LIFECYCLE:
        result = native[config['risks'][trade]]['result']['data']
        operation = native_request(client.snapshot(event['receipts'][config['risks'][trade]]['snapshot_id']), config['risks'][trade])['operation']
        state = lifecycle_risk.checked_state(result, report, operation)
        require(state in hedge['lifecycle_risk'], 'Issuer hedge lost exact native lifecycle state')
    return saved, native


def check_attribution(attribution):
    """Retain raw residuals within the native difference/addition half-ULP budget.

    Apply the original example's arithmetic reconstruction to each native money
    field. No financial tolerance, residual clamp or native result is changed.
    """
    paths = [('cash',), ('current_model_pnl',), *[('marking', k) for k in ('current', 'pending', 'projected')]]
    def amount(row, path):
        for key in path:
            row = row[key]
        return row['value']
    for path in paths:
        total = amount(attribution['total'], path)
        running, budget = 0.0, math.ulp(total)
        for row in attribution['ordered']:
            delta = amount(row, path)
            running += delta
            budget += math.ulp(delta) + math.ulp(running)
        residual = amount(attribution['residual'], path)
        budget = (budget + math.ulp(residual))/2
        require(math.isfinite(residual) and residual == total-running and abs(residual) <= budget,
                'Issuer P&L residual exceeds its native arithmetic roundoff budget')


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
    with tempfile.TemporaryDirectory(prefix='convex-core-issuer-') as name, books.BookFeed(records) as feed:
        folder = Path(name)
        def import_at_cutoff(commands):
            path = folder / 'captured.json'
            path.write_text(json.dumps(dict(schema_version=1, references=[dict(recorded_at=plan['known_at'], command=c) for c in commands], curves=[], fixings=[])))
            subprocess.run([str(importer), str(folder / 'state.redb'), str(path)], cwd=ROOT, check=True, stdout=subprocess.PIPE)
            path.unlink()
        import_at_cutoff([r['command'] for r in capture['references']])
        with core.supplied.ServerProcess(binary, folder) as server:
            config = automatic.configuration(server, capture, plan, factors)
        value = configure(config, definition(capture, factors))
        rows, scope = reference_changes(value)
        initial_command = dict(idempotency_key='core.issuer.initial', changes=[], master_changes=[
            dict(expected_version=2, definition=dict(kind='calculation', definition=value)), *rows])
        import_at_cutoff([initial_command])
        current = dict(id=books.BOOK, version=1, reference_commit=initial_command['idempotency_key'], workflow=config, position_step=0)
        feed.retain_book(transport.item('book', current))
        selection = transport.universe(records)
        selection['book'] = books.BOOK
        (folder / 'subscription.json').write_text(json.dumps(selection))
        (folder / 'workflow.json').write_text(json.dumps(config))
        saved, audits, residuals = [], [], []
        with core.supplied.ServerProcess(binary, folder) as server, automatic.WorkerProcess(feed, core.Client(server).root, folder, plan['known_at']) as worker:
            client = core.Client(server)
            worker.wait('unavailable')
            ready = worker.wait('ready')
            base_snapshot = ready['receipts']['pnl']['snapshot_id']
            more, initial = compare(client, ready, config)
            saved.extend(more)
            residuals.extend(g['attribution']['residual'] for g in initial['pnl']['result']['data']['report']['reporting_totals'])
        value = append(value, {}, 'issuer.cancel', cancel=True)
        value = append(value, defaults(), 'issuer.default')
        value['reference_inputs']['issuer_scope']['version'] = 2
        scope['directory']['issuers'][0]['default'] = copy.deepcopy(NOTICE)
        for kind in ('default', 'recovery', 'partial-cash', 'complete-cash'):
            if kind == 'recovery':
                value = append(value, recoveries(), 'issuer.recovery', resolve=True)
            elif kind == 'partial-cash':
                value = append(value, {t: [payment(t, 'issuer.partial.' + t, 2 if t == CDS else 3, 'recovery', amount)]
                    for t, amount in ((BOND, '-100'), (BOND2, '200'), (CDS, '-100000'))}, 'issuer.partial')
            elif kind == 'complete-cash':
                value = append(value, {t: [payment(t, 'issuer.complete.' + t, 3 if t == CDS else 4, 'recovery', amount),
                    payment(t, 'issuer.earned.' + t, 4 if t == CDS else 5, 'default_accrual' if t == CDS else 'bond_coupon', earned)]
                    for t, amount, earned in ((BOND, '-320', '-21.35'), (BOND2, '640', '42.70'), (CDS, '-170000', '218.75'))}, 'issuer.complete')
            value = lifecycle_risk.bind_requests(value, config, LIFECYCLE)
            proposal = event_example.batch(current, value, 'issuer.' + kind)
            ingest = events
            if kind == 'default':
                proposal.update(base_snapshot=base_snapshot, issuer_scope_change=dict(expected_version=1,
                    definition=dict(kind='issuer_scope', definition=copy.deepcopy(scope))))
                ingest = issuer_events
            elif kind == 'recovery':
                proposal.update(schema_version=2, resolve_default=True)
            with core.supplied.ServerProcess(binary, folder) as server:
                command, release = ingest.prepare(analytics.Client(core.Client(server).root, timeout=120), proposal)
            import_at_cutoff([command])
            with core.supplied.ServerProcess(binary, folder) as server:
                client = core.Client(server)
                native_client = analytics.Client(client.root, timeout=120)
                try:
                    output = ingest.ingest(native_client, proposal)
                except analytics.Blocked:
                    snapshot = client.assemble(release['workflow']['plan'])
                    for calc in [*config['risks'].values(), config['pnl']]:
                        row = client.run(snapshot, calc)
                        result = client.call('GET', '/analytics/results/' + row['result_id'])
                        if result.get('ok') != 'true':
                            print('Issuer ' + kind + ' ' + calc + ': ' + json.dumps(result.get('error')), file=sys.stderr)
                    raise
                require(output == ingest.ingest(native_client, proposal), 'Issuer retry changed its exact release')
                if kind == 'recovery':
                    feed.retain_book(output)
                    with automatic.lost_result_ack(client.root, path_marker='/run/hedge') as (proxy, committed), automatic.WorkerProcess(feed, proxy, folder, plan['known_at']) as worker:
                        require(committed.wait(30), 'Issuer recovery hedge did not commit before ACK')
                        worker.process.kill()
                        worker.process.wait(timeout=5)
                with automatic.WorkerProcess(feed, client.root, folder, plan['known_at']) as worker:
                    worker.wait('unavailable')
                    if kind != 'recovery':
                        feed.send(output)
                    if kind == 'default':
                        blocked = worker.wait('unavailable', 'issuer_recovery_pending')
                        require(blocked['ready'] is False and 'receipts' not in blocked, 'Issuer default exposed priced results')
                        feed.retain_book(output)
                        snapshot = client.assemble(release['workflow']['plan'])
                        for trade in AFFECTED:
                            calc = config['risks'][trade]
                            row = client.run(snapshot, calc)
                            result = client.result(row)
                            require('lifecycle_unpriced' in result['result']['data'] and 'lifecycle' not in result['result']['data'], 'Issuer default invented risk')
                            require(client.server.call(native_request(client.snapshot(snapshot), calc)) == result, 'Issuer unpriced parity failed')
                            audits.append((row, client.result(row, raw=True), calc))
                    else:
                        ready = cds.wait_book(worker, output['payload']['version'])
                        if kind == 'recovery':
                            require(all(r['reused'] for r in ready['receipts'].values()), 'Issuer lost-ACK recovery did not reuse results')
                        more, native = compare(client, ready, config)
                        saved.extend(more)
                        residuals.extend(g['attribution']['residual'] for g in native['pnl']['result']['data']['report']['reporting_totals'])
                        candidates = native['hedge']['result']['data']['exposure']['problem']['candidates']
                        require(all(c['risk']['product'] not in ('cash_bond', 'cds') for c in candidates), 'Defaulted issuer remained hedge-eligible')
                        for trade in AFFECTED:
                            mark = event_example.closing_mark(native, trade)
                            risk = native[config['risks'][trade]]['result']['data']['lifecycle']
                            require(mark['state']['remaining_fraction'] == '0' and risk['default_entity'] == 'ACME', 'Issuer default retained product exposure')
                            require(all(s['base_pv'] == 0 and s['value'] == 0 for s in risk['pending']), 'Issuer default retained pending risk')
                            if kind == 'complete-cash':
                                require(mark['state']['status'] == 'closed', 'Issuer actual receipts did not close claims')
                        unaffected = event_example.closing_mark(native, OTHER)
                        prior_other = event_example.closing_mark(initial, OTHER)
                        require(unaffected['state'] == prior_other['state'] and unaffected['local'] == prior_other['local'],
                                'A legal parent relationship propagated an unreported default')
                        if kind == 'recovery':
                            require(native['hedge']['result']['data']['proposal']['quantities'] != initial['hedge']['result']['data']['proposal']['quantities'], 'Issuer default did not change hedges')
                current = output['payload']
            if kind == 'default':
                with core.supplied.ServerProcess(binary, folder) as server, automatic.WorkerProcess(feed, core.Client(server).root, folder, plan['known_at']) as worker:
                    worker.wait('unavailable')
                    worker.wait('unavailable', 'issuer_recovery_pending')
                    for row, raw, calc in audits:
                        client = core.Client(server)
                        require(client.run(row['snapshot_id'], calc)['reused'] and client.result(row, raw=True) == raw, 'Restart changed issuer unpriced audit')
        with core.supplied.ServerProcess(binary, folder) as server, automatic.WorkerProcess(feed, core.Client(server).root, folder, plan['known_at']) as worker:
            client = core.Client(server)
            worker.wait('unavailable')
            restored = worker.wait('ready')
            require(all(r['reused'] for r in restored['receipts'].values()), 'Issuer restart did not reuse durable results')
            for row, raw in saved:
                require(client.result(row, raw=True) == raw, 'Issuer restart changed stored bytes')
            for calc in ('risk.0', 'risk.5', 'risk.6', 'pnl', 'hedge'):
                row = restored['receipts'][calc]
                again = client.run(row['snapshot_id'], calc, recompute=True)
                require(client.result(row, raw=True) == client.result(again, raw=True), 'Issuer saved-input recomputation changed')
            for row, raw, calc in audits:
                again = client.run(row['snapshot_id'], calc, recompute=True)
                require(client.result(again, raw=True) == raw, 'Issuer audit recomputation invented recovery')
    print(json.dumps(dict(status='passed', data_quality='synthetic historical replay', trades=8, factors=5,
        affected_bonds=2, affected_cds=1, unaffected_legal_entity=True, atomic_issuer_and_book_update=True,
        persisted_priced_results=len(saved), persisted_unpriced_results=len(audits),
        unknown_recovery_blocks_across_restart=True, contract_specific_recovery=True, partial_complete_receipts=True,
        native_parity=True, causal_pnl_reconciled=True, constrained_hedge=True, defaulted_issuer_candidates_excluded=True,
        retained_native_pnl_residuals=residuals,
        post_commit_hedge_ack_kill_recovered=True, server_restart_bytes_cache_recompute_identical=True,
        result_digests={r['result_id']: hashlib.sha256(raw).hexdigest() for r, raw in saved},
        unpriced_result_digests={r['result_id']: hashlib.sha256(raw).hexdigest() for r, raw, _ in audits},
        execution_authority='none', processes_stopped=True), indent=2))


if __name__ == '__main__':
    try:
        main()
    except (RuntimeError, OSError, ValueError, KeyError, IndexError, StopIteration, subprocess.SubprocessError,
            adapter.IngestionError, analytics.Blocked) as error:
        print(str(error), file=sys.stderr)
        raise SystemExit(1)
