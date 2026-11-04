"""Validate complete UFR product inputs and explicit join sides against the
actual exported schema. Financial admission remains the native owner's job.
"""
import argparse
from copy import deepcopy
import json
from pathlib import Path
import subprocess
from jsonschema import Draft202012Validator
from curve_replay_schema import formats

parser = argparse.ArgumentParser()
parser.add_argument('--binary',required=True)
args = parser.parse_args()
schema = json.loads(subprocess.check_output([args.binary,'--emit-schema'],text=True))
Draft202012Validator.check_schema(schema)
validator = Draft202012Validator(schema,format_checker=formats())
fixture = json.loads((Path(__file__).resolve().parents[1]/'fixtures/curves/product-ufr-admitted.json').read_bytes())
requests = []
for case in fixture['cases']:
    request = dict(schema_version=1,inputs=case['inputs'],operation=dict(kind='curves'))
    validator.validate(request)
    requests.append(request)
original = requests[0]
negative = []
for field in ['base','last_liquid_point','initial','lower','upper','parameter_scales',
              'quote_scales','weights','objective_tolerance','max_iterations']:
    bad = deepcopy(original)
    del bad['inputs']['calibration']['rates'][0]['recipe']['curve']['calibration']['Ufr'][field]
    negative.append(bad)
for value in [[1], [1,1,1]]:
    bad = deepcopy(original)
    bad['inputs']['calibration']['rates'][0]['recipe']['curve']['calibration']['Ufr']['parameter_scales'] = value
    negative.append(bad)
bad = deepcopy(original)
bad['inputs']['calibration']['rates'][0]['recipe']['curve']['calibration']['Ufr']['sampled_nodes'] = []
negative.append(bad)
for side in ['Left','Right']:
    joined = deepcopy(original)
    joined['operation'] = dict(kind='curve_query',curve=fixture['cases'][0]['inputs']['calibration']['rates'][0]['recipe']['curve']['key'],
                              query=dict(kind='UfrJoinedForward',point=dict(Date='2031-10-07'),side=side))
    validator.validate(joined)
    requests.append(joined)
bad = deepcopy(requests[-1]);del bad['operation']['query']['side'];negative.append(bad)
bad = deepcopy(requests[-1]);bad['operation']['query']['side']='Average';negative.append(bad)
for bad in negative:
    if not list(validator.iter_errors(bad)):
        raise SystemExit('Incomplete/lossy UFR fit or implicit boundary-side schema accepted')
print(json.dumps(dict(positive=len(requests),negative=len(negative),certified=False)))
