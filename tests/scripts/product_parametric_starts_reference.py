"""Separate explicit local starts; no native answers or tolerance changes.

The first Svensson bond starts exhausted the unchanged helper-valuation budget.
This fixture declares a second local solve from nonzero 1bp loading /1% decay
offsets, before executing it. There is no production retry or guessed start.
Both original failing starts and complete independent financial controls remain.
"""
import argparse
from copy import deepcopy
import hashlib
import json
import product_parametric_conditioned_reference as conditioned

ROOT = conditioned.strict.scaled.admitted.original.ROOT
DEST = ROOT / 'tests/fixtures/curves/product-parametric-starts.json'


def build():
    declarations = {}
    for case in conditioned.build()['cases']:
        if not case['id'].startswith('svensson.government.'):
            continue
        initial = deepcopy(case['model'])
        initial['beta0'] += .0001
        initial['beta1'] -= .0001
        initial['beta2'] += .0001
        initial['beta3'] -= .0001
        initial['tau1'] *= .99
        initial['tau2'] *= 1.01
        declarations[case['id']] = initial
    parent = ROOT / 'tests/fixtures/curves/product-parametric-conditioned.json'
    return dict(parent_sha256=hashlib.sha256(parent.read_bytes()).hexdigest(),
                qualification='Explicit second local starting points, not independent approval or global convergence assurance. Earlier budget failures retained; financial controls/tolerances unchanged.',
                initial_parameters=declarations)


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    content = json.dumps(build(), indent=2) + '\n'
    if args.check:
        if DEST.read_text() != content:
            raise SystemExit('Frozen starting-point declaration bytes differ')
        print('Two explicit nonzero local starts; original independent financial bounds unchanged')
    else:
        if DEST.exists():
            raise SystemExit('Refusing to overwrite starting-point declaration')
        DEST.write_text(content)
