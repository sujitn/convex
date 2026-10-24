#!/usr/bin/env python3
"""Run a supplied core book through the existing HTTP server and native analytics.

Only request assembly and transport live here. Pricing, finite-difference risk,
aggregation, constrained hedging and causal P&L execute in the Rust library.
"""
import argparse
import copy
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import tempfile
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / 'tests/fixtures/library'
CORE = {'mixed.bond', 'stepped-amortizing-IRS', 'seasoned-OIS', 'graph-hedge.CDS',
        'mixed.overnight', 'mixed.term'}
EMPTY = dict(calendars=[], helper_calendars=[], curves=[], calibration=None,
             fixings=[], publications=[])


def require(condition, reason):
    if not condition:
        raise RuntimeError(reason)


def case(filename, identity):
    values = json.loads((FIXTURES / filename).read_text())['cases']
    return copy.deepcopy(next(c['request'] for c in values if c['id'] == identity))


def snapshot():
    """Select a complete six-trade core book; retain original terms and cash."""
    request = case('causal-pnl-v12-api-01.json', 'causal.full.mixed')
    data = request['operation']['input']
    phases = [(step, standalone) for step, standalone in
              zip(data['steps'], data['standalone'], strict=True) if step['cause'] != 'model']
    # The retained model phase changes only the excluded optional bond. A
    # metadata-only phase is correctly rejected by the native causal admission.
    data['steps'] = [step for step, _ in phases]
    data['standalone'] = [standalone for _, standalone in phases]
    bindings = [data['before'], *[s['after'] for s in data['steps']],
                *[s['information'] for s in data['standalone'] if s['kind'] == 'available']]
    for binding in bindings:
        point = binding['point']
        selected = [(p, i) for p, i in zip(point['positions'], binding['holdings'], strict=True)
                    if p['identity']['id'] in CORE]
        require(len(selected) == len(CORE), 'Incomplete core book in retained input')
        point['positions'] = [p for p, _ in selected]
        binding['holdings'] = [i for _, i in selected]
        point['fills'] = [f for f in point['fills'] if f['trade_id'] in CORE]
        point['snapshot_id'] = 'core.' + point['snapshot_id']
        point['manifest']['id'] = 'core.' + point['manifest']['id']
        for fx in point['fx']:
            fx['snapshot_id'] = point['snapshot_id']
    used = sorted({i for b in bindings for i in b['holdings']})
    remap = {old: new for new, old in enumerate(used)}
    data['holdings'] = [data['holdings'][i] for i in used]
    for b in bindings:
        b['holdings'] = [remap[i] for i in b['holdings']]
    # These are the complete explicitly named index families in this selected
    # core book. Optional-bond TERM6M history is outside its declared scope.
    indices = {'USD.TERM3M', 'USD.SOFR'}
    data['resources']['publications'] = [r for r in data['resources']['publications']
                                        if r['index_id'] in indices]
    data['resources']['fixings'] = [r for r in data['resources']['fixings']
                                   if r['index_id'] in indices]
    return request


def risk_requests(pnl):
    data = pnl['operation']['input']
    opening = data['before']
    market = data['markets'][opening['market']]
    resources = copy.deepcopy(data['resources'])
    resources['curves'] = market['curves']
    resources['fixings'].extend(opening['additional_fixings'])
    resources['publications'].extend(opening['additional_publications'])
    publications = {}
    for entry in resources['publications']:
        identity = (entry['index_id'], entry['observation_date'])
        require(identity not in publications or publications[identity] == entry,
                'Conflicting supplied publication decisions')
        publications[identity] = entry
    resources['publications'] = list(publications.values())
    source = case('mixed-scenario-api-01.json', 'mixed.full.opposing')
    # All five original factors remain explicit, including recovery. No missing
    # factor becomes zero; the original native owners calculate every column.
    keys = source['operation']['action']['input']['book']['moves']
    factors = [dict(key=keys[i]['key'], settings=dict(bump=0.0001,
               stability=dict(absolute=0.05, relative=0.0001), policy_version=1))
               for i in [0, 1, 2, 4, 5]]
    graph = market['calibration'] or dict(rates=[], credit=[], cash=[], rate_context=None,
                                       controls=source['operation']['input']['controls'])
    requests = []
    for position, index in zip(opening['point']['positions'], opening['holdings'], strict=True):
        holding = data['holdings'][index]
        require(holding['id'] == position['identity']['id'], 'Holding binding mismatch')
        require(holding['product']['kind'] == 'product', 'Core product owner missing')
        reference = copy.deepcopy(position['current'])
        reference['quantity'] = reference['reference_quantity']
        requests.append(dict(schema_version=1, inputs=resources, operation=dict(
            kind='graph_hedge_risk', graph=graph, product=holding['product']['product'],
            input=dict(identity=position['identity'], reference_position=reference,
                       snapshot_id=opening['point']['snapshot_id'], factors=factors,
                       max_factor_evaluations=25))))
    return requests, factors


