"""Actual strict schema for full member construction, scenarios, risk and hedge books."""
import argparse
from copy import deepcopy
import hashlib
import json
import subprocess
from jsonschema import Draft202012Validator
from curve_replay_schema import formats


def validate(binary):
    raw = subprocess.check_output([binary, '--emit-schema']); schema = json.loads(raw)
    Draft202012Validator.check_schema(schema)
    validator = Draft202012Validator(schema, format_checker=formats())
    corpus = json.loads(subprocess.check_output([binary, '--emit-parity']))
    positives = [c for c in corpus['cases'] if c['expected_error'] is None]
    for case in positives: validator.validate(case['request'])
    negatives = []

    def reject(original, path, keys):
        def node(request):
            for key in path: request = request[key]
            return request
        for key in keys:
            request = deepcopy(original); del node(request)[key]; negatives.append(request)
        request = deepcopy(original); node(request)['cached_risk'] = 0; negatives.append(request)

    risk = next(c['request'] for c in positives
                if c['request']['operation'].get('action', {}).get('kind') == 'product_risk')
    action = ['operation', 'action']; product = action + ['product']; supplied = product + ['input']
    reject(risk, action, ['product', 'key', 'position', 'snapshot_id', 'settings'])
    reject(risk, product, ['input', 'discount', 'policy'])
    reject(risk, product + ['policy'], ['max_member_evaluations'])
    reject(risk, supplied, ['contract', 'basket', 'calendar_stamp', 'settlement', 'delivery',
                           'market', 'members', 'selection', 'context'])
    reject(risk, supplied + ['members', 0], ['instrument', 'stamp', 'bond', 'clean_price_per100',
                                          'quote_stamp', 'financing', 'coupon_carry'])
    reject(risk, product + ['discount'], ['curve', 'stamp'])
    scenario = next(c['request'] for c in positives
                    if c['request']['operation'].get('action', {}).get('kind') == 'product_scenario')
    reject(scenario, action, ['product', 'key', 'position', 'snapshot_id', 'delta'])
    column = next(c['request'] for c in positives if c['id'] == 'delivery.book.future')
    reject(column, ['operation'], ['graph', 'product', 'input'])
    reject(column, ['operation', 'input'], ['identity', 'reference_position', 'snapshot_id',
                                          'factors', 'max_factor_evaluations'])
    for i in range(5):
        reject(column, ['operation', 'input', 'factors', i], ['key', 'settings'])
    for request in negatives:
        if validator.is_valid(request):
            raise ValueError('Schema admitted incomplete or ignored full basket input')
    return dict(status='passed', positive_cases=len(positives), rejected_mutations=len(negatives),
                schema_sha256=hashlib.sha256(raw).hexdigest(), certified=False)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(); parser.add_argument('--binary', required=True)
    print(json.dumps(validate(parser.parse_args().binary)))
