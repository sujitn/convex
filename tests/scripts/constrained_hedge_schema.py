"""Check actual supplied complete-factor hedge schema; native finance stays separate."""
import argparse
from copy import deepcopy
import hashlib
import json
from pathlib import Path
import subprocess
from jsonschema import Draft202012Validator
from curve_replay_schema import formats


def validate(binary):
    raw = subprocess.check_output([binary, '--emit-schema'])
    schema = json.loads(raw); Draft202012Validator.check_schema(schema)
    validator = Draft202012Validator(schema, format_checker=formats())
    folder = Path(__file__).resolve().parents[1]/'fixtures/library'
    cases = json.loads((folder/'constrained-hedge-admitted.json').read_bytes())['cases']
    cases += json.loads((folder/'constrained-hedge-lots.json').read_bytes())['cases']
    positives = [c for c in cases if c['expected_error'] is None]
    for case in positives:
        validator.validate(case['request'])
    original=positives[0]['request'];negative=[]
    for field in ['snapshot_id','entity','settlement','reporting_currency','position_state','expected_holdings','holdings','factors','candidates','max_cost','settings']:
        candidate=deepcopy(original);del candidate['operation']['problem'][field];negative.append(candidate)
    for field in ['reference_position','sensitivities','instrument','economics','calculation','credit_scope','fx']:
        candidate=deepcopy(original);del candidate['operation']['problem']['candidates'][0]['risk'][field];negative.append(candidate)
    for field in ['trade_lower','trade_upper','holding_lower','holding_upper','buy_available','sell_available','lot','costs','eligibility']:
        candidate=deepcopy(original);del candidate['operation']['problem']['candidates'][0][field];negative.append(candidate)
    for field in ['max_work','rank_relative_tolerance','max_condition','solver_tolerance','factor_tolerance','cost_tolerance','cost_scale','cost_weight','svd_iterations','policy_version']:
        candidate=deepcopy(original);del candidate['operation']['problem']['settings'][field];negative.append(candidate)
    for owner,key,value in [(['candidates',0],'buy_available',1),(['candidates',0,'risk'],'dv01',5),
                            (['settings'],'optimizer','guess'),(['candidates',0,'risk','reference_position'],'quantity',1000),
                            (['candidates',0,'costs'],'currency','BAD'),(['candidates',0,'risk'],'product','synthetic')]:
        candidate=deepcopy(original);node=candidate['operation']['problem']
        for part in owner:node=node[part]
        node[key]=value;negative.append(candidate)
    for candidate in negative:
        if validator.is_valid(candidate):
            raise ValueError('Schema admitted missing/lost/replaced complete-factor hedge input')
    return dict(status='passed', positive_cases=len(positives), rejected_mutations=len(negative),
                schema_sha256=hashlib.sha256(raw).hexdigest(), certified=False)


if __name__ == '__main__':
    parser=argparse.ArgumentParser(); parser.add_argument('--binary',required=True)
    print(json.dumps(validate(parser.parse_args().binary)))
