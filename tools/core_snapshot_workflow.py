#!/usr/bin/env python3
"""Run the core book through persisted port-assembled snapshots and restart recovery.

Python assembles requests and checks behavior. All finance executes in the
existing native library through the existing server. Inputs are synthetic.
"""
import argparse
import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import urllib.error
import urllib.parse
import urllib.request

import core_workflow as supplied

ROOT = supplied.ROOT
require = supplied.require


def prepare():
    pnl = supplied.snapshot()
    requests, factors = supplied.risk_requests(pnl)
    updated_book = copy.deepcopy(pnl)
    updated_book['operation']['input']['before'] = copy.deepcopy(updated_book['operation']['input']['steps'][0]['after'])
    updated_requests, _ = supplied.risk_requests(updated_book)
    updated_market = updated_book['operation']['input']['markets'][updated_book['operation']['input']['before']['market']]
    updated_keys = {r['key']['id']: r['key'] for r in updated_market['curves']}
    for request in updated_requests:
        for factor in request['operation']['input']['factors']:
            coordinate = factor['key']['coordinate']
            for key, value in coordinate.items():
                if isinstance(value, dict) and set(value) == {'id', 'version'} and value['id'] in updated_keys:
                    coordinate[key] = copy.deepcopy(updated_keys[value['id']])
    factors = copy.deepcopy(updated_requests[0]['operation']['input']['factors'])
    data = pnl['operation']['input']
    point = data['before']['point']
    cutoff = point['known_at']
    calculations = {}
    reference_inputs = copy.deepcopy(requests[0]['inputs'])
    reference_inputs['curves'] = []
    reference_inputs['fixings'] = []
    opening = 'market.' + str(data['before']['market'])
    for n, request in enumerate(requests):
        request['inputs'] = copy.deepcopy(supplied.EMPTY)
        calculations['risk.' + str(n)] = dict(request=request, curve_sets=[opening])
    records, sets = [], {}
    for n, market in enumerate(data['markets']):
        pins = []
        for recipe in market['curves']:
            record = dict(schema_version=1, curve_id=recipe['key']['id'], version=n + 1,
                source_time=cutoff, recorded_at=cutoff, source='core.synthetic.curves',
                model_id='native.supplied.recipes', input_versions=[], quality='Demo',
                max_age_ms=86400000, recipe=recipe)
            records.append(record)
            pins.append(dict(id=record['curve_id'], version=record['version']))
        sets['market.' + str(n)] = pins
        market['curves'] = []
    fixings = []
    for observation in data['resources']['fixings']:
        fixings.append(dict(schema_version=1,
            key=dict(index_id=observation['index_id'], observation_date=observation['observation_date'],
                     source=observation['source_id']),
            version=observation['revision'], rate=dict(value=observation['rate'], unit='decimal_rate'),
            published_at=observation['published_at'], recorded_at=observation['known_at'],
            is_preliminary=observation['status'] == 'Preliminary', entitled=True,
            evidence_reference='retained synthetic core book; paper calculation only'))
    data['resources']['fixings'] = []
    require(data['resources'] == reference_inputs, 'The core book must share the declared reference inputs')
    data['resources'] = copy.deepcopy(supplied.EMPTY)
    calculations['pnl'] = dict(request=pnl, curve_sets=list(sets))
    definition = dict(id='core.synthetic.book', calculations=calculations, reference_inputs=reference_inputs,
        effective=dict(from_='2026-01-01T00:00:00Z', until=None),
        provenance=dict(source='core.synthetic.reference', evidence_reference='retained full native core contracts'))
    definition['effective']['from'] = definition['effective'].pop('from_')
    command = dict(idempotency_key='core.synthetic.book.initial', changes=[],
        master_changes=[dict(expected_version=None, definition=dict(kind='calculation', definition=definition))])
    next_definition = copy.deepcopy(definition)
    for n, request in enumerate(updated_requests):
        request['inputs'] = copy.deepcopy(supplied.EMPTY)
        next_definition['calculations']['risk.' + str(n)] = dict(request=request, curve_sets=['market.1'])
    next_definition['provenance']['evidence_reference'] = 'retained complete post-market-update native contracts'
    next_command = dict(idempotency_key='core.synthetic.book.updated', changes=[],
        master_changes=[dict(expected_version=1, definition=dict(kind='calculation', definition=next_definition))])
    capture = dict(schema_version=1, references=[dict(recorded_at=cutoff, command=command), dict(recorded_at=cutoff, command=next_command)],
                   curves=records, fixings=fixings)
    plan = dict(definition=dict(id=definition['id'], version=1), as_of=cutoff, known_at=cutoff,
                curve_sets=sets, fixings=[f['key'] for f in fixings])
    return capture, plan, factors


