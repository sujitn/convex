"""Frame the unchanged full request corpus for the existing exact-route inventory."""
import argparse
import json
from pathlib import Path

DEST = Path(__file__).resolve().parents[1] / 'fixtures/library'


def corpus():
    value = json.loads((DEST / 'icma-risk-final-api.json').read_bytes())
    return dict(schema_version=1, **value)


if __name__ == '__main__':
    p = argparse.ArgumentParser(); p.add_argument('--write', action='store_true'); a = p.parse_args()
    value = corpus(); data = (json.dumps(value, indent=2) + '\n').encode(); path = DEST / 'icma-risk-routes.json'
    if a.write:
        if path.exists(): raise ValueError('Refusing to overwrite original route framing')
        path.write_bytes(data)
    elif path.read_bytes() != data: raise ValueError('Original request route framing changed')
    print(json.dumps(dict(status='passed', requests=len(value['cases']), certified=False)))
