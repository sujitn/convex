"""Explicit actual-pillar tenor admission; retain every original failed request."""
import argparse, copy, datetime, hashlib, json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DEST = ROOT / 'tests/fixtures/library'


def qualify():
    prior = json.loads((DEST / 'icma-helpers-qualified.json').read_bytes())
    refs = json.loads((DEST / 'icma-helpers-reference.json').read_bytes())['cases']
    refs += json.loads((DEST / 'icma-helpers-additional-reference.json').read_bytes())['cases']
    cases = copy.deepcopy(prior['cases'])
    answers = []
    for case in cases[:]:
        if case['expected_error'] is not None or case['request']['operation']['kind'] != 'calibration':
            continue
        action = case['request']['operation']['action']
        if action['kind'] not in ['scenario', 'product_risk']:
            continue
        actual = copy.deepcopy(case)
        actual['id'] += '.terms'
        key = actual['request']['operation']['action']['key']
        graph = actual['request']['operation']['input']
        entry = next(e for e in graph['cash'] if e['recipe']['curve']['key'] == key['coordinate']['curve'])
        helper = next(h['input'] for h in entry['recipe']['helpers'] if h['input']['id'] == key['coordinate']['helper_id'])
        origin = datetime.date.fromisoformat(entry['recipe']['curve']['reference_date'])
        pillar = datetime.date.fromisoformat(helper['pillar'])
        key['factor']['tenor'] = str((pillar-origin).days) + 'D'
        cases.append(actual)
        answer = copy.deepcopy(next(r for r in refs if r['id'] == case['id']))
        answer['id'] = actual['id']
        answers.append(answer)
        case['expected_error'] = 'invalid_input'
    bad = copy.deepcopy(next(c for c in cases if c['id'] == 'icma_helpers.graph.positive.scenario.terms'))
    bad['id'] = 'icma_helpers.invalid.admitted_risk_policy'
    bad['request']['operation']['action']['key']['factor']['convention_version'] = 99
    bad['expected_error'] = 'invalid_input'
    cases.append(bad)
    qualified = dict(certified=False, qualification='Original actual-cash tenor spelling is invalid under the existing exact D/W/M/Y contract. All preceding requests remain; six separately declared complete requests use the actual original pillar-to-reference day tenor. Economic inputs, native units, source/quote versions, bumps, all full financial answers and every bound remain unchanged.', cases=cases)
    api_cases = copy.deepcopy(cases)
    api_cases += json.loads((DEST / 'icma-helpers-boundaries.json').read_bytes())['cases']
    api = dict(schema_version=1, certified=False, cases=api_cases)
    return qualified, dict(certified=False, cases=answers), api


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--write', action='store_true')
    args = parser.parse_args()
    values = qualify()
    names = ['icma-helpers-terms.json', 'icma-helpers-terms-reference.json', 'icma-helpers-complete-api.json']
    for name, value in zip(names, values):
        path = DEST / name
        data = (json.dumps(value, indent=2) + '\n').encode()
        if args.write:
            if path.exists():
                raise ValueError('Refusing to overwrite declaration')
            path.write_bytes(data)
        elif path.read_bytes() != data:
            raise ValueError('Original tenor admission changed')
    print(json.dumps(dict(requests=len(values[2]['cases']), successes=sum(c['expected_error'] is None for c in values[2]['cases']), certified=False)))
