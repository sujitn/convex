#!/usr/bin/env python3
"""Validate actual generated schemas; schema acceptance is not financial admission."""
import argparse
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
    cases = json.loads((ROOT / "tests/fixtures/library/cases.json").read_bytes())["cases"]
    for case in cases:
        error = next(validator.iter_errors(case["request"]), None)
        if error is not None:
            raise ValueError(f"{case['id']}: schema rejected actual typed request at {list(error.path)}")
    original = next(c["request"] for c in cases if c["id"] == "bond.construct")
    # Unknown economic fields and numeric cash must not pass the published schema.
    for name, mutation in (
        ("unknown root", lambda r: r.update(invented=True)),
        ("numeric face", lambda r: r["operation"]["bond"]["terms"].update(face_value=1.25)),
        ("missing inputs", lambda r: r.pop("inputs")),
        ("unknown action", lambda r: r["operation"]["action"].update(kind="invented")),
    ):
        request = json.loads(json.dumps(original))
        mutation(request)
        if validator.is_valid(request):
            raise ValueError(f"schema admitted {name}")
    return {"status": "passed", "cases": len(cases), "rejected_mutations": 4,
            "schema_sha256": hashlib.sha256(content).hexdigest(), "certified": False,
            "qualification": "The native operation still validates financial terms, units, versions and data admission."}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True)
    args = parser.parse_args()
    print(json.dumps(validate(args.binary), indent=2))


if __name__ == "__main__":
    main()
