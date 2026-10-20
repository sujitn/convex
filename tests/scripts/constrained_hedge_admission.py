"""Separate wire admission; preserve raw controls, financial answers and bounds."""
import argparse
from copy import deepcopy
import hashlib
import json
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];FOLDER=ROOT/'tests/fixtures/library'

def admitted():
    original=json.loads((FOLDER/'constrained-hedge.json').read_bytes());value=deepcopy(original)
    def visit(node):
        if isinstance(node,dict):
            if isinstance(node.get('coordinate'),dict):
                curve=node['coordinate'].get('curve')
                if curve is not None:curve['version']=str(curve['version'])
            for child in node.values():visit(child)
        elif isinstance(node,list):
            for child in node:visit(child)
    visit(value)
    value['provenance']+=' Separate wire admission spells original CurveKey.version as the existing string type. No financial value, answer or bound changes.'
    return value

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--write',action='store_true');args=parser.parse_args()
    raw=json.dumps(admitted(),indent=2)+'\n';path=FOLDER/'constrained-hedge-admitted.json'
    if args.write:
        if path.exists():raise SystemExit('Refusing to overwrite frozen admitted controls')
        path.write_text(raw)
    else:
        if path.read_text()!=raw:raise ValueError('Frozen admitted controls differ')
    print(json.dumps({'requests':len(admitted()['cases']),'financial_answers_unchanged':True,'sha256':hashlib.sha256(raw.encode()).hexdigest(),'certified':False}))