def hedge_request(pnl, risks, factors):
    data = pnl['operation']['input']
    point = data['before']['point']
    holdings = []
    for position, result in zip(point['positions'], risks, strict=True):
        vector = copy.deepcopy(result['vector'])
        vector['fx'] = next(f for f in point['fx']
                            if f['from'] == vector['reference_position']['currency']
                            and f['to'] == data['reporting_currency'])
        holdings.append(dict(risk=vector, current_quantity=position['current']['quantity'],
                             pending_quantity=position['pending_quantity']))
    credit = next(h['risk']['credit_scope'][0] for h in holdings if h['risk']['id'] == 'graph-hedge.CDS')
    constraints = []
    for factor in factors:
        key = factor['key']
        constraints.append(dict(key=key, risk_settings=factor['settings'],
            credit=credit if key['factor']['family'] in ['hazard', 'recovery'] else None,
            scale=10000.0 if key['unit'] in ['currency_per_annual_rate', 'currency_per_fraction'] else 1.0,
            weight=1.0, target=0.0, lower=-1e9, upper=1e9))
    candidates = []
    # Explicit synthetic proposal policy: original native risk, broad paper
    # limits, zero transaction-cost assumptions and whole futures contracts.
    for identity, limit, holding_limit, lot in [
        ('mixed.bond', '5000', '50000', None),
        ('stepped-amortizing-IRS', '1000000', '3000000', None),
        ('graph-hedge.CDS', '1000000', '3000000', None),
        ('mixed.overnight', '10', '20', '1'),
    ]:
        risk = next(h['risk'] for h in holdings if h['risk']['id'] == identity)
        candidates.append(dict(risk=risk,
            eligibility=dict(eligible=True, decision=dict(id='core.synthetic.proposal-policy', version=1),
                             reason='Explicit synthetic paper-proposal eligibility; no execution permission'),
            trade_lower='-' + limit, trade_upper=limit,
            holding_lower='-' + holding_limit, holding_upper=holding_limit,
            buy_available=limit, sell_available=limit, lot=lot,
            costs=dict(currency='USD', buy_per_reference='0', sell_per_reference='0',
                       observation=dict(id='core.synthetic.zero-cost-assumption', version=1))))
    problem = dict(snapshot_id=point['snapshot_id'], entity=point['positions'][0]['identity']['entity'],
        settlement=point['valuation_date'], reporting_currency=data['reporting_currency'],
        position_state=point['manifest'], expected_holdings=[h['risk']['id'] for h in holdings],
        holdings=holdings, factors=constraints, candidates=candidates, max_cost=0.0,
        settings=dict(policy_version=1, rank_relative_tolerance=1e-10, max_condition=1e4,
            solver_tolerance=1e-10, factor_tolerance=1e-8, cost_tolerance=1e-8,
            cost_scale=1.0, cost_weight=0.0, max_work=100000, svd_iterations=1000))
    return dict(schema_version=1, inputs=EMPTY,
                operation=dict(kind='hedge', problem=problem, action=dict(kind='solve')))


def calculate(call):
    """Every financial result below comes back through the server endpoint."""
    pnl = snapshot()
    requests, factors = risk_requests(pnl)
    prices, risks = [], []
    for request in requests:
        result = call(request)['result']['data']
        risks.append(result)
        vector = result['vector']
        prices.append(dict(trade=vector['id'], basis=result['basis'],
            reference_measure=vector['sensitivities'][0]['base_pv'],
            reference_position=vector['reference_position']))
    hedge = call(hedge_request(pnl, risks, factors))['result']['data']
    require(hedge['status'] == 'feasible' and hedge['proposal'] is not None,
            'Native hedge solver did not produce a feasible proposal: ' + hedge['status'])
    require(all(c['satisfied'] for c in hedge['proposal']['constraints']), 'Hedge postcondition failed')
    result = call(pnl)
    report = result['result']['data']['report']
    causes = [step['cause'] for step in pnl['operation']['input']['steps']]
    totals = []
    for group in report['reporting_totals']:
        attribution = group['attribution']
        residual = attribution['residual']
        require(all(amount['value'] == 0 for amount in
                    [residual['cash'], residual['current_model_pnl'], *residual['marking'].values()]),
                'Native causal P&L did not reconcile for this example')
        totals.append(dict(basis=group['basis'], currency=group['currency'],
            total=attribution['total'],
            ordered=[dict(cause=cause, current_model_pnl=change['current_model_pnl']['value'],
                          cash=change['cash']['value'])
                     for cause, change in zip(causes, attribution['ordered'], strict=True)],
            residual=residual))
    broken = copy.deepcopy(requests[0])
    broken['inputs']['curves'] = [c for c in broken['inputs']['curves'] if c['key']['id'] != 'D.OIS']
    failure = call(broken, success=False)
    require(failure.get('ok') == 'false' and failure['error']['code'] == 'missing_input',
            'Missing curve did not produce the expected native missing-input error')
    return dict(status='passed', data_quality='synthetic', snapshot_id=pnl['operation']['input']['before']['point']['snapshot_id'],
        input_sha256=hashlib.sha256(json.dumps(pnl, sort_keys=True, separators=(',', ':')).encode()).hexdigest(),
        prices=prices, factor_count=len(factors), factor_keys=[f['key'] for f in factors],
        factor_risk=dict(currency=hedge['exposure']['problem']['reporting_currency'],
                         before=hedge['exposure']['before'], after=hedge['proposal']['after']),
        hedge=dict(status=hedge['status'], authority=hedge['authority'],
                   candidates=[c['risk']['id'] for c in hedge['exposure']['problem']['candidates']],
                   quantities=hedge['proposal']['quantities'], residual=hedge['proposal']['residual']),
        pnl_reporting_totals=totals, missing_input_error=failure['error']['code']), pnl, result


