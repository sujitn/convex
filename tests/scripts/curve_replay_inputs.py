"""Preserve original curve request cases and the before-edit schema-1 capture."""
import argparse
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PACKS = ['curve-families', 'curve-methods', 'ufr', 'monotone', 'operations']


def expected():
    rows = []
    for name in PACKS:
        pack = json.loads((ROOT / f'tests/fixtures/library/{name}.json').read_bytes())
        for case in pack['cases']:
            request = case['request']
            curves = request.get('inputs', {}).get('curves', [])
            if request.get('operation', {}).get('kind') in ['curve_query', 'curves'] and curves and len(curves) <= 64:
                rows.append({'pack': name, **case})
    return {'schema_version': 1, 'certified': False,
            'qualification': 'Exact replay transport controls copied unchanged from original native input packs; no candidate financial oracle. Existing frozen independent references remain unchanged.',
            'cases': rows}


def check():
    generated = json.dumps(expected(), sort_keys=True, separators=(',', ':')) + '\n'
    actual = (ROOT / 'tests/fixtures/library/curve-replay.json').read_text()
    if actual != generated:
        raise ValueError('Original input pack or exact replay corpus drift')
    capture = json.loads((ROOT / 'tests/fixtures/library/curve-replay-schema-one.json').read_bytes())
    if capture['snapshot_id'] != 'sha256:d8892dc4513afa7f7515e1b34b30b5fc95667293356df97ed215e56d2408cc7a':
        raise ValueError('Original before-edit schema-1 capture changed')
    rows = expected()['cases']
    counts = [len(rows), sum(r['expected_error'] is None for r in rows)]
    if counts != [2563, 2208]:
        raise ValueError('Original replay reachability scope changed')
    return {'cases': counts[0], 'successes': counts[1], 'failures': counts[0]-counts[1], 'certified': False}


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--check', action='store_true', required=True)
    parser.parse_args()
    print(json.dumps(check(), sort_keys=True))
