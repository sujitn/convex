#!/usr/bin/env python3
"""Admit full native event batches through the existing core/reference ports.

The producer supplies actual fills, lifecycle steps and complete after points.
Native P&L validates their economics. No position, cash or pricing arithmetic is
implemented here. The durable reference receipt is also the retry receipt.
"""
import argparse
import copy
import json
import sys
import urllib.parse

import book_updates
import lifecycle_risk
import market_data_adapter as adapter
import market_data_stream as stream
import streaming_analytics as analytics


def full_point(data, binding):
    """Expand only local wire indices, preserving every original native value."""
    value = copy.deepcopy(binding)
    value['holdings'] = [data['holdings'][i] for i in binding['holdings']]
    value['market'] = data['markets'][binding['market']]
    return value


def facts(data):
    """Stable, scoped identities from the ordered path, never counterfactuals."""
    found = {}
    for binding in [data['before'], *[s['after'] for s in data['steps']]]:
        rows = [('fill', f['trade_id'], f['id'], f) for f in binding['point']['fills']]
        for i in binding['holdings']:
            holding = data['holdings'][i]
            for step in holding['lifecycle']['events']:
                event = step['event']
                rows.append(('lifecycle', event['trade_id'], event['event_id'], step))
                if event['action']['kind'] in ('payment', 'security_receipt'):
                    receipt = event['action']['terms']
                    rows.append(('receipt', event['trade_id'], receipt['receipt_id'], step))
        for kind, trade, identity, row in rows:
            key = (kind, stream.text_id(trade), stream.text_id(identity))
            if key in found and found[key] != row:
                raise ValueError('event_identity_conflict')
            found[key] = row
    if len(found) > 4096:
        raise ValueError('event_identity_bound')
    return found


def prepare(client, batch):
    """Check immutable prefix, exact base pin and full current-risk bindings."""
    adapter.payload_bytes(batch)
    fields = {'schema_version', 'id', 'base', 'definition', 'workflow'}
    if (not isinstance(batch, dict) or type(batch.get('schema_version')) is not int
            or batch['schema_version'] not in (1, 2)
            or set(batch) != fields | ({'resolve_default'} if batch['schema_version'] == 2 else set())
            or batch['schema_version'] == 2 and batch['resolve_default'] is not True):
        raise ValueError('event_batch_schema')
    analytics.name(batch['id'])
    identity = stream.text_id(batch['base']['id'])
    base = stream.book_record(dict(schema_version=1, kind='book', payload=batch['base']), identity)
    config, previous = batch['workflow'], base['workflow']
    universe = ({(b['curve_id'], b['source']) for b in config['bindings']},
                {stream.fixing_key(k) for k in config['plan']['fixings']}, base['id'])
    analytics.validate(config, universe)
    expected = previous['plan']['definition']
    if (set(expected) != {'id', 'version'} or type(expected['version']) is not int
            or not 1 <= expected['version'] < 2**64 - 1):
        raise ValueError('event_batch_base_pin')
    normalized = copy.deepcopy(config)
    normalized['plan']['definition'] = copy.deepcopy(expected)
    if normalized != previous:
        raise ValueError('event_batch_workflow_changed')
    if (config['plan']['definition'] != dict(id=expected['id'], version=expected['version'] + 1)
            or expected['version'] >= 2**64 - 1 or base['version'] >= 2**64 - 1):
        raise ValueError('event_batch_revision')
    original = book_updates.resolve(client, base)['definition']['definition']
    book_updates.bind(previous, original, base['position_step'])
    proposed = copy.deepcopy(batch['definition'])
    if (proposed['id'] != original['id'] or proposed['effective'] != original['effective']
            or proposed['reference_inputs'] != original['reference_inputs']):
        raise ValueError('event_batch_reference_changed')
    old = original['calculations'][previous['pnl']]['request']['operation']['input']
    new = proposed['calculations'][config['pnl']]['request']['operation']['input']
    count = len(old['steps'])
    resolving = batch['schema_version'] == 2
    if resolving:
        # Only an actually unpriced closing default may be consolidated. Its
        # original complete definition remains immutable in the reference port.
        pending = pending_defaults(client, base, original)
        if not pending or old['steps'][-1]['cause'] != 'event':
            raise ValueError('default_resolution_requires_unpriced_base')
    common = count - 1 if resolving else count
    if (not 1 <= count <= len(new['steps']) <= 16 or not resolving and count == len(new['steps'])
            or len(new['standalone']) != len(new['steps'])
            or {k: v for k, v in old.items() if k not in ('before', 'steps', 'standalone', 'holdings')}
            != {k: v for k, v in new.items() if k not in ('before', 'steps', 'standalone', 'holdings')}):
        raise ValueError('event_batch_path_extension')
    if full_point(old, old['before']) != full_point(new, new['before']):
        raise ValueError('event_batch_opening_changed')
    for i in range(common):
        a, b = old['steps'][i], new['steps'][i]
        sa, sb = copy.deepcopy(old['standalone'][i]), copy.deepcopy(new['standalone'][i])
        if sa['kind'] == 'available':
            sa['information'] = full_point(old, sa['information'])
        if sb['kind'] == 'available':
            sb['information'] = full_point(new, sb['information'])
        if (a['cause'] != b['cause'] or full_point(old, a['after']) != full_point(new, b['after']) or sa != sb):
            raise ValueError('event_batch_history_changed')
    allowed = ('market', 'fx', 'position', 'event', 'time') if resolving else ('position', 'event', 'time')
    if any(s['cause'] not in allowed for s in new['steps'][common:]):
        raise ValueError('event_batch_cause')
    before, after = facts(old), facts(new)
    if any(after.get(k) != v for k, v in before.items()) or not set(after) - set(before):
        raise ValueError('event_batch_requires_new_immutable_facts')
    if resolving:
        recoveries = [v['event']['action']['terms']['default_event'] for k, v in after.items()
                      if k[0] == 'lifecycle' and k not in before and v['event']['action']['kind'] == 'recovery']
        if not {v['state']['default_event'] for v in pending} <= set(recoveries):
            raise ValueError('default_resolution_requires_actual_recovery')
    book_updates.bind(config, proposed, len(new['steps']) - 1)
    # Include orchestration too: reusing a batch ID with changed policy, source
    # book version or economics must conflict at the durable idempotency owner.
    fingerprint = analytics.digest(batch)
    provenance = proposed['provenance']
    provenance['evidence_reference'] += '; core event batch sha256=' + fingerprint
    key = 'core.events.' + analytics.digest(dict(book=base['id'], batch=batch['id']))
    command = dict(idempotency_key=key, changes=[], master_changes=[dict(expected_version=expected['version'],
        definition=dict(kind='calculation', definition=proposed))])
    release = dict(id=base['id'], version=base['version'] + 1, reference_commit=key,
                   workflow=copy.deepcopy(config), position_step=len(new['steps']) - 1)
    stream.book_record(dict(schema_version=1, kind='book', payload=release), base['id'])
    adapter.payload_bytes(command)
    return command, release


