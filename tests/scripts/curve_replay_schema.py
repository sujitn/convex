#!/usr/bin/env python3
"""Validate the actual operator envelope and original native recipe schemas."""
import argparse
import copy
import datetime
import hashlib
import json
from pathlib import Path
import re
import subprocess

from jsonschema import Draft202012Validator, FormatChecker


def formats():
    checker = FormatChecker()
    # jsonschema's RFC3339 checker is optional. Exercise actual timestamp
    # validation even when that extra package is absent from the pinned runtime.
    @checker.checks('date-time')
    def timestamp(value):
        if not isinstance(value, str):
            return True
        if re.fullmatch(r'\d{4}-\d{2}-\d{2}[Tt]\d{2}:\d{2}:\d{2}(\.\d+)?([Zz]|[+-]\d{2}:\d{2})', value) is None:
            return False
        try:
            return datetime.datetime.fromisoformat(value.upper()).utcoffset() is not None
        except ValueError:
            return False
    return checker


def emit(binary, option):
    return subprocess.check_output([str(Path(binary).resolve()), option])


def validate(binary, native_binary):
    raw = emit(binary, '--emit-schema')
    schema = json.loads(raw)
    Draft202012Validator.check_schema(schema)
    validator = Draft202012Validator(schema, format_checker=formats())
    document = json.loads(emit(binary, '--emit-configuration'))
    validator.validate(document)
    original = json.loads(emit(binary, '--schema-one'))['content']
    legacy = copy.deepcopy(document)
    legacy['assembly'].pop('curve_recipes')
    legacy['assembly']['curves'] = original['curves']
    validator.validate(legacy)
    mutations = []
    for field in schema['$defs']['curve_recipe']['required']:
        changed = copy.deepcopy(document)
        changed['assembly']['curve_recipes'][0].pop(field)
        mutations.append(changed)
    for field, value in [('schema_version', 2), ('version', 0),
                         ('source_time', 'not-a-timestamp'), ('quality', 'Certified'),
                         ('max_age_ms', 86400001), ('unknown_capture', True)]:
        changed = copy.deepcopy(document)
        changed['assembly']['curve_recipes'][0][field] = value
        mutations.append(changed)
    for field in ['id', 'version']:
        changed = copy.deepcopy(document)
        changed['assembly']['curve_recipes'][0]['recipe']['key'].pop(field)
        mutations.append(changed)
    changed = copy.deepcopy(document)
    changed['assembly']['curve_recipes'] *= 65
    mutations.append(changed)
    for i, changed in enumerate(mutations):
        if validator.is_valid(changed):
            raise ValueError(f'Operator schema admitted mutation {i}')

    # Financial schemas remain owned by the pure library. Ports intentionally
    # allow the complete opaque payload; the strict native engine owns admission.
    native_raw = emit(native_binary, '--emit-schema')
    native_schema = json.loads(native_raw)
    native = {'$schema': native_schema['$schema'],
              '$defs': native_schema['$defs'], '$ref': '#/$defs/CurveRecipe'}
    Draft202012Validator.check_schema(native)
    native_validator = Draft202012Validator(native, format_checker=formats())
    recipe = document['assembly']['curve_recipes'][0]['recipe']
    native_validator.validate(recipe)
    native_mutations = []
    for field in native_schema['$defs']['CurveRecipe']['required']:
        changed = copy.deepcopy(recipe)
        changed.pop(field)
        native_mutations.append(changed)
    changed = copy.deepcopy(recipe)
    changed['source']['FlatForward']['sampled_grid'] = []
    native_mutations.append(changed)
    changed = copy.deepcopy(recipe)
    changed['guessed_version'] = 7
    native_mutations.append(changed)
    for i, changed in enumerate(native_mutations):
        if native_validator.is_valid(changed):
            raise ValueError(f'Native schema admitted mutation {i}')
    return dict(status='passed', configuration_documents=2, native_recipes=1,
                rejected_envelope_mutations=len(mutations),
                rejected_native_mutations=len(native_mutations),
                operator_schema_sha256=hashlib.sha256(raw).hexdigest(),
                native_schema_sha256=hashlib.sha256(native_raw).hexdigest(),
                certified=False)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True)
    parser.add_argument('--native-binary', required=True)
    args = parser.parse_args()
    print(json.dumps(validate(args.binary, args.native_binary), indent=2))
