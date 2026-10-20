#!/usr/bin/env python3
"""Actual complete helper schema admission, independent of financial admission."""
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
    schema = json.loads(content)
    Draft202012Validator.check_schema(schema)
    validator = Draft202012Validator(schema, format_checker=FormatChecker())
    cases = json.loads((ROOT / "tests/fixtures/library/rate-helpers.json").read_bytes())["cases"]
    for case in cases:
        error = next(validator.iter_errors(case["request"]), None)
        if error is not None:
            raise ValueError(f"{case['id']}: schema rejected full helper at {list(error.path)}")
    examples = {c["id"]: c["request"] for c in cases}
    mutations = (
        ("irs", lambda r: r["operation"]["input"]["input"]["recipe"]["helpers"][0]["terms"]["fixed"]["rates"][0].update(value=0.03)),
        ("fra", lambda r: r["operation"]["input"]["input"]["recipe"]["helpers"][0]["terms"]["terms"].pop("fixing_date")),
        ("deposit", lambda r: r["operation"]["input"]["input"].pop("quote_versions")),
        ("ois", lambda r: r["operation"]["input"]["input"]["recipe"].update(cached_residual=0.0)),
        ("rate_future", lambda r: r["operation"]["input"].pop("context")),
        ("basis", lambda r: r["operation"]["input"]["input"]["recipe"]["helpers"][0]["terms"].pop("quoted_leg")),
    )
    for name, mutate in mutations:
        request = copy.deepcopy(examples[f"rate_helpers.{name}.positive"])
        mutate(request)
        if validator.is_valid(request):
            raise ValueError("Schema discarded required helper economics: " + name)
    return dict(status="passed", cases=len(cases), rejected_mutations=len(mutations),
                schema_sha256=hashlib.sha256(content).hexdigest(), certified=False)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True)
    print(json.dumps(validate(parser.parse_args().binary), indent=2))
