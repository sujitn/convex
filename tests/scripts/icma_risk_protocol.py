"""Keep raw floating framing failure; separately admit its full original reset terms."""
import argparse
from copy import deepcopy as clone
import json
from pathlib import Path

DEST = Path(__file__).resolve().parents[1] / 'fixtures/library'


def qualify():
    raw = json.loads((DEST / 'icma-risk-api.json').read_bytes())
    cases = clone(raw['cases'])
    original = next(c for c in cases if c['id'] == 'icma_risk.invalid.full_term_floating')
    original['expected_error'] = 'invalid_input'
    admitted = clone(original); admitted['id'] += '.admitted'; admitted['expected_error'] = 'unsupported'
    terms = admitted['request']['operation']['input']['security']['bond']['terms']
    terms['coupon'].pop('reset_frequency')
    terms['coupon']['resets'] = clone(terms['schedule']); terms['coupon']['resets']['frequency'] = 'Quarterly'
    cases.append(admitted)
    return dict(certified=False, qualification='Original raw requests/answers/bounds and actual malformed reset-frequency failure remain. '
                'A separate full original ScheduleSpec reset input reaches the existing floating fixed-cash model guard; no financial control changes.', cases=cases)


if __name__ == '__main__':
    p = argparse.ArgumentParser(); p.add_argument('--write', action='store_true'); a = p.parse_args()
    v = qualify(); data = (json.dumps(v, indent=2) + '\n').encode(); path = DEST / 'icma-risk-final-api.json'
    if a.write:
        if path.exists(): raise ValueError('Refusing to overwrite original protocol admission')
        path.write_bytes(data)
    elif path.read_bytes() != data: raise ValueError('Original protocol admission changed')
    print(json.dumps(dict(status='passed', requests=len(v['cases']), certified=False)))