class ServerProcess:
    def __init__(self, binary, directory):
        self.binary, self.directory = binary, directory
        self.process = None

    def __enter__(self):
        config = self.directory / 'server.toml'
        config.write_text('profile = "demo"\nhost = "127.0.0.1"\nport = 0\n'
            'websocket_enabled = false\nstorage_path = ' + json.dumps(str(self.directory / 'state.redb')) +
            '\n[access]\nmode = "local_development"\n')
        self.log = (self.directory / 'server.log').open('w+')
        env = {k: v for k, v in os.environ.items() if not k.startswith('CONVEX_')}
        env['RUST_LOG'] = 'info'
        self.process = subprocess.Popen([str(self.binary), str(config)], cwd=ROOT,
                                        env=env, stdout=self.log, stderr=self.log)
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            text = (self.directory / 'server.log').read_text()
            address = re.search(r'Starting Convex server on (127\.0\.0\.1:\d+)', text)
            if address:
                self.url = 'http://' + address[1] + '/api/v1/analytics'
                return self
            if self.process.poll() is not None:
                self.log.close()
                raise RuntimeError('Server startup failed: ' + text[-4000:])
            time.sleep(0.05)
        self.__exit__(None, None, None)
        raise RuntimeError('Server did not start within 30 seconds')

    def __exit__(self, *_):
        if self.process and self.process.poll() is None:
            self.process.send_signal(signal.SIGTERM)
            try:
                self.process.wait(timeout=15)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait()
                raise RuntimeError('Server did not drain within 15 seconds')
        self.log.close()
        require(self.process.returncode == 0, 'Server shutdown failed')

    def call(self, value, success=True):
        request = urllib.request.Request(self.url, json.dumps(value).encode(),
                                         {'Content-Type': 'application/json'})
        # All connections are task-owned localhost; ignore machine proxy settings.
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        with opener.open(request, timeout=120) as response:
            require(response.headers.get('x-convex-profile') == 'demo', 'Missing demo label')
            result = json.load(response)
        if success:
            require(result.get('ok') == 'true', 'Analytics failed: ' + json.dumps(result.get('error')))
        return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, help='Use an already built convex-server')
    parser.add_argument('--pnl-output', type=Path, help='Save the complete native P&L JSON envelope')
    args = parser.parse_args()
    binary = args.binary
    if binary is None:
        subprocess.run(['cargo', 'build', '--locked', '-p', 'convex-server'], cwd=ROOT, check=True)
        meta = json.loads(subprocess.check_output(['cargo', 'metadata', '--locked', '--no-deps',
                                                   '--format-version', '1'], cwd=ROOT))
        binary = Path(meta['target_directory']) / 'debug/convex-server'
    binary = binary.resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix='convex-core-workflow-') as folder:
        directory = Path(folder)
        with ServerProcess(binary, directory) as server:
            summary, pnl, original = calculate(server.call)
        with ServerProcess(binary, directory) as restarted:
            replay = restarted.call(pnl)
        require(replay == original, 'Restart changed the same-snapshot native result')
    if args.pnl_output:
        args.pnl_output.write_text(json.dumps(original, separators=(',', ':')) + '\n')
    summary['restart_replay_identical'] = True
    summary['server_stopped'] = True
    summary['meaning'] = 'Supplied synthetic pricing/risk/hedge proposal and model P&L; no live feeds, orders or cash postings'
    print(json.dumps(summary, indent=2))


if __name__ == '__main__':
    try:
        main()
    except (RuntimeError, OSError, ValueError, subprocess.SubprocessError, StopIteration) as error:
        print(str(error), file=sys.stderr)
        raise SystemExit(1)
