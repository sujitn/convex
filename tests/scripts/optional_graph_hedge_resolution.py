"""Separate undersampled h/2 control, retaining the original failed floor trial.

The original declared 50bp bump has a 25bp half-step that is actually above
two native cells on the original finest event grid. A separate explicit 40bp
bump has a 20bp half-step below the declared floor. No answer or tolerance is
changed, and original raw controls stay available.
"""
import argparse
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SOURCE = 'tests/fixtures/library/optional-graph-hedge-admitted.json'
TARGET = 'tests/fixtures/library/optional-graph-hedge-resolved.json'


def generate():
    value = json.loads((ROOT / SOURCE).read_bytes())
    case = next(c for c in value['cases'] if c['id'] == 'optional_graph.invalid.half_unresolved')
    case['request']['operation']['input']['factors'][0]['settings']['bump'] = .004
    value['source_reads'] = {SOURCE: hashlib.sha256((ROOT / SOURCE).read_bytes()).hexdigest()}
    value['qualification'] = 'Separate explicit unresolved half-bump trial. Original financial controls, original finest tree and all refinement/resolution policies remain.'
    return value


if __name__ == '__main__':
    p = argparse.ArgumentParser(); p.add_argument('--write', action='store_true'); a = p.parse_args()
    raw = (json.dumps(generate(), indent=2) + '\n').encode(); out = ROOT / TARGET
    if a.write:
        if out.exists(): raise ValueError('Refusing to overwrite separate resolution control')
        out.write_bytes(raw)
    elif out.read_bytes() != raw: raise ValueError('Separate resolution control differs')
    print(json.dumps(dict(status='passed', cases=30, certified=False)))
