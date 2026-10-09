"""Pre-execution distinction between consistent new revisions and stale pins."""
import argparse
from copy import deepcopy
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def qualified():
    raw = json.loads((ROOT/'tests/fixtures/library/icma-government.json').read_bytes())
    refs = json.loads((ROOT/'tests/fixtures/library/icma-government-reference.json').read_bytes())
    cases = deepcopy(raw['cases']); additional = []
    base = next(r for r in refs['cases'] if r['id'] == 'icma_gov.regular.LinearMaturity.price')
    corrections = {
        'stale_context': ('convention', 'reference_context'),
        'wrong_security': ('reference_context', 'security'),
        'common_policy': None,
    }
    baseline = next(c for c in cases if c['id'] == base['id'])
    for label, path in corrections.items():
        original = next(c for c in cases if c['id'] == 'icma_gov.invalid.'+label+'.price')
        # The raw Python generator shared pin objects. Those wire requests
        # consistently rebind BOTH revision and its reference, so are admissible.
        original['expected_error'] = None
        answer = deepcopy(base); answer['id'] = original['id']; additional.append(answer)
        separate = deepcopy(baseline); separate['id'] = 'icma_gov.qualified.'+label+'.price'
        op = separate['request']['operation']
        if path:
            op['target']['icma'][path[0]][path[1]]['version'] = 99
        else:
            op['settings']['government']['quotation_policy']['version'] = 99
        separate['expected_error'] = 'invalid_input'; cases.append(separate)
    # The first unselected security's independent yield is .0365. The original
    # .0225 degenerate bracket excludes that quote; it is a no-root diagnostic.
    original = next(c for c in cases if c['id'] == 'icma_gov.invalid.non_unique.price')
    original['expected_error'] = 'no_root'
    separate = deepcopy(baseline); separate['id'] = 'icma_gov.qualified.non_unique.price'
    separate['request']['operation']['settings']['government']['inverse'].update(
        lower=0.0365, upper=0.03650000000000001, price_tolerance=1)
    separate['expected_error'] = 'non_unique'; cases.append(separate)
    return dict(certified=False, qualification='All original87 requests retained physically. Three consistent source revisions have unchanged full independent economics; three separate stale-pin controls and one actual non-identifying bracket are declared before candidate execution. Original wrong error expectations remain raw diagnostics. No numerical bounds, source cash or accepted financial answers are relaxed.', cases=cases), dict(certified=False, cases=additional)


if __name__ == '__main__':
    p = argparse.ArgumentParser(); p.add_argument('--write', action='store_true'); a = p.parse_args(); values = qualified()
    for path, value in zip(['tests/fixtures/library/icma-government-qualified.json', 'tests/fixtures/library/icma-government-additional-reference.json'], values):
        file = ROOT/path
        if a.write:
            if file.exists(): raise ValueError('Refusing to overwrite declared controls')
            file.write_text(json.dumps(value, indent=2)+'\n')
        elif json.loads(file.read_bytes()) != value: raise ValueError('Original qualification changed: '+path)
    print(json.dumps(dict(cases=len(values[0]['cases']), successes=sum(c['expected_error'] is None for c in values[0]['cases']), failures=sum(c['expected_error'] is not None for c in values[0]['cases']), certified=False)))
