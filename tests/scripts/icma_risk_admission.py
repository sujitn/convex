"""Separate pre-execution coverage/framing admission; raw declarations stay intact."""
import argparse
from copy import deepcopy as clone
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DEST = ROOT / 'tests/fixtures/library'


def qualify():
    raw = json.loads((DEST / 'icma-risk.json').read_bytes())
    refs = json.loads((DEST / 'icma-risk-reference.json').read_bytes())
    cases = clone(raw['cases']); additional = []
    # Removing only an unused final nominal tail leaves full original cash
    # coverage. Retain that valid request and its unchanged complete answer.
    tail = next(c for c in cases if c['id'] == 'icma_risk.invalid.incomplete_context')
    tail['expected_error'] = None
    answer = clone(refs['cases'][0]); answer['id'] = tail['id']; additional.append(answer)
    missing = clone(tail); missing['id'] = 'icma_risk.invalid.actual_incomplete_context'
    context = missing['request']['operation']['input']['reference_context']
    maturity = missing['request']['operation']['input']['security']['bond']['terms']['schedule']['end_date']
    context['periods'] = [p for p in context['periods'] if p['end'] <= maturity]
    missing['expected_error'] = 'invalid_input'; cases.append(missing)
    # Keep the original malformed context; add a fully framed conditional path
    # to test the native prohibition of an unconditional fixed-yield proxy.
    malformed = next(c for c in cases if c['id'] == 'icma_risk.invalid.conditional_workout')
    malformed['expected_error'] = 'invalid_input'
    conditional = clone(malformed); conditional['id'] += '.admitted'
    context = conditional['request']['operation']['cash']['context']
    context.pop('information_date'); settle = context['settlement']
    context.update(reference_date=settle, trade_date=settle,
                   notice=dict(kind='unnotified', earliest_notice=settle))
    conditional['expected_error'] = 'not_applicable'; cases.append(conditional)
    return dict(certified=False, qualification='Raw requests/answers/domains/bounds remain immutable. '
                'Unused final nominal tail removal is valid with identical full independent cash answers. '
                'A separate actual delayed-payment coverage failure and fully framed conditional-workout '
                'failure are declared before first candidate execution.', cases=cases), dict(certified=False, cases=additional)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(); parser.add_argument('--write', action='store_true'); args = parser.parse_args()
    for name, value in zip(['icma-risk-admitted.json', 'icma-risk-admission-reference.json'], qualify()):
        path = DEST / name; data = (json.dumps(value, indent=2) + '\n').encode()
        if args.write:
            if path.exists(): raise ValueError('Refusing to overwrite declared admission')
            path.write_bytes(data)
        elif path.read_bytes() != data:
            raise ValueError('Original admission changed: ' + name)
    print(json.dumps(dict(status='passed', requests=len(qualify()[0]['cases']), certified=False)))
