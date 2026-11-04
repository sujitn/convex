"""Actual generated native-derivative request schemas with complete economics."""
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
    cases = json.loads((ROOT/'tests/fixtures/library/coupon-parameter-derivatives.json').read_bytes())['cases']
    positives = [case for case in cases if case['expected_error'] is None]
    for case in positives:
        validator.validate(case['request'])
    mutations = [
        lambda r: r['operation']['settings'].pop('absolute_second_tolerance'),
        lambda r: r['operation']['settings'].update(currency_per_bp=True),
        lambda r: r['operation']['coordinate'].pop('source_version'),
        lambda r: r['operation']['coordinate'].update(cached_value=0.1),
        lambda r: r['operation']['input']['bond']['terms']['profiles']['notional'][0].update(value=5000.01),
        lambda r: r['operation'].update(first_derivative=0),
    ]
    for mutate in mutations:
        request = copy.deepcopy(positives[0]['request'])
        mutate(request)
        if validator.is_valid(request):
            raise ValueError('Derivative schema admitted missing policy or lost/ignored economics')
    return dict(status='passed', positive_requests=len(positives), rejected_mutations=len(mutations),
                schema_sha256=hashlib.sha256(raw).hexdigest(), certified=False)


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', required=True)
    print(json.dumps(validate(parser.parse_args().binary)))
