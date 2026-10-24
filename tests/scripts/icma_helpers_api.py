"""Assemble all unchanged declared contextual helper requests for API coverage."""
import argparse,json
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
def corpus():
    cases=[]
    for file in ['icma-helpers-qualified.json','icma-helpers-boundaries.json']:
        cases.extend(json.loads((ROOT/'tests/fixtures/library'/file).read_bytes())['cases'])
    return dict(schema_version=1,certified=False,cases=cases)
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();v=corpus();path=ROOT/'tests/fixtures/library/icma-helpers-api.json';data=(json.dumps(v,indent=2)+'\n').encode()
    if a.write:
        if path.exists():raise ValueError('Refusing to overwrite original API request corpus')
        path.write_bytes(data)
    elif path.read_bytes()!=data:raise ValueError('Original API corpus changed')
    print(json.dumps(dict(requests=len(v['cases']),successes=sum(c['expected_error'] is None for c in v['cases']),certified=False)))
