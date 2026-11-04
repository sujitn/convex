"""Separate complete calibrated control including the actual inherited OAS.

The first calibration control retained the supplied 0.00123456 OAS in its
request but omitted it in the new independent financial calculation. Keep
that raw trial immutable; this separately declared control includes the SAME
full original OAS input in every analytic PV and parameter/rate move. Its
cash, model quotes/settings, sigma source and comparison bounds are unchanged.
"""
import argparse
from copy import deepcopy
import hashlib
import json
from pathlib import Path

from optional_graph_hedge_reference import financial_price

ROOT = Path(__file__).resolve().parents[2]
SOURCES = ['tests/fixtures/library/optional-graph-hedge-qualification.json',
           'crates/convex-analytics/tests/fixtures/optional-bonds.json']
TARGET = 'tests/fixtures/library/optional-graph-hedge-calibrated.json'


def generate():
    case = deepcopy(json.loads((ROOT / SOURCES[0]).read_bytes())['cases'][-1])
    case['id'] = 'optional_graph.calibrated.full_oas'
    case['request']['operation']['input']['identity']['id'] = case['id']
    req = case['request']; op = req['operation']; oas = op['product']['oas_decimal']
    sigma = json.loads((ROOT / SOURCES[1]).read_bytes())['oracle']['parameters']['volatility']
    factors = []
    for kind, f in zip(['discount', 'oas', 'a', 'sigma'], op['input']['factors']):
        h = f['settings']['bump']; values = []
        for move in [0., h, -h, h / 2., -h / 2.]:
            values.append(financial_price(op['product']['input'], req['inputs'],
                discount=.04 + (move if kind == 'discount' else 0.),
                oas=oas + (move if kind == 'oas' else 0.),
                a=.03 + (move if kind == 'a' else 0.),
                sigma=sigma + (move if kind == 'sigma' else 0.)))
        prices = [v['pv'] for v in values]; unit = 1. if kind in ['a', 'sigma'] else .0001
        factors.append(dict(kind=kind, prices=prices, valuations=values,
            value=(prices[1] - prices[2]) / (2. * h) * unit,
            half_value=(prices[3] - prices[4]) / h * unit))
    case['financial'] = dict(factors=factors)
    return dict(schema_version=1, certified=False,
        source_reads={p: hashlib.sha256((ROOT / p).read_bytes()).hexdigest() for p in SOURCES},
        qualification='Complete original OAS explicitly included in independently computed calibrated model controls. Original failed OAS omission, original answers and all bounds retained.', cases=[case])


if __name__ == '__main__':
    p = argparse.ArgumentParser(); p.add_argument('--write', action='store_true'); a = p.parse_args()
    raw = (json.dumps(generate(), indent=2) + '\n').encode(); out = ROOT / TARGET
    if a.write:
        if out.exists(): raise ValueError('Refusing to overwrite separate full OAS controls')
        out.write_bytes(raw)
    elif out.read_bytes() != raw: raise ValueError('Separate full OAS control differs')
    print(json.dumps(dict(status='passed', cases=1, certified=False)))
