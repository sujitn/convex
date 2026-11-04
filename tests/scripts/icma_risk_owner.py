"""Separate the risk owner while conserving all predeclared financial controls."""
import argparse
from copy import deepcopy
import json
from pathlib import Path

DEST = Path(__file__).resolve().parents[1] / 'fixtures/library'


def corpus():
    value = deepcopy(json.loads((DEST / 'icma-risk-final-api.json').read_bytes()))
    for case in value['cases']:
        if case['request']['operation']['kind'] != 'icma_quotation':
            raise ValueError('Unexpected original quotation owner')
        case['request']['operation']['kind'] = 'icma_yield_risk'
    return dict(schema_version=1, **value)


if __name__ == '__main__':
    p = argparse.ArgumentParser(); p.add_argument('--write', action='store_true'); a = p.parse_args()
    value = corpus(); data = (json.dumps(value, indent=2) + '\n').encode(); path = DEST / 'icma-risk-owner-api.json'
    if a.write:
        if path.exists(): raise ValueError('Refusing to overwrite original owner framing')
        path.write_bytes(data)
    elif path.read_bytes() != data: raise ValueError('Risk owner framing changed original financial inputs')
    print(json.dumps(dict(status='passed', requests=len(value['cases']), financial_parameters_and_answers_unchanged=True, certified=False)))
