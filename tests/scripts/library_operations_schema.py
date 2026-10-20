#!/usr/bin/env python3
"""Validate supplemental requests against the actual generated native schema."""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import subprocess
from jsonschema import Draft202012Validator, FormatChecker

ROOT = Path(__file__).resolve().parents[2]


def validate(binary):
    content = subprocess.check_output([str(Path(binary).resolve()), "--emit-schema"])
    schema = json.loads(content); Draft202012Validator.check_schema(schema)
    validator = Draft202012Validator(schema, format_checker=FormatChecker())
    cases = json.loads((ROOT / "tests/fixtures/library/operations.json").read_bytes())["cases"]
    requests = {c["id"]: c["request"] for c in cases}
    for case in cases:
        error = next(validator.iter_errors(case["request"]), None)
        if error is not None:
            raise ValueError(f"{case['id']}: schema rejected typed operation at {list(error.path)}")
    mutations = (
        ("conversion.discount.Simple", lambda r: r["operation"]["period"].update(invented_basis="Act365")),
        ("conversion.discount.Simple", lambda r: r["operation"]["action"].update(ignored_spread=0.0)),
        ("time.inverse.Act365Fixed.Nearest", lambda r: r["operation"]["action"].pop("rounding")),
        ("risk.nodes.zero", lambda r: r["operation"]["action"]["input"]["position"].update(quantity=5000.0)),
        ("risk.nodes.zero", lambda r: r["operation"]["action"]["input"].update(cached_bucket_values=[1.0])),
    )
    for identity, mutate in mutations:
        candidate = copy.deepcopy(requests[identity]); mutate(candidate)
        if validator.is_valid(candidate): raise ValueError("Schema discarded an economic term: " + identity)
    return dict(status="passed", cases=len(cases), rejected_mutations=len(mutations),
                schema_sha256=hashlib.sha256(content).hexdigest(), certified=False)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__); parser.add_argument("--binary", required=True)
    print(json.dumps(validate(parser.parse_args().binary), indent=2))
