"""Preserve exact original requests, schema-2 identity and Decimal controls."""
import argparse
import datetime
from decimal import Decimal, localcontext
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def check():
    rows = []
    for name in ['cases', 'operations']:
        source = json.loads((ROOT / f'tests/fixtures/library/{name}.json').read_bytes())
        rows.extend({'pack': name, **row} for row in source['cases']
                    if row['request']['operation']['kind'] == 'calibration')
    actual = json.loads((ROOT / 'tests/fixtures/library/calibration-replay.json').read_bytes())
    assert actual['cases'] == rows and actual['certified'] is False
    assert (len(rows), sum(row['expected_error'] is None for row in rows)) == (32, 18)
    original = json.loads((ROOT / 'tests/fixtures/library/curve-replay-schema-two.json').read_bytes())
    assert original['snapshot_id'] == 'sha256:03d3416a919c6acb045341e4ecdf77d6cdaeb77dc4659914f44c0072f94cace0'
    reference = json.loads((ROOT / 'tests/fixtures/library/calibration-replay-reference.json').read_bytes())
    assert reference['absolute_tolerance'] == 1e-12 and reference['certified'] is False
    with localcontext() as context:
        context.prec = 60
        for row in reference['cases']:
            days = (datetime.date.fromisoformat(row['date']) - datetime.date(2026, 10, 2)).days
            fraction = Decimal(days) / 360
            discount = 1 / (1 + Decimal(row['simple_rate']) * fraction)
            assert row['days'] == days and row['act360_fraction'] == str(fraction)
            assert row['discount_factor'] == str(discount)
            assert row['continuous_zero'] == str(-discount.ln() / fraction)
    return {'cases': 32, 'successes': 18, 'original_failures': 14,
            'independent_deposit_controls': len(reference['cases']), 'certified': False}


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--check', required=True, action='store_true')
    parser.parse_args()
    print(json.dumps(check(), sort_keys=True))
