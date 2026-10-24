"""Versioned operation inventory framing; preserve every qualified request."""
import argparse
import json
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2]
def corpus():
    value=json.loads((ROOT/'tests/fixtures/library/icma-government-qualified.json').read_bytes())
    value['schema_version']=1
    return value
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();value=corpus();file=ROOT/'tests/fixtures/library/icma-government-api-cases.json'
    if a.write:
        if file.exists():raise ValueError('Refusing to overwrite original API corpus declaration')
        file.write_text(json.dumps(value,indent=2)+'\n')
    elif json.loads(file.read_bytes())!=value:raise ValueError('Original API request framing changed')
    print(json.dumps(dict(schema_version=1,cases=len(value['cases']),all_original_qualified_requests_preserved=True,certified=False)))