def risk_scope(report, close, previous_step=None, covered=()):
    """Fail closed where standalone product risk cannot represent lifecycle risk.

    Check holdings affected since the exact previously released closing point.
    Unchanged holdings retain their prior native owners. Current active products
    and due nominal claims use existing product risk. New ended/partial/notice
    exposure or future claims require full lifecycle risk; never release them
    with unchanged original product vectors.
    """
    trades = report['trades']
    if {t['id'] for t in trades} != {p['identity']['id'] for p in close['positions']}:
        raise ValueError('event_risk_coverage')
    for trade in trades:
        if trade['id'] in covered:
            continue  # Complete native lifecycle risk has matched this exact P&L state.
        state = trade['marks'][len(report['input']['causes'])]['state']
        position = next(p for p in close['positions'] if p['identity']['id'] == trade['id'])
        if previous_step is not None:
            old = report['input']['points'][previous_step + 1]
            prior = copy.deepcopy(next(p for p in old['positions'] if p['identity']['id'] == trade['id']))
            # Native causal admission treats these two per-point declarations
            # as metadata; full actual product/state changes are checked there.
            for key in ('economics', 'calculation'):
                if key in position['identity']:
                    prior['identity'][key] = position['identity'][key]
            if (state == trade['marks'][previous_step + 1]['state'] and position == prior
                    and close['valuation_date'] == old['valuation_date']):
                continue
        if (state['status'] != 'active' or state['remaining_fraction'] != '1'
                or state['exercise_notices'] or state['delivery_pending'] is not None
                or any(o['due_date'] > close['valuation_date'] for o in state['obligations'])
                or (state['obligations'] and position['overdue'] != 'nominal')):
            raise ValueError('event_lifecycle_risk_unavailable')