class Client:
    def __init__(self, server):
        self.server = server
        self.root = server.url.removesuffix('/analytics')
        self.opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

    def call(self, method, path, value=None, status=200, raw=False):
        request = urllib.request.Request(self.root + path,
            None if value is None else json.dumps(value).encode(),
            {'Content-Type': 'application/json'}, method=method)
        if path == '/reference/transactions':
            request.add_header('Idempotency-Key', value['idempotency_key'])
        try:
            response = self.opener.open(request, timeout=120)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            body = response.read()
            require(response.status == status,
                    f'{method} {path}: HTTP {response.status}: {body[:1000]!r}')
            require(response.headers.get('x-convex-profile') == 'demo', 'Missing demo profile')
        return body if raw else json.loads(body) if body else None

    def assemble(self, plan):
        return self.call('POST', '/analytics/snapshots', plan)['snapshot_id']

    def run(self, snapshot, name, recompute=False):
        key = urllib.parse.quote(snapshot, safe='')
        return self.call('POST', f'/analytics/snapshots/{key}/run/{name}', dict(recompute=recompute))

    def result(self, receipt, raw=False):
        result = self.call('GET', '/analytics/results/' + receipt['result_id'], raw=raw)
        if not raw:
            require(result.get('ok') == 'true', 'Native ' + receipt['calculation'] + ' failed: ' + json.dumps(result.get('error')))
        return result

    def snapshot(self, identity):
        return self.call('GET', '/analytics/snapshots/' + urllib.parse.quote(identity, safe=''))


def measure(result):
    return result['result']['data']['vector']['sensitivities'][0]['base_pv']


def risks(client, identity):
    receipts = [client.run(identity, 'risk.' + str(n)) for n in range(6)]
    require(all(not r['reused'] for r in receipts), 'New resolved inputs reused an older result')
    results = [client.result(r) for r in receipts]
    return receipts, results


def native_request(snapshot, name):
    """Inspect saved inputs and build the original native wire request for parity."""
    content = snapshot['content']
    definition = content['definition']['definition']['definition']
    calc = definition['calculations'][name]
    request = copy.deepcopy(calc['request'])
    fixings = [dict(index_id=f['key']['index_id'], observation_date=f['key']['observation_date'],
        rate=f['rate']['value'], published_at=f['published_at'], known_at=f['recorded_at'], revision=f['version'],
        status='Preliminary' if f['is_preliminary'] else 'Final', source_id=f['key']['source'])
        for f in content['fixings']]
    recipes = lambda key: [r['recipe'] for r in content['curves'][key]]
    if request['operation']['kind'] == 'graph_hedge_risk':
        request['inputs'] = copy.deepcopy(definition['reference_inputs'])
        request['inputs']['curves'] = recipes(calc['curve_sets'][0])
        request['inputs']['fixings'] = fixings
        request['operation']['input']['snapshot_id'] = snapshot['id']
    elif request['operation']['kind'] == 'book_pnl':
        data = request['operation']['input']
        data['resources'] = copy.deepcopy(definition['reference_inputs'])
        data['resources']['fixings'] = fixings
        for market, key in zip(data['markets'], calc['curve_sets'], strict=True):
            market['curves'] = recipes(key)
    return request


