"""Retain exact original financial test bytes; verify the single ownership migration."""
import hashlib
import json
from pathlib import Path
ROOT = Path(__file__).resolve().parents[2]
original = (ROOT/'tests/fixtures/library/icma-helpers-original-cash-calibration.rs').read_bytes()
expected = 'f4032bcb150a58d7648d991e118a8690d0d7764ec4b483724477ddce8496e1aa'
if hashlib.sha256(original).hexdigest() != expected:
    raise ValueError('Original full cash-calibration financial test changed')
actual = (ROOT/'crates/convex-analytics/tests/cash_calibration.rs').read_bytes()
if original.count(b'h.quote_kind = kind;\n        h.quote = if matches!') != 1 or actual != original.replace(b'h.quote_kind = kind;\n        h.quote = if matches!', b'h.quote_kind = kind.clone();\n        h.quote = if matches!'):
    raise ValueError('Ownership migration changed original inputs, numerics or assertions')
print(json.dumps(dict(status='passed', original_sha256=expected, original_controls_unchanged=True,
                     sole_change='Explicit Clone of existing quote tag before later reuse', certified=False)))
