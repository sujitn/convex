#!/usr/bin/env python3
"""Start a bounded native P&L window from an exact durable closing snapshot.

The producer supplies full after points. The server captures and checks the exact
opening and cumulative carry; lifecycle state is rebuilt by the original owners.
"""
import argparse
import book_capacity
import copy
import json
import os
import sys
import urllib.parse

import book_events as events
import book_updates
import market_data_adapter as adapter
import market_data_stream as stream
import streaming_analytics as analytics
import lifecycle_history


def prepare(client, batch):
    book_capacity.payload_bytes(batch)
    if (not isinstance(batch, dict) or type(batch.get('schema_version')) is not int
            or batch['schema_version'] != 1 or set(batch) != {
                'schema_version', 'id', 'base', 'base_snapshot', 'definition', 'workflow'}):
        raise ValueError('checkpoint_schema')
    analytics.name(batch['id'])
    base = stream.book_record(dict(schema_version=1, kind='book', payload=batch['base']), batch['base']['id'])
    record = book_updates.resolve(client, base)
    previous, config = base['workflow'], batch['workflow']
    saved = client.call('GET', '/analytics/snapshots/' + urllib.parse.quote(batch['base_snapshot'], safe=''))
    if (saved['id'] != batch['base_snapshot'] or saved['content']['definition'] != record
            or saved['content']['plan']['definition'] != previous['plan']['definition']):
        raise ValueError('checkpoint_base_snapshot_mismatch')
    receipt, _ = client.run(saved['id'], previous['pnl'])  # Requires actual successful native P&L.
    original = record['definition']['definition']
    proposed = copy.deepcopy(batch['definition'])
    expected = previous['plan']['definition']
    if (config['plan']['definition'] != dict(id=expected['id'], version=expected['version'] + 1)
            or expected['version'] >= 2**64 - 1 or base['version'] >= 2**64 - 1):
        raise ValueError('checkpoint_revision')
    original = lifecycle_history.expand(original)
    proposed = lifecycle_history.expand(proposed)
    refs = copy.deepcopy(original['reference_inputs'])
    refs.pop('lifecycle_history', None)
    selected = copy.deepcopy(proposed['reference_inputs'])
    selected.pop('lifecycle_history', None)
    refs['pnl_checkpoint'] = dict(snapshot_id=saved['id'], calculation=previous['pnl'], build_id=receipt['build_id'])
    if (proposed['id'] != original['id'] or proposed['effective'] != original['effective']
            or selected != refs):
        raise ValueError('checkpoint_reference_selection')
    # Rebase only the P&L window. Retain orchestration/solver/source policies.
    a, b = copy.deepcopy(previous), copy.deepcopy(config)
    a.pop('plan'); b.pop('plan'); old_bindings, new_bindings = a.pop('bindings'), b.pop('bindings')
    if a != b or any(config['plan'][k] != saved['content']['plan'][k] for k in ('as_of', 'known_at', 'fixings')):
        raise ValueError('checkpoint_workflow_policy_changed')
    for pins in config['plan']['curve_sets'].values():
        if pins not in list(saved['content']['plan']['curve_sets'].values()):
            raise ValueError('checkpoint_curve_selection_changed')
    allowed = [{k: v for k, v in row.items() if k not in ('set', 'follow')} for row in old_bindings]
    if any({k: v for k, v in row.items() if k not in ('set', 'follow')} not in allowed for row in new_bindings):
        raise ValueError('checkpoint_source_policy_changed')
    universe = ({(b['curve_id'], b['source']) for b in config['bindings']},
                {stream.fixing_key(k) for k in config['plan']['fixings']}, base['id'])
    analytics.validate(config, universe)
    old = original['calculations'][previous['pnl']]['request']['operation']['input']
    new = proposed['calculations'][config['pnl']]['request']['operation']['input']
    if not 1 <= len(new['steps']) <= 16 or len(new['standalone']) != len(new['steps']):
        raise ValueError('checkpoint_window_bound')
    opening = events.full_point(new, new['before'])
    closing = events.full_point(old, old['steps'][-1]['after'])
    closing['point']['fills'] = []
    if opening != closing:
        raise ValueError('checkpoint_opening_changed')
    book_updates.bind(config, proposed, len(new['steps']) - 1)
    fingerprint = analytics.digest(batch)
    proposed = lifecycle_history.pack(proposed)
    proposed['provenance']['evidence_reference'] += '; core checkpoint sha256=' + fingerprint
    key = 'core.checkpoint.' + analytics.digest(dict(book=base['id'], batch=batch['id']))
    command = dict(idempotency_key=key, changes=[], master_changes=[dict(expected_version=expected['version'],
        definition=dict(kind='calculation', definition=proposed))])
    release = dict(id=base['id'], version=base['version'] + 1, reference_commit=key,
                   workflow=copy.deepcopy(config), position_step=len(new['steps']) - 1)
    stream.book_record(dict(schema_version=1, kind='book', payload=release), base['id'])
    book_capacity.payload_bytes(command)
    return command, release


def ingest(client, batch):
    command, release = prepare(client, batch)
    # The previous window's step index has no meaning in the new window.
    adapted = copy.deepcopy(batch)
    adapted['base']['position_step'] = None
    return events.admit(client, adapted, command, release)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('batch')
    parser.add_argument('--url', required=True)
    parser.add_argument('--token-env', default='CONVEX_TOKEN')
    parser.add_argument('--timeout', type=float, default=10)
    parser.add_argument('--prepare-only', action='store_true')
    parser.add_argument('--compact-lifecycle', action='store_true', help='Share completed event prefixes in the new checkpoint')
    args = parser.parse_args()
    if not 0 < args.timeout <= 120:
        raise ValueError('checkpoint_timeout')
    client = analytics.Client(args.url, os.environ.get(args.token_env), args.timeout)
    batch = analytics.load(args.batch)
    if args.compact_lifecycle:
        batch['definition'] = lifecycle_history.compact(batch['definition'], batch['workflow']['pnl'])
    if args.prepare_only:
        command, release = prepare(client, batch)
        result = dict(schema_version=1, command=command, release=release)
    else:
        result = ingest(client, batch)
    print(json.dumps(result, allow_nan=False))


if __name__ == '__main__':
    try:
        main()
    except adapter.IngestionError as error:
        status = str(error.http_status) if error.http_status is not None else 'unconfirmed'
        print('Checkpoint HTTP ' + status + '; retain the exact batch for retry.', file=sys.stderr)
        raise SystemExit(1)
    except (ValueError, KeyError, TypeError, IndexError, OverflowError, RecursionError, OSError,
            analytics.Blocked, book_updates.BookInputError):
        print('Checkpoint rejected or unconfirmed; retain the exact batch for retry.', file=sys.stderr)
        raise SystemExit(1)
