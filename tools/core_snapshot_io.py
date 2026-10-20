#!/usr/bin/env python3
"""Measure snapshot read/JSON/integrity costs for existing 36/60-trade books.

Reuse the existing importer, server snapshot port, capacity workload and actual
file adapter. The Rust diagnostic verifies every captured byte/value outside
each timed stage. Financial update/restart checks remain core_update_latency.py.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile

import core_capacity_workflow as capacity

core, transport, ROOT = capacity.core, capacity.transport, capacity.ROOT


def measure(directory, lots, receipts, samples, warmup):
    binaries = [(directory / name).resolve(strict=True)
        for name in ['convex-server', 'convex-core-inputs', 'examples/core_snapshot_io']]
    server_binary, importer, diagnostic = binaries
    value, plan, _ = capacity.workload(lots, receipts)
    fixture = json.loads(capacity.benchmark.FIXTURE.read_bytes())
    command = dict(idempotency_key='snapshot.io.initial', changes=[], master_changes=[dict(expected_version=None,
        definition=dict(kind='calculation', definition=value))])
    with tempfile.TemporaryDirectory(prefix='convex-snapshot-io-') as name:
        folder = Path(name)
        source = folder / 'capture.json'
        source.write_text(json.dumps(dict(schema_version=1,
            references=[dict(recorded_at=plan['known_at'], command=command)], curves=fixture['curves'], fixings=fixture['fixings'])))
        subprocess.run([str(importer), str(folder / 'state.redb'), str(source)], cwd=ROOT, check=True, stdout=subprocess.PIPE)
        with core.supplied.ServerProcess(server_binary, folder) as server:
            client = core.Client(server)
            identity = client.assemble(plan)
            saved = client.snapshot(identity)
        path = folder / 'state.redb.core-snapshots' / (identity.removeprefix('sha256:') + '.core.json')
        result = json.loads(subprocess.check_output([str(diagnostic), str(path), str(samples), str(warmup)], cwd=ROOT))
        core.require(result['status'] == 'passed' and result['snapshot_id'] == identity, 'Wrong admitted diagnostic snapshot')
        core.require(json.loads(path.read_bytes()) == saved, 'Diagnostic did not consume actual port-captured inputs')
        result.update(trades=6 * lots, receipts_per_bond=receipts,
            definition_sha256=hashlib.sha256(capacity.analytics.encoded(value)).hexdigest(),
            exact_file_sha256=hashlib.sha256(path.read_bytes()).hexdigest())
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary-dir', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--samples', type=int, default=7)
    parser.add_argument('--warmup', type=int, default=1)
    args = parser.parse_args()
    if not 3 <= args.samples <= 31 or not 1 <= args.warmup <= 3:
        parser.error('samples must be 3..31 and warmup 1..3')
    cases = [measure(args.binary_dir, lots, receipts, args.samples, args.warmup) for lots, receipts in [(6, 12), (10, 0)]]
    result = dict(status='passed', cases=cases, harness_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        binaries={str((args.binary_dir / name).resolve()): hashlib.sha256((args.binary_dir / name).read_bytes()).hexdigest()
            for name in ['convex-server', 'convex-core-inputs', 'examples/core_snapshot_io']},
        qualification='Sequential warm local diagnostic; use the unchanged update harness separately for pricing/risk/hedge/P&L parity, streaming and restart checks.')
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(dict(status='passed', output=str(args.output), cases=[dict(trades=c['trades'],
        median_ms={k: v['median_ns'] / 1e6 for k, v in c['stages'].items()}) for c in cases]), indent=2))


if __name__ == '__main__':
    main()
