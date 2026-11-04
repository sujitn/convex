#!/usr/bin/env python3
"""Admit explicit date rolls and scheduled cash through existing core ports.

The source supplies full after points, curve/fixing selections and scheduled
native events. This adapter performs no calendar, position or cash arithmetic.
Actual receipts are separate event batches. See DATE-ROLLS.md.
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
import streaming_analytics as analytics


def prepare(client, batch):
    book_capacity.payload_bytes(batch)
    if (not isinstance(batch, dict) or type(batch.get('schema_version')) is not int
            or batch['schema_version'] != 1 or set(batch) != {
                'schema_version', 'id', 'base', 'base_snapshot', 'definition', 'workflow'}):
        raise ValueError('date_roll_schema')
    record = book_updates.resolve(client, batch['base'])
    saved = client.call('GET', '/analytics/snapshots/' + urllib.parse.quote(batch['base_snapshot'], safe=''))
    if (saved['id'] != batch['base_snapshot'] or saved['content']['definition'] != record
            or saved['content']['plan']['definition'] != batch['base']['workflow']['plan']['definition']):
        raise ValueError('date_roll_base_snapshot_mismatch')
    client.run(saved['id'], batch['base']['workflow']['pnl'])
    ordinary = {k: copy.deepcopy(v) for k, v in batch.items() if k != 'base_snapshot'}
    command, release = events.prepare(client, ordinary, date_roll=True, base_plan=saved['content']['plan'])
    command['master_changes'][0]['definition']['definition']['provenance']['evidence_reference'] += (
        '; date roll sha256=' + analytics.digest(batch))
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
        raise ValueError('date_roll_timeout')
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
        print('Date roll HTTP ' + status + '; retain the exact batch and reference receipt.', file=sys.stderr)
        raise SystemExit(1)
    except (ValueError, KeyError, TypeError, IndexError, OverflowError, RecursionError, OSError, analytics.Blocked):
        print('Date roll rejected or unconfirmed; retain the exact batch for retry.', file=sys.stderr)
        raise SystemExit(1)
