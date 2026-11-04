"""Explicit caller coordinates for the unchanged bounded native solver.

Scale all strict-input coordinates together by 100: the graph's finite
difference step is divided by 100, preserving the original native perturbation
and singular-value ratio. This changes the effect of the unchanged solver's
absolute damping floor, not financial observations, target answers, tolerances,
starts, native bounds, maximum work or its convergence/identification rules.
All earlier independent declarations and actual failures remain frozen.
"""
import argparse
import json
import product_parametric_strict_reference as strict

DEST = strict.scaled.admitted.original.ROOT / 'tests/fixtures/curves/product-parametric-conditioned.json'


def build():
    fixture = strict.build()
    for case in fixture['cases']:
        graph = case['inputs']['calibration']
        recipe = (graph['rates'] or graph['cash'])[0]['recipe']
        settings = recipe['curve']['calibration']['Parametric']
        settings['parameter_scales'] = [v * 100 for v in settings['parameter_scales']]
        graph['controls']['jacobian_step'] = 1e-8
    fixture['coordinate_policy'] = 'Explicit common coordinate scaling and inverse Jacobian step; original native perturbations and financial bounds/work retained. No solver defaults or automatic retries.'
    return fixture


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    content = json.dumps(build(), indent=2) + '\n'
    if args.check:
        if DEST.read_text() != content:
            raise SystemExit('Frozen conditioned control bytes differ')
        print('12 full product graphs /60 independent Decimal controls; explicit caller coordinates passed')
    else:
        if DEST.exists():
            raise SystemExit('Refusing to overwrite independent coordinate declaration')
        DEST.write_text(content)