def exercise(client, capture, plan, factors):
    identity = client.assemble(plan)
    require(client.assemble(plan) == identity, 'Unchanged exact inputs changed snapshot identity')
    original = client.snapshot(identity)
    receipts, baseline = risks(client, identity)
    require(client.run(identity, 'risk.0')['reused'], 'Durable result was recalculated')
    for n, result in enumerate(baseline):
        require(client.server.call(native_request(original, 'risk.' + str(n))) == result,
                'Persisted calculation differs from the original native endpoint')
    pnl_receipt = client.run(identity, 'pnl')
    pnl = client.result(pnl_receipt)
    require(client.server.call(native_request(original, 'pnl')) == pnl, 'Native P&L parity failed')
    totals = pnl['result']['data']['report']['reporting_totals']
    for group in totals:
        r = group['attribution']['residual']
        require(all(a['value'] == 0 for a in [r['cash'], r['current_model_pnl'], *r['marking'].values()]),
                'Native causal P&L did not reconcile')
    broken = copy.deepcopy(plan)
    broken['curve_sets']['market.0'][0]['version'] = 999999
    client.call('POST', '/analytics/snapshots', broken, status=404)

    # A new immutable source record carries an existing captured alternative;
    # native curve keys remain scoped to their complete market/snapshot.
    update = copy.deepcopy(next(r for r in capture['curves'] if r['curve_id'] == 'D.OIS' and r['version'] == 2))
    update['version'] = 3
    client.call('POST', '/analytics/curves', update, status=204)
    conflict = copy.deepcopy(update)
    conflict['model_id'] = 'conflicting.immutable.source'
    client.call('POST', '/analytics/curves', conflict, status=409)
    changed = copy.deepcopy(plan)
    changed['definition']['version'] = 2
    next(p for p in changed['curve_sets']['market.1'] if p['id'] == 'D.OIS')['version'] = 3
    curve_identity = client.assemble(changed)
    require(curve_identity != identity, 'Curve update reused old snapshot identity')
    curve_receipts, curve_results = risks(client, curve_identity)
    require(measure(curve_results[0]) != measure(baseline[0]), 'Bond did not consume the updated discount curve')

    # OIS uses LatestKnown; the rate future retains the actual FirstPublished
    # revision. This correction must preserve both immutable observations.
    correction = copy.deepcopy(next(f for f in capture['fixings']
        if f['key']['index_id'] == 'USD.SOFR' and f['key']['observation_date'] == '2026-09-16'))
    correction.update(version=2, rate=dict(value='0.039', unit='decimal_rate'), published_at=plan['known_at'], recorded_at=plan['known_at'])
    client.call('POST', '/analytics/fixings', [correction], status=204)
    fixing_identity = client.assemble(changed)
    require(fixing_identity != curve_identity, 'Fixing correction reused old snapshot identity')
    fixing_receipts, fixing_results = risks(client, fixing_identity)
    by_id = lambda values: {v['result']['data']['vector']['id']: v for v in values}
    before, after = by_id(curve_results), by_id(fixing_results)
    require(measure(before['seasoned-OIS']) != measure(after['seasoned-OIS']), 'OIS did not consume eligible correction')
    require(measure(before['mixed.overnight']) == measure(after['mixed.overnight']),
            'FirstPublished future was incorrectly restated to the latest revision')

    book = supplied.snapshot()
    book['operation']['input']['before'] = copy.deepcopy(book['operation']['input']['steps'][0]['after'])
    point = book['operation']['input']['before']['point']
    point['snapshot_id'] = fixing_identity
    for fx in point['fx']:
        fx['snapshot_id'] = fixing_identity
    hedge = supplied.hedge_request(book, [v['result']['data'] for v in fixing_results], factors)
    definition = dict(id='core.synthetic.hedge', reference_inputs=copy.deepcopy(supplied.EMPTY), calculations=dict(hedge=dict(request=hedge, curve_sets=[])),
        effective={'from': '2026-01-01T00:00:00Z', 'until': None},
        provenance=dict(source='core.synthetic.proposal', evidence_reference='actual stored core risk; paper proposal only'))
    command = dict(idempotency_key='core.synthetic.hedge.initial', changes=[],
        master_changes=[dict(expected_version=None, definition=dict(kind='calculation', definition=definition))])
    commit = client.call('POST', '/reference/transactions', command, status=200)
    hedge_plan = dict(definition=dict(id=definition['id'], version=1), as_of=plan['as_of'],
        known_at=commit['recorded_at'], curve_sets={}, fixings=[])
    hedge_identity = client.assemble(hedge_plan)
    hedge_receipt = client.run(hedge_identity, 'hedge')
    solved = client.result(hedge_receipt)['result']['data']
    require(solved['status'] == 'feasible' and solved['proposal'] is not None, 'Native proposal not feasible')
    require(all(c['satisfied'] for c in solved['proposal']['constraints']), 'Native hedge postcondition failed')

    all_receipts = receipts + curve_receipts + fixing_receipts + [pnl_receipt, hedge_receipt]
    saved = [(r, client.result(r, raw=True)) for r in all_receipts]
    summary = dict(status='passed', data_quality='synthetic', trades=6, factors=5,
        snapshots=[identity, curve_identity, fixing_identity, hedge_identity], persisted_results=len(saved),
        exact_native_parity=True, missing_curve_rejected=True, immutable_curve_conflict_rejected=True,
        curve_update_consumed=True, latest_known_fixing_consumed=True, first_published_future_preserved=True,
        hedge_status=solved['status'], pnl=[dict(basis=g['basis'], currency=g['currency'],
            current_model_pnl=g['attribution']['total']['current_model_pnl']['value'],
            cash=g['attribution']['total']['cash']['value'], residual=g['attribution']['residual']) for g in totals])
    return summary, saved


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary-dir', type=Path, help='Directory containing already built server/import executables')
    args = parser.parse_args()
    directory = args.binary_dir
    if directory is None:
        subprocess.run(['cargo', 'build', '--locked', '-p', 'convex-server', '--bins'], cwd=ROOT, check=True)
        meta = json.loads(subprocess.check_output(['cargo', 'metadata', '--locked', '--no-deps', '--format-version', '1'], cwd=ROOT))
        directory = Path(meta['target_directory']) / 'debug'
    binary = (directory / 'convex-server').resolve(strict=True)
    importer = (directory / 'convex-core-inputs').resolve(strict=True)
    capture, plan, factors = prepare()
    with tempfile.TemporaryDirectory(prefix='convex-core-persisted-') as name:
        folder = Path(name)
        source = folder / 'captured-inputs.json'
        source.write_text(json.dumps(capture))
        subprocess.run([str(importer), str(folder / 'state.redb'), str(source)], cwd=ROOT, check=True,
                       stdout=subprocess.PIPE)
        with supplied.ServerProcess(binary, folder) as server:
            summary, saved = exercise(Client(server), capture, plan, factors)
        source.unlink()  # Recovery must use durable snapshots/results, without reseeding inputs.
        with supplied.ServerProcess(binary, folder) as server:
            client = Client(server)
            for receipt, raw in saved:
                require(client.result(receipt, raw=True) == raw, 'Restart lost or changed a durable result')
                cached = client.run(receipt['snapshot_id'], receipt['calculation'])
                require(cached['reused'] and cached['result_id'] == receipt['result_id'], 'Restart did not recover result cache')
            for receipt, raw in [saved[0], saved[-2], saved[-1]]:
                replay = client.run(receipt['snapshot_id'], receipt['calculation'], recompute=True)
                require(not replay['reused'] and client.result(replay, raw=True) == raw,
                        'Recomputing saved inputs changed the original result after source updates')
    summary.update(restart_results_identical=True, restart_cache_recovered=True,
                   saved_snapshot_recomputation_identical=True, servers_stopped=True)
    print(json.dumps(summary, indent=2))


if __name__ == '__main__':
    try:
        main()
    except (RuntimeError, OSError, ValueError, subprocess.SubprocessError, StopIteration, KeyError) as error:
        print(str(error), file=sys.stderr)
        raise SystemExit(1)
