#!/usr/bin/env python3
"""Measure native replay and the existing durable core path on supplied mixed lots.

Synthetic economics come from the retained core book. Split coupon receipts are
explicit valid facts; large receipt counts are stress cases, not an observed
production distribution. No provider, financial formula or runtime policy is added.
"""
import argparse
import copy
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import tempfile

import core_roll_workflow as rolls
import lifecycle_history
import lifecycle_risk

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / 'tests/fixtures/library/date-rolls-core.json'


def renamed(value, names):
    if isinstance(value, dict):
        return {key: renamed(child, names) for key, child in value.items()}
    if isinstance(value, list):
        return [renamed(child, names) for child in value]
    if isinstance(value, str):
        for original, replacement in names.items():
            value = value.replace(original, replacement)
    return value


def workload(lots, receipts):
    if type(lots) is not int or not 1 <= lots <= 6 or type(receipts) is not int or not 0 <= receipts <= 2048:
        raise ValueError('benchmark_workload_bound')
    original = json.loads(FIXTURE.read_bytes())
    value = copy.deepcopy(original['definition'])
    old = value['calculations']['pnl']['request']['operation']['input']
    closing = copy.deepcopy(old['steps'][-1]['after'])
    set_name = value['calculations']['pnl']['curve_sets'][closing['market']]
    templates = [old['holdings'][i] for i in closing['holdings']]
    identities = [holding['id'] for holding in templates]
    specs = {c['request']['operation']['input']['identity']['id']: c
             for name, c in value['calculations'].items() if name != 'pnl'}
    holdings, positions, calculations, risk_names = [], [], {}, {}
    for lot in range(lots):
        names = {identity: f'bench.lot.{lot}.{identity}' for identity in identities}
        for template, position in zip(templates, closing['point']['positions'], strict=True):
            holding, position = renamed(template, names), renamed(position, names)
            life = holding['lifecycle']
            if template['id'] == rolls.BOND:
                # A USD1m face holding: original contract/reference face stays1000.
                life['position']['value'] = '-1000000'
                position['current']['quantity'] = '-1000000'
                for index in range(receipts):
                    identity = f'bench.lot.{lot}.receipt.{index}'
                    terms = dict(receipt_id=identity,
                        obligation=dict(kind='bond_coupon', effective_date='2026-10-08', ordinal=0),
                        amount=dict(value='-1', currency='USD', unit='currency'),
                        value_date='2026-10-09', source='benchmark.supplied.split-settlement')
                    life['events'].append(rolls.event(holding['id'], dict(kind='payment', terms=terms),
                        identity+'.event', len(life['events']), '2026-10-09', closing['point']['known_at']))
            name = f'risk.{len(holdings)}'
            calculations[name] = renamed(specs[template['id']], names)
            calculations[name]['curve_sets'] = ['current']
            risk_names[holding['id']] = name
            holdings.append(holding)
            positions.append(position)
    before = copy.deepcopy(closing)
    before.update(market=0, holdings=list(range(len(holdings))))
    before['point'].update(positions=positions, fills=[])
    def identify(binding, identity):
        binding['point'].update(snapshot_id=identity, manifest=dict(id=identity, version=1))
        for fx in binding['point']['fx']:
            fx['snapshot_id'] = identity
    identify(before, f'benchmark.{lots}.{receipts}.before')
    after = copy.deepcopy(before)
    identify(after, f'benchmark.{lots}.{receipts}.after')
    after['point']['positions'][1]['pending_quantity'] = '-74000'
    old.update(before=before, steps=[dict(cause='position', after=after)], holdings=holdings,
        markets=[old['markets'][closing['market']]],
        standalone=[dict(kind='unavailable', reason='Ordered actual book position update')])
    value['calculations'] = dict(pnl=value['calculations']['pnl'], **calculations)
    value['calculations']['pnl']['curve_sets'] = ['current']
    identity = f'benchmark.lots.{lots}.receipts.{receipts}'
    value['id'] = identity
    value['provenance']['evidence_reference'] = 'Synthetic mixed lots; supplied split receipts; no market certification'
    value = lifecycle_risk.bind_requests(value, dict(pnl='pnl', risks=risk_names),
        [h['id'] for h in holdings if h['product']['product']['kind'] in ('bond', 'swap', 'cds')])
    plan = copy.deepcopy(original['plan'])
    plan.update(definition=dict(id=identity, version=1), curve_sets=dict(current=plan['curve_sets'][set_name]))
    resources = copy.deepcopy(value['reference_inputs'])
    replays = [dict(schema_version=1, inputs=resources,
        operation=dict(kind='lifecycle', input=copy.deepcopy(holdings[0]['lifecycle'])))]
    # Direct native parity/timings consume exactly the same port-selected values.
    saved = dict(id='benchmark.native.context', content=dict(plan=plan, curves=dict(current=[
        r for pin in plan['curve_sets']['current'] for r in original['curves']
        if r['curve_id'] == pin['id'] and r['version'] == pin['version']]),
        fixings=sorted(original['fixings'], key=lambda f: (f['key']['index_id'], f['key']['observation_date'], f['key']['source'], f['version'])),
        definition=dict(definition=dict(definition=value))))
    requests = {name: rolls.native_request(saved, name) for name in value['calculations']}
    next_value = copy.deepcopy(value)
    data = next_value['calculations']['pnl']['request']['operation']['input']
    data['before'] = copy.deepcopy(data['steps'][-1]['after'])
    identify(data['steps'][0]['after'], f'benchmark.{lots}.{receipts}.next')
    data['steps'][0]['after']['point']['positions'][1]['pending_quantity'] = '-73000'
    next_value['reference_inputs']['pnl_checkpoint'] = dict(snapshot_id='supplied.at.runtime', calculation='pnl', build_id='supplied.at.runtime')
    next_value = lifecycle_risk.bind_requests(next_value, dict(pnl='pnl', risks=risk_names),
        [h['id'] for h in holdings if h['product']['product']['kind'] in ('bond', 'swap', 'cds')])
    return dict(id=identity, trades=len(holdings), receipts_per_bond=receipts,
        lifecycle_events=sum(len(h['lifecycle']['events']) for h in holdings),
        classification='representative synthetic lots' if receipts <= 12 else 'split-receipt history stress',
        definition=value, next_definition=next_value, compact_definition=lifecycle_history.compact(next_value),
        plan=plan, requests=requests, replays=replays)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path)
    parser.add_argument('--samples', type=int, default=7)
    parser.add_argument('--warmup', type=int, default=1)
    parser.add_argument('--quick', action='store_true', help='6/12trade and0/12receipt cases only')
    parser.add_argument('--output', type=Path)
    parser.add_argument('--workload-directory', type=Path, help='Retain each exact input and raw result')
    args = parser.parse_args()
    if not 3 <= args.samples <= 21 or not 0 <= args.warmup <= 3:
        raise ValueError('benchmark_sample_bound')
    cases = [(lots, receipts) for lots in ([1, 2] if args.quick else [1, 2, 4, 5, 6])
             for receipts in ([0, 12] if args.quick else [0, 12, 120])]
    if not args.quick:
        cases += [(1, n) for n in (512, 1024)]
    fixture = json.loads(FIXTURE.read_bytes())
    binary = args.binary
    if binary is None:
        subprocess.run(['cargo', 'build', '--locked', '--profile', 'ci', '-p', 'convex-engine',
                        '--example', 'core_replay_benchmark'], cwd=ROOT, check=True)
        meta = json.loads(subprocess.check_output(['cargo', 'metadata', '--locked', '--no-deps', '--format-version', '1'], cwd=ROOT))
        binary = Path(meta['target_directory'])/'ci/examples/core_replay_benchmark'
    result, workload_hashes = None, {}
    if args.workload_directory:
        args.workload_directory.mkdir(exist_ok=False, parents=True)
    for lots, receipts in cases:
        case = workload(lots, receipts)
        payload = dict(schema_version=1, samples=args.samples, warmup=args.warmup,
            curves=fixture['curves'], fixings=fixture['fixings'], cases=[case])
        raw = json.dumps(payload, allow_nan=False, separators=(',', ':')).encode()
        if args.workload_directory:
            (args.workload_directory/(case['id']+'.input.json')).write_bytes(raw)
        with tempfile.TemporaryDirectory(prefix='convex-core-measurement-') as folder:
            path = Path(folder)/'workload.json'
            path.write_bytes(raw)
            measured = subprocess.check_output([str(binary.resolve(strict=True)), str(path)], cwd=ROOT)
        if args.workload_directory:
            (args.workload_directory/(case['id']+'.result.json')).write_bytes(measured)
        workload_hashes[case['id']] = hashlib.sha256(raw).hexdigest()
        current = json.loads(measured)
        if result is None:
            result = current
        else:
            result['cases'].extend(current['cases'])
    result['provenance'] = dict(workload_hashes=workload_hashes,
        fixture_sha256=hashlib.sha256(FIXTURE.read_bytes()).hexdigest(),
        binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(), host=platform.platform(),
        architecture=platform.machine(), logical_cpus=os.cpu_count(),
        rustc=subprocess.check_output(['rustc', '--version'], text=True).strip(),
        timer='Rust monotonic Instant; sequential; warm OS cache; raw samples retained',
        build='existing ci profile when built by this command; supplied binary requires separate build provenance',
        target=os.environ.get('CARGO_TARGET_DIR'), data='synthetic supplied economics; no live-provider or latency certification')
    text = json.dumps(result, indent=2, allow_nan=False)+'\n'
    if args.output:
        args.output.write_text(text)
    print(text, end='')


if __name__ == '__main__':
    main()
