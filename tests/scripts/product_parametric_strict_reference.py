"""Independent full-product controls with a stricter caller objective.

Earlier declarations and candidate failures are retained. The original dated
quotes, model answers, parameter/discount accuracy bounds, starts, parameter
scales, native residual tolerances and work limits remain unchanged. A quote
objective of 1e-11 did not imply the separately declared decay accuracy; this
input requests 1e-13 explicitly, without changing the solver's stopping policy.
"""
import argparse
import json
import product_parametric_scaled_reference as scaled

DEST = scaled.admitted.original.ROOT / 'tests/fixtures/curves/product-parametric-strict.json'


def build():
    fixture = scaled.build()
    for case in fixture['cases']:
        graph = case['inputs']['calibration']
        recipe = (graph['rates'] or graph['cash'])[0]['recipe']
        recipe['curve']['calibration']['Parametric']['objective_tolerance'] = 1e-13
    fixture['objective_policy'] = 'Explicit stricter caller objective; original financial answers and all acceptance bounds unchanged. Earlier looser objective failures remain recorded.'
    return fixture


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    content = json.dumps(build(), indent=2) + '\n'
    if args.check:
        if DEST.read_text() != content:
            raise SystemExit('Frozen strict-objective control bytes differ')
        print('12 full product graphs /60 independent Decimal controls; stricter caller objective passed')
    else:
        if DEST.exists():
            raise SystemExit('Refusing to overwrite independent strict-objective declaration')
        DEST.write_text(content)
