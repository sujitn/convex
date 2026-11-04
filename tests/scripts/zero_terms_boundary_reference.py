#!/usr/bin/env python3
"""Supplemental exact-ingress domains, declared after repair and before their comparison."""
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[2]
TARGET = ROOT / "tests/fixtures/library/zero-terms-boundaries.json"


def document():
    invalid = ["1e-3", " 100", "100 ", "+100", "1,000",
               "0.00000000000000000000000000001", "79228162514264337593543950336"]
    cases = [dict(field=field, value=value) for field in ["face_value", "redemption_value", "issue_price"]
             for value in invalid + [97.125]]
    return dict(schema_version=1, provenance="Supplementary structural domains declared after using existing ExactDecimal and before the first boundary comparison; no financial oracle or pre-edit claim.",
                rejected_cases=cases, certified=False)


value = document()
data = (json.dumps(value, indent=2) + "\n").encode()
mode = sys.argv[1] if len(sys.argv) == 2 else "check"
if mode == "write":
    with TARGET.open("xb") as stream:
        stream.write(data)
elif mode == "check":
    if TARGET.read_bytes() != data:
        raise ValueError("Supplementary structural declaration changed")
else:
    raise ValueError("Use write or check")
print(json.dumps(dict(status="passed", rejected_cases=len(value["rejected_cases"]), certified=False)))
