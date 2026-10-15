"""Validate actual schemas for full tagged evidence and directional native columns."""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import subprocess
from jsonschema import Draft202012Validator, FormatChecker

ROOT = Path(__file__).resolve().parents[2]


def validate(binary):
    raw = subprocess.check_output([binary, '--emit-schema'])
    schema = json.loads(raw)
    Draft202012Validator.check_schema(schema)
    validator = Draft202012Validator(schema, format_checker=FormatChecker())
    cases = json.loads((ROOT/'tests/fixtures/library/directional-hedges.json').read_bytes())['cases']
    positives = [case for case in cases if case['expected_error'] is None]
    for case in positives:
        validator.validate(case['request'])
    mutations = [
        lambda s: s.pop('kind'),
        lambda s: s.update(kind='automatic'),
        lambda s: s['result'].pop('second_unit'),
        lambda s: s['result'].pop('native_prices'),
        lambda s: s['result']['native_prices'].pop(),
        lambda s: s['result']['settings']['native'].pop('side'),
        lambda s: s['result']['settings'].pop('second'),
        lambda s: s['result'].update(method='central'),
    ]
    for mutate in mutations:
        request = copy.deepcopy(positives[0]['request'])
        mutate(request['operation']['problem']['holdings'][0]['risk']['sensitivities'][0])
        if validator.is_valid(request):
            raise ValueError('Schema admitted lost/ignored original evidence or method/precision')
    return dict(status='passed', positive_requests=len(positives), rejected_mutations=len(mutations),
                schema_sha256=hashlib.sha256(raw).hexdigest(), certified=False)


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', required=True)
    print(json.dumps(validate(parser.parse_args().binary)))
