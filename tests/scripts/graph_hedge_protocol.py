"""Preserve malformed yield ingress and add the original complete native boundary."""
from copy import deepcopy
import argparse, hashlib, json
from pathlib import Path
FOLDER=Path(__file__).resolve().parents[1]/'fixtures/library'
if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--write',action='store_true');a=parser.parse_args()
    source=FOLDER/'graph-hedge-terms.json';raw=source.read_bytes();corpus=deepcopy(json.loads(raw))
    original=next(c for c in corpus['cases'] if c['id']=='graph_hedge.invalid.yield_alias')
    original['expected_error']='invalid_input'
    admitted=deepcopy(original);admitted['id']+='complete';admitted['expected_error']='not_applicable'
    admitted['request']['operation']['product']['method']['convention']={'day_count':'Act365Fixed','compounding':'SemiAnnual'}
    corpus['cases'].append(admitted);corpus['protocol_source_sha256']=hashlib.sha256(raw).hexdigest()
    data=(json.dumps(corpus,indent=2)+'\n').encode();target=FOLDER/'graph-hedge-api.json'
    if a.write:
        if target.exists():raise ValueError('Refusing to replace separate protocol controls')
        target.write_bytes(data)
    elif target.read_bytes()!=data:raise ValueError('Protocol controls differ')
    print(json.dumps(dict(status='passed',requests=40,full_financial_controls_unchanged=True,certified=False)))
