"""Explicit original FlatForward origin admission, before candidate execution.

The original control's supplied time-zero quote differs from its first positive
interval zero. Native MethodGrid requires equality at origin. Only that unused
zero-time raw label changes; original product quotes and all financial answers
and tolerances remain byte-equivalent after parsing. Preserve the first control.
"""
import argparse
import json
from pathlib import Path
import product_ufr_reference as original

DEST = Path(__file__).resolve().parents[2] / 'tests/fixtures/curves/product-ufr-admitted.json'

def build():
    value = original.build()
    for case in value['cases']:
        base = case['inputs']['curves'][0]
        if 'FlatForward' in base['source']:
            nodes = base['source']['FlatForward']['nodes']
            nodes[0]['quote'] = nodes[1]['quote'].copy()
    return value

if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    content = json.dumps(build(), indent=2)+'\n'
    if args.check:
        if DEST.read_text() != content:
            raise SystemExit('Frozen UFR origin admission bytes differ')
        print('Original origin admission;36 complete quote/126 analytic expectations and bounds unchanged')
    else:
        if DEST.exists():
            raise SystemExit('Refusing to overwrite independent admitted controls')
        DEST.write_text(content)
