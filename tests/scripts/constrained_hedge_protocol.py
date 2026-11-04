"""API-report wrapper only; every declared admitted/lot request remains unchanged."""
import argparse
import json
from pathlib import Path
FOLDER=Path(__file__).resolve().parents[1]/'fixtures/library'

def controls():
    cases=[]
    for name in ['constrained-hedge-admitted.json','constrained-hedge-lots.json']:
        cases+=json.loads((FOLDER/name).read_bytes())['cases']
    return {'schema_version':1,'certified':False,
            'qualification':'Original admitted and separately declared lot requests, unchanged financial answers/bounds. Supplied-model engineering evidence; no market, order, product-risk or latency certificate.',
            'cases':cases}

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--write',action='store_true');args=parser.parse_args()
    path=FOLDER/'constrained-hedge-protocol.json';content=json.dumps(controls(),indent=2)+'\n'
    if args.write:
        if path.exists():raise SystemExit('Refusing to overwrite protocol evidence')
        path.write_text(content)
    elif path.read_text()!=content:raise ValueError('Frozen complete protocol differs')
    print(json.dumps({'requests':len(controls()['cases']),'original_case_objects_unchanged':True,'certified':False}))
