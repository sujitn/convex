"""Validate original complete products against the actual exported native schema."""
import argparse
from copy import deepcopy
import json
from pathlib import Path
import subprocess
from jsonschema import Draft202012Validator
from curve_replay_schema import formats

parser=argparse.ArgumentParser();parser.add_argument('--binary',required=True);args=parser.parse_args()
schema=json.loads(subprocess.check_output([args.binary,'--emit-schema'],text=True))
Draft202012Validator.check_schema(schema)
validator=Draft202012Validator(schema,format_checker=formats())
fixture=json.loads((Path(__file__).resolve().parents[1]/'fixtures/curves/product-parametric-conditioned.json').read_bytes())
requests=[]
for case in fixture['cases']:
    request=dict(schema_version=1,inputs=case['inputs'],operation=dict(kind='curves'))
    validator.validate(request);requests.append(request)
original=requests[0];negative=[]
for field in ['initial','lower','upper','parameter_scales','weights','quote_scales','objective_tolerance','max_iterations']:
    bad=deepcopy(original);del bad['inputs']['calibration']['rates'][0]['recipe']['curve']['calibration']['Parametric'][field];negative.append(bad)
bad=deepcopy(original);bad['inputs']['calibration']['rates'][0]['recipe']['curve']['calibration']['Parametric']['sampled_nodes']=[];negative.append(bad)
for bad in negative:
    if not list(validator.iter_errors(bad)):raise SystemExit('Incomplete/lossy parametric fit schema accepted')
print(json.dumps(dict(positive=len(requests),negative=len(negative),certificate=False)))
