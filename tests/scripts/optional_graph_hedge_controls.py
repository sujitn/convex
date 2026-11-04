"""Check committed optional graph hedge controls against their unchanged generators.

The generators' closed forms call the platform libm (expm1, erfc, exp), so their
floats differ across platforms in the last digits and their own byte-exact check
passes only on the platform that wrote the controls. This regenerates the same
controls and requires structure, text (including source reads) and integers to
match exactly. Each float must agree within 1e-11 absolute, or 1e-13 relative
(about 450 ulp) above magnitude 100. The largest drift seen on Linux and Windows
is under 3e-12, and the tightest consumer tolerance (cash) is 1e-10.
"""
import argparse
import json
import math
from pathlib import Path

import optional_graph_hedge_calibrated as calibrated
import optional_graph_hedge_qualification as qualification
import optional_graph_hedge_reference as reference

FOLDER = Path(__file__).resolve().parents[2] / 'tests/fixtures/library'
CONTROLS = {
    'reference': (reference.generate, ['optional-graph-hedge-api.json', 'optional-graph-hedge-reference.json',
                                       'optional-graph-hedge-book.json']),
    'qualification': (qualification.generate, ['optional-graph-hedge-admitted.json',
                                               'optional-graph-hedge-qualification.json']),
    'calibrated': (lambda: [calibrated.generate()], ['optional-graph-hedge-calibrated.json']),
}
ABS, REL = 1e-11, 1e-13


def compare(old, new, path, drift):
    if isinstance(old, float):
        if not isinstance(new, float) or not math.isclose(old, new, rel_tol=REL, abs_tol=ABS):
            raise ValueError(f'{path}: committed {old!r}, regenerated {new!r}')
        drift['floats_differing'] += old != new
        drift['max_abs'] = max(drift['max_abs'], abs(old - new))
    elif isinstance(old, dict):
        if not isinstance(new, dict) or old.keys() != new.keys(): raise ValueError(f'{path}: keys differ')
        for key in old: compare(old[key], new[key], f'{path}/{key}', drift)
    elif isinstance(old, list):
        if not isinstance(new, list) or len(old) != len(new): raise ValueError(f'{path}: lengths differ')
        for i, (x, y) in enumerate(zip(old, new)): compare(x, y, f'{path}[{i}]', drift)
    elif type(old) is not type(new) or old != new:
        raise ValueError(f'{path}: committed {old!r}, regenerated {new!r}')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(); parser.add_argument('controls', choices=sorted(CONTROLS)); args = parser.parse_args()
    generate, names = CONTROLS[args.controls]
    for name, value in zip(names, generate(), strict=True):
        drift = dict(floats_differing=0, max_abs=0.)
        compare(json.loads((FOLDER / name).read_bytes()), json.loads(json.dumps(value, default=str)), name, drift)
        print(json.dumps(dict(status='passed', path=name, cases=len(value.get('cases', [])), certified=False, **drift)))
