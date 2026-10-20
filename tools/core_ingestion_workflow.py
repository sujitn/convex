#!/usr/bin/env python3
"""Run provider-neutral ingestion into the persisted core analytics workflow.

Uses the existing complete synthetic book and native checks unchanged. The
adapter is a separate process publishing through the server's existing ports.
"""
import argparse
import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile

import core_snapshot_workflow as core

ROOT = core.ROOT
require = core.require
ADAPTER = ROOT / 'tools/market_data_adapter.py'


def write_capture(path, records):
    path.write_text(''.join(json.dumps(r) + '\n' for r in records))


def record(kind, payload):
    return dict(schema_version=1, kind=kind, payload=payload)


def publish(path, url, expected_status=None):
    completed = subprocess.run([sys.executable, str(ADAPTER), str(path), '--url', url],
                               cwd=ROOT, text=True, capture_output=True, check=False)
    if expected_status is None:
        require(completed.returncode == 0, 'Market-data adapter failed: ' + completed.stderr)
        receipt = json.loads(completed.stdout)
        require(receipt['status'] == 'passed' and receipt['records'] == receipt['acknowledged'],
                'Adapter reported success without all acknowledgements')
    else:
        require(completed.returncode == 1 and not completed.stdout, 'Adapter incorrectly accepted a failed publication')
        receipt = json.loads(completed.stderr)
        require(receipt['http_status'] == expected_status, 'Unexpected adapter rejection: ' + completed.stderr)
    return receipt


class IngestingClient(core.Client):
    """Run the unchanged existing core exercise with its input writes via the CLI.

    Only input delivery is replaced; all requests, native assertions, financial
    controls, curve/fixing updates, hedge construction and P&L stay in that exercise.
    """
    def __init__(self, server, folder, records):
        super().__init__(server)
        self.folder = folder
        self.records = records
        self.feed = folder / 'market.jsonl'

    def call(self, method, path, value=None, status=200, raw=False):
        kinds = {'/analytics/curves': 'curve', '/analytics/fixings': 'fixings'}
        if method != 'POST' or path not in kinds:
            return super().call(method, path, value, status, raw)
        item = record(kinds[path], value)
        if status == 204:
            self.records.append(item)
            write_capture(self.feed, self.records)
            publish(self.feed, self.root)
        else:
            probe = self.folder / 'rejected-market.jsonl'
            write_capture(probe, [item])
            publish(probe, self.root, expected_status=status)
        return None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary-dir', type=Path, help='Directory containing the existing server/import executables')
    args = parser.parse_args()
    directory = args.binary_dir
    if directory is None:
        subprocess.run(['cargo', 'build', '--locked', '-p', 'convex-server', '--bins'], cwd=ROOT, check=True)
        metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--locked', '--no-deps',
                                                      '--format-version', '1'], cwd=ROOT))
        directory = Path(metadata['target_directory']) / 'debug'
    binary = (directory / 'convex-server').resolve(strict=True)
    importer = (directory / 'convex-core-inputs').resolve(strict=True)
    capture, plan, factors = core.prepare()
    records = [record('curve', recipe) for recipe in capture['curves']]
    records.append(record('fixings', capture['fixings']))
    records.append(copy.deepcopy(records[0]))  # Explicit duplicate delivery.
    with tempfile.TemporaryDirectory(prefix='convex-core-ingestion-') as name:
        folder = Path(name)
        # Reference terms use their existing historical import path. Every
        # market input is delivered by the adapter while the server is running.
        source = folder / 'references.json'
        source.write_text(json.dumps(dict(schema_version=1, references=capture['references'], curves=[], fixings=[])))
        subprocess.run([str(importer), str(folder / 'state.redb'), str(source)],
                       cwd=ROOT, check=True, stdout=subprocess.PIPE)
        with core.supplied.ServerProcess(binary, folder) as server:
            client = IngestingClient(server, folder, records)
            client.call('POST', '/analytics/snapshots', plan, status=404)
            # Real redb prefix commit followed by a conflicting immutable record.
            # A failed adapter must not report success or publish a partial snapshot.
            conflict = copy.deepcopy(records[0])
            conflict['payload']['model_id'] += '.conflicting-delivery'
            failed_feed = folder / 'interrupted-market.jsonl'
            write_capture(failed_feed, [records[0], conflict, records[1]])
            failed = publish(failed_feed, client.root, expected_status=409)
            require(failed['acknowledged'] == 1 and failed['line'] == 2,
                    'Failed adapter did not retain its actual acknowledged prefix')
            client.call('POST', '/analytics/snapshots', plan, status=404)
            require(not list((folder / 'state.redb.core-snapshots').iterdir()),
                    'Incomplete ingestion published a partial snapshot')
            write_capture(client.feed, records)
            publish(client.feed, client.root)  # Replays the committed first record.
            summary, saved = core.exercise(client, capture, plan, factors)

            # A rejected fixing batch must not add a revision to the complete
            # latest snapshot. Only immutable provenance is changed in the probe.
            extra = copy.deepcopy(capture['fixings'][0])
            extra['version'] = 99
            conflict = copy.deepcopy(capture['fixings'][0])
            conflict['evidence_reference'] += '; conflicting delivery'
            probe = folder / 'rejected-fixings.jsonl'
            write_capture(probe, [record('fixings', [extra, conflict])])
            publish(probe, client.root, expected_status=409)
            latest = copy.deepcopy(plan)
            latest['definition']['version'] = 2
            next(p for p in latest['curve_sets']['market.1'] if p['id'] == 'D.OIS')['version'] = 3
            require(client.assemble(latest) == summary['snapshots'][2],
                    'Rejected fixing batch changed the accepted snapshot')
        source.unlink()
        with core.supplied.ServerProcess(binary, folder) as server:
            client = core.Client(server)
            for receipt, raw in saved:
                require(client.result(receipt, raw=True) == raw, 'Restart changed a saved native result')
            publish(folder / 'market.jsonl', client.root)  # New adapter process, no local checkpoint.
            require(client.assemble(latest) == summary['snapshots'][2],
                    'Replaying normalized input changed an immutable snapshot')
            for receipt, raw in saved:
                cached = client.run(receipt['snapshot_id'], receipt['calculation'])
                require(cached['reused'] and cached['result_id'] == receipt['result_id']
                        and client.result(cached, raw=True) == raw, 'Recovery changed an accepted result/cache key')
            for receipt, raw in [saved[0], saved[-2], saved[-1]]:
                replay = client.run(receipt['snapshot_id'], receipt['calculation'], recompute=True)
                require(not replay['reused'] and client.result(replay, raw=True) == raw,
                        'Saved-input recomputation changed after ingestion/restart/replay')
    summary.update(ingestion_adapter='normalized JSONL via existing HTTP/curve/fixing ports',
                   normalized_records=len(records), complete_images=True,
                   committed_prefix_recovered=True, duplicate_delivery_idempotent=True,
                   fixing_batch_conflict_no_partial=True, no_partial_snapshot=True,
                   adapter_and_server_restart_recovered=True, restart_results_identical=True,
                   saved_snapshot_recomputation_identical=True, servers_stopped=True)
    print(json.dumps(summary, indent=2))


if __name__ == '__main__':
    try:
        main()
    except (RuntimeError, OSError, ValueError, subprocess.SubprocessError, StopIteration, KeyError) as error:
        print(str(error), file=sys.stderr)
        raise SystemExit(1)
