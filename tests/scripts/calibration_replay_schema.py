"""Check actual operator capture and unchanged pure financial schemas."""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import subprocess
from jsonschema import Draft202012Validator
from curve_replay_schema import formats


def check(binary, native_binary):
    emit = lambda option: subprocess.check_output([str(Path(binary).resolve()), option])
    raw = emit('--emit-schema')
    schema = json.loads(raw)
    Draft202012Validator.check_schema(schema)
    validator = Draft202012Validator(schema, format_checker=formats())
    document = json.loads(emit('--emit-configuration'))
    validator.validate(document)
    mutations = []
    for field in schema['$defs']['calibration']['required']:
        changed = copy.deepcopy(document)
        changed['assembly']['calibration'].pop(field)
        mutations.append(changed)
    for field, value in [('schema_version', 2), ('version', 0), ('curves', []),
                         ('quality', 'Certified'), ('recorded_at', 'not-a-time'),
                         ('max_age_ms', 86400001), ('unknown_capture', True)]:
        changed = copy.deepcopy(document)
        changed['assembly']['calibration'][field] = value
        mutations.append(changed)
    for field in ['curve_id', 'definition_version', 'version']:
        changed = copy.deepcopy(document)
        changed['assembly']['calibration']['curves'][0].pop(field)
        mutations.append(changed)
    for i, changed in enumerate(mutations):
        assert not validator.is_valid(changed), f'Operator schema admitted capture mutation {i}'
    native_raw = subprocess.check_output([str(Path(native_binary).resolve()), '--emit-schema'])
    native_schema = json.loads(native_raw)
    assert native_schema['type'] == 'object' and {'inputs', 'operation'} <= native_schema['properties'].keys()
    Draft202012Validator.check_schema(native_schema)
    native = Draft202012Validator(native_schema, format_checker=formats())
    inputs = document['assembly']['calibration']['inputs']
    request = {'schema_version': 1, 'inputs': inputs, 'operation': {'kind': 'curves'}}
    native.validate(request)
    malformed = []
    for field in ['quote_versions', 'futures_calendar_stamps', 'recipe']:
        changed = copy.deepcopy(request)
        changed['inputs']['calibration']['rates'][0].pop(field)
        malformed.append(changed)
    for field in ['calendar', 'terms', 'curve']:
        changed = copy.deepcopy(request)
        changed['inputs']['calibration']['rates'][0]['recipe']['helpers'][0]['terms'].pop(field)
        malformed.append(changed)
    changed = copy.deepcopy(request)
    changed['inputs']['calibration']['rates'][0]['recipe']['helpers'][0]['terms']['terms']['omitted_principal'] = '1'
    malformed.append(changed)
    for i, changed in enumerate(malformed):
        assert not native.is_valid(changed), f'Native schema admitted missing/lost product terms {i}'
    return {'status': 'passed', 'operator_schema_sha256': hashlib.sha256(raw).hexdigest(),
            'native_schema_sha256': hashlib.sha256(native_raw).hexdigest(),
            'capture_mutations_rejected': len(mutations), 'native_mutations_rejected': len(malformed),
            'certified': False}


if __name__ == '__main__':
    p = argparse.ArgumentParser()
    p.add_argument('--binary', required=True)
    p.add_argument('--native-binary', required=True)
    args = p.parse_args()
    print(json.dumps(check(args.binary, args.native_binary), sort_keys=True))
