#!/usr/bin/env python3
"""Atomically select actual issuer notices and the complete native book update.

Uses the existing reference transaction, event admission and saved-input ports.
The producer supplies explicit legal mappings and each contract's actual terms;
this adapter does not infer defaults, recovery, quantities or cash.
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
import issuer_scope
import market_data_adapter as adapter
import streaming_analytics as analytics


def prepare(client, batch):
    book_capacity.payload_bytes(batch)
    if (not isinstance(batch, dict) or type(batch.get('schema_version')) is not int
            or batch['schema_version'] != 1 or set(batch) != {
                'schema_version', 'id', 'base', 'base_snapshot', 'definition', 'workflow', 'issuer_scope_change'}):
        raise ValueError('issuer_batch_schema')
    original_record = book_updates.resolve(client, batch['base'])
    original = original_record['definition']['definition']
    saved = client.call('GET', '/analytics/snapshots/' + urllib.parse.quote(batch['base_snapshot'], safe=''))
    if (saved['id'] != batch['base_snapshot'] or saved['content']['definition'] != original_record
            or saved['content']['plan']['definition'] != batch['base']['workflow']['plan']['definition']):
        raise ValueError('issuer_base_snapshot_mismatch')
    previous = issuer_scope.captured(saved)
    if previous is None:
        raise ValueError('issuer_base_scope_required')
    # Replay the immutable previously priced base, including on retry after the
    # new scope is committed. Never reassemble an obsolete scope at a later cutoff.
    client.run(saved['id'], batch['base']['workflow']['pnl'])
    change = batch['issuer_scope_change']
    if (set(change) != {'expected_version', 'definition'} or set(change['definition']) != {'kind', 'definition'}
            or change['definition']['kind'] != 'issuer_scope'
            or change['expected_version'] != previous['reference']['version']):
        raise ValueError('issuer_scope_change')
    scope = change['definition']['definition']
    old = saved['content']['issuer_scope']['definition']['definition']
    pin = batch['definition']['reference_inputs'].get('issuer_scope')
    if (scope['id'] != old['id'] or scope['effective'] != old['effective']
            or pin != dict(id=old['id'], version=change['expected_version'] + 1)):
        raise ValueError('issuer_scope_revision')
    a, before = issuer_scope.owners(old['directory'])
    b, after = issuer_scope.owners(scope['directory'])
    if (any(k not in b or b[k]['version'] < v['version']
                or v['default'] is not None and b[k]['default'] != v['default'] for k, v in a.items())
            or any(k not in after or after[k]['entity_id'] != v['entity_id']
                or after[k]['version'] < v['version'] for k, v in before.items())
            or not any(v['default'] is not None and (k not in a or a[k]['default'] is None) for k, v in b.items())):
        raise ValueError('issuer_notice_must_extend_retained_facts')
    issuer_scope.validate_book(batch['definition'], batch['workflow'], scope['directory'])
    ordinary = {k: copy.deepcopy(batch[k]) for k in ('schema_version', 'id', 'base', 'definition', 'workflow')}
    ordinary['definition']['reference_inputs']['issuer_scope'] = copy.deepcopy(previous['reference'])
    # Reuse every existing immutable-prefix, causal and full-risk binding check.
    command, release = events.prepare(client, ordinary)
    proposed = command['master_changes'][0]['definition']['definition']
    proposed['reference_inputs']['issuer_scope'] = copy.deepcopy(pin)
    proposed['provenance']['evidence_reference'] += '; issuer batch sha256=' + analytics.digest(batch)
    command['master_changes'].append(copy.deepcopy(change))
    book_capacity.payload_bytes(command)
    return command, release


def ingest(client, batch):
    command, release = prepare(client, batch)
    return events.admit(client, batch, command, release)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('batch')
    parser.add_argument('--url', required=True)
    parser.add_argument('--token-env', default='CONVEX_TOKEN')
    parser.add_argument('--timeout', type=float, default=10)
    parser.add_argument('--prepare-only', action='store_true')
    args = parser.parse_args()
    if not 0 < args.timeout <= 120:
        raise ValueError('issuer_timeout')
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
        print('Issuer request HTTP ' + status + '; retain the exact batch and reference receipt.', file=sys.stderr)
        raise SystemExit(1)
    except (ValueError, KeyError, TypeError, IndexError, OverflowError, RecursionError, OSError, analytics.Blocked):
        print('Issuer batch rejected or unconfirmed; retain the exact batch for retry.', file=sys.stderr)
        raise SystemExit(1)
