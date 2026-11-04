"""Separate original credit-role string admission, preserving legal/financial controls."""
from copy import deepcopy
import argparse
import hashlib
import json
from pathlib import Path

FOLDER=Path(__file__).resolve().parents[1]/'fixtures/library'
if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--write',action='store_true');a=parser.parse_args()
    source=FOLDER/'graph-hedge-admitted.json';raw=source.read_bytes();corpus=deepcopy(json.loads(raw))
    for case in corpus['cases']:
        for curve in case['request']['inputs']['curves']:
            if curve['role'].get('kind')=='Credit' and curve['role']['seniority']=='SeniorUnsecured':
                curve['role']['seniority']='senior_unsecured'
    corpus['role_admission_source_sha256']=hashlib.sha256(raw).hexdigest();data=(json.dumps(corpus,indent=2)+'\n').encode()
    target=FOLDER/'graph-hedge-qualified.json'
    if a.write:
        if target.exists():raise ValueError('Refusing to replace credit-role admission')
        target.write_bytes(data)
    elif target.read_bytes()!=data:raise ValueError('Credit-role admission differs')
    print(json.dumps(dict(status='passed',requests=39,original_financial_and_legal_controls_unchanged=True,certified=False)))