def pending_defaults(client, release, definition):
    """Replay durable native default audits, without accepting a priced result."""
    config = release['workflow']
    record = book_updates.resolve(client, release)
    plan = copy.deepcopy(config['plan'])
    plan['known_at'] = analytics.iso(max(analytics.timestamp(plan['known_at']), analytics.timestamp(record['recorded_at'])))
    snapshot = client.assemble(plan)
    data = definition['calculations'][config['pnl']]['request']['operation']['input']
    found = []
    for calc in config['risks'].values():
        operation = definition['calculations'][calc]['request']['operation']
        if operation['kind'] == 'lifecycle_hedge_risk':
            _, risk = client.run(snapshot, calc)
            if risk['input']['snapshot_id'] != snapshot:
                raise ValueError('lifecycle_risk_identity_mismatch')
            if 'lifecycle_unpriced' in risk:
                found.append(lifecycle_risk.checked_pending(risk, operation, data))
    return found


def ingest(client, batch):
    """Retry the identical command after uncertainty; release only checked P&L.

    No local event database or cursor is required. Reference CAS/idempotence,
    immutable snapshots/results and the existing stream book image own recovery.
    """
    command, release = prepare(client, batch)
    commit = client.call('POST', '/reference/transactions', command)
    record = book_updates.resolve(client, release)
    expected = command['master_changes'][0]['definition']
    if (record['definition'] != expected or commit['recorded_at'] != record['recorded_at']
            or commit['idempotency_key'] != command['idempotency_key']):
        raise ValueError('event_commit_mismatch')
    plan = copy.deepcopy(release['workflow']['plan'])
    plan['known_at'] = analytics.iso(max(analytics.timestamp(plan['known_at']), analytics.timestamp(record['recorded_at'])))
    snapshot = client.assemble(plan)
    data = batch['definition']['calculations'][release['workflow']['pnl']]['request']['operation']['input']
    covered = set()
    pending = []
    risks = []
    for identity, calc in release['workflow']['risks'].items():
        operation = batch['definition']['calculations'][calc]['request']['operation']
        if operation['kind'] == 'lifecycle_hedge_risk':
            _, risk = client.run(snapshot, calc)
            if risk['vector']['id'] != identity or risk['input']['snapshot_id'] != snapshot:
                raise ValueError('lifecycle_risk_identity_mismatch')
            if 'lifecycle_unpriced' in risk:
                pending.append(lifecycle_risk.checked_pending(risk, operation, data))
            else:
                risks.append(risk)
            covered.add(identity)
    if pending:
        if batch['schema_version'] == 2:
            raise ValueError('default_resolution_incomplete')
        # Publish the exact new input selection so the worker latches unavailable
        # and withdraws old readiness. No recovery PV, risk or P&L is invented.
        return dict(schema_version=1, kind='book', payload=release)
    _, result = client.run(snapshot, release['workflow']['pnl'])
    for risk in risks:
        lifecycle_risk.checked_state(risk, result['report'])
    risk_scope(result['report'], data['steps'][-1]['after']['point'], batch['base']['position_step'], covered)
    return dict(schema_version=1, kind='book', payload=release)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('batch')
    parser.add_argument('--url', required=True)
    parser.add_argument('--token-env', default='CONVEX_TOKEN')
    parser.add_argument('--timeout', type=float, default=10)
    parser.add_argument('--prepare-only', action='store_true', help='Validate and print the command/release without writing')
    args = parser.parse_args()
    if not 0 < args.timeout <= 120:
        raise ValueError('event_timeout')
    import os
    client = analytics.Client(args.url, os.environ.get(args.token_env), args.timeout)
    batch = analytics.load(args.batch)
    if args.prepare_only:
        command, release = prepare(client, batch)
        output = dict(schema_version=1, command=command, release=release)
    else:
        output = ingest(client, batch)
    print(json.dumps(output, allow_nan=False))


if __name__ == '__main__':
    try:
        main()
    except adapter.IngestionError as error:
        status = str(error.http_status) if error.http_status is not None else 'unconfirmed'
        print('Core event request HTTP ' + status + '; retain the exact batch and reference receipt.', file=sys.stderr)
        raise SystemExit(1)
    except (ValueError, KeyError, TypeError, IndexError, OverflowError, RecursionError, OSError,
            analytics.Blocked):
        print('Core event batch rejected or unconfirmed; retain the exact batch for retry.', file=sys.stderr)
        raise SystemExit(1)
