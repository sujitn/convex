#!/usr/bin/env python3
"""Independent Decimal80 claim PV/central-risk control; no native execution.

USD1,000 actual position cash,14/365 years,3% continuous collateral discount;
five original +/-1bp and half-step valuations. Absolute USD/risk bound1e-7.
This is a synthetic analytic control, not a market/model certification pack.
"""
import argparse
import copy
from decimal import Decimal, localcontext
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tools'))
import core_book_workflow as books
import core_lifecycle_workflow as example
import core_snapshot_workflow as core
import lifecycle_risk

PATH = ROOT / 'tests/fixtures/library/lifecycle-risk-contract.json'


def generate():
    capture, _, factors = core.prepare()
    config = dict(pnl='pnl', risks={identity: 'risk.' + str(i) for i, identity in enumerate([
        'mixed.bond', 'stepped-amortizing-IRS', 'seasoned-OIS', 'graph-hedge.CDS', 'mixed.overnight', 'mixed.term'])})
    # Resolve actual calculation names from the preserved full native requests.
    definition = books.definition(capture, factors, 0)
    config['risks'] = {c['request']['operation']['input']['identity']['id']: name
        for name, c in definition['calculations'].items() if name.startswith('risk.')}
    definition = example.append_event(definition, example.termination('0', '2026-10-21'), 'core.claim.control')
    definition = lifecycle_risk.bind_requests(definition, config, [example.TRADE])
    calc = config['risks'][example.TRADE]
    curves = copy.deepcopy(capture['curves'])
    with localcontext() as context:
        context.prec = 80
        df = (-Decimal('0.03') * 50).exp()
        for row in curves:
            if row['curve_id'] == 'D.OIS':
                row['recipe']['source']['Nodes'][-1]['quote']['value'] = float(df)
        time, bump, unit = Decimal(14) / Decimal(365), Decimal('0.0001'), Decimal('0.0001')
        pv = lambda move: Decimal(1000) * (-(Decimal('0.03') + move) * time).exp()
        samples = [pv(d) for d in [Decimal(0), bump, -bump, bump / 2, -bump / 2]]
        expected = dict(zip(['base_pv', 'plus_pv', 'minus_pv', 'half_plus_pv', 'half_minus_pv'], map(str, samples)))
        expected['value'] = str((samples[1] - samples[2]) / (2 * bump) * unit)
        expected['half_value'] = str((samples[3] - samples[4]) / bump * unit)
    saved = dict(id='core.claim.control.native', content=dict(definition=dict(definition=dict(definition=definition)),
        curves={'market.' + str(i): [r for r in curves if r['version'] == i + 1] for i in range(2)}, fixings=capture['fixings']))
    return dict(schema_version=1, provenance='Synthetic independent Decimal80 cash discount/central-difference control; native owners supply all product/P&L/hedge calculations',
        units=dict(cash='USD for the complete actual holding, not per reference notional', rate='annual continuous decimal',
            discount_time='Actual365Fixed14days', normalized_risk='USD per1bp', state='one complete state copy'),
        reference=dict(cash='1000', continuous_rate='0.03', days=14, denominator=365, bump='0.0001',
            absolute_bound='0.0000001', relative_bound='0', expected_increment=expected),
        request=example.native_request(saved, calc))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('mode', choices=['write', 'check'])
    args = parser.parse_args()
    data = (json.dumps(generate(), sort_keys=True, indent=2, allow_nan=False) + '\n').encode()
    if args.mode == 'write':
        if PATH.exists():
            raise RuntimeError('Reference already exists; never silently refresh it')
        PATH.write_bytes(data)
    elif PATH.read_bytes() != data:
        raise RuntimeError('Independent reference bytes changed')
    print(json.dumps(dict(status='passed', mode=args.mode, reference=str(PATH.relative_to(ROOT)))))


if __name__ == '__main__':
    main()
